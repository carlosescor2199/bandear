use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_util::{Sink, SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::{EngineEvent, EventBus, MixOrigin};
use crate::health::LinkReport;
use crate::mix::GroupLayout;
use crate::protocol::{
    control_error, control_message_type, mix_ack, mix_error, mix_sequence, mix_state_message,
    parse_mix_update, parse_register_command, parse_stats_command, registered_name, ControlConfig,
    GroupConfig,
};
use crate::targets::TargetRegistry;

/// The error a peer whose IP is not a target gets, for every message that joins on the target.
const NO_TARGET_ERROR: &str = "no UDP target configured for client IP";

/// Counts the live control connections per client IP.
///
/// More than one socket can come from the same IP (a reload before the old one closes), so the
/// registry counts rather than flags; the engine reports a musician as connected while the count is
/// above zero. The IP is the join key between [MixState], the UDP target and the control peer.
pub(crate) type ControlPeers = Arc<Mutex<HashMap<IpAddr, usize>>>;

/// How often a control connection checks whether a device switch changed the channel count.
const CONFIG_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How often a control connection sends its client a WebSocket ping.
const CONTROL_PING_INTERVAL: Duration = Duration::from_secs(2);

/// How long a control connection may go without receiving any frame before it is declared dead.
///
/// Five ping intervals: a client answers each ping on its own (any WebSocket library does), so a
/// live phone is never silent this long, while one that vanished is let go within a glance.
const CONTROL_DEAD_AFTER: Duration = Duration::from_secs(10);

/// The control channel's heartbeat: how often a connection pings its client, and how long it may
/// receive nothing (no pong, message, ping or close) before it is closed.
///
/// A phone that drops off Wi-Fi never closes its socket, so without this its half-open connection
/// would stay counted as a live control peer forever. Closing it drops the connection's
/// [PeerGuard], which is what starts the grace period and shows the musician as gone.
#[derive(Clone, Copy, Debug)]
struct Heartbeat {
    ping_interval: Duration,
    dead_after: Duration,
}

impl Default for Heartbeat {
    fn default() -> Self {
        Self {
            ping_interval: CONTROL_PING_INTERVAL,
            dead_after: CONTROL_DEAD_AFTER,
        }
    }
}

/// The live control configuration shared between the engine and every connection.
///
/// A device switch can change the captured channel count while the server runs. The engine stores the
/// current count here; each connection notices a change on its next poll and re-sends `config` on the
/// same socket, so a client rebuilds its per-channel controls without losing the control channel.
pub(crate) struct ConfigState {
    channels: AtomicU8,
}

impl ConfigState {
    pub(crate) fn new(channels: u8) -> Self {
        Self {
            channels: AtomicU8::new(channels),
        }
    }

    pub(crate) fn current(&self) -> u8 {
        self.channels.load(Ordering::SeqCst)
    }

    /// Publishes a new channel count. Storing an unchanged count is harmless: a connection compares
    /// it against the count it last sent.
    pub(crate) fn set_channels(&self, channels: u8) {
        self.channels.store(channels, Ordering::SeqCst);
    }
}

/// Builds the `config` control message for a channel count and group layout.
///
/// Free of connection state so it can be sent on connect and re-sent on a device switch from the
/// same code, which is what keeps the two messages identical in shape.
fn config_message(source_channels: u8, groups: &GroupLayout) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ControlConfig {
        message_type: "config",
        source_channels,
        sample_rate: TARGET_SAMPLE_RATE,
        groups: groups
            .groups()
            .iter()
            .map(|group| GroupConfig {
                name: group.name.clone(),
                channels: group.channels.clone(),
            })
            .collect(),
    })
}

pub(crate) fn spawn_control_thread(
    control_port: u16,
    targets: Arc<TargetRegistry>,
    stopped: Arc<AtomicBool>,
    config_state: Arc<ConfigState>,
    groups: Arc<GroupLayout>,
    peers: ControlPeers,
    events: EventBus,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(("0.0.0.0", control_port)) {
            Ok(listener) => listener,
            Err(error) => {
                eprintln!("control listener bind error: {error}");
                return;
            }
        };
        if let Err(error) = listener.set_nonblocking(true) {
            eprintln!("control listener setup error: {error}");
            return;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                eprintln!("control runtime error: {error}");
                return;
            }
        };
        runtime.block_on(run_control_server(
            listener,
            targets,
            stopped,
            config_state,
            groups,
            peers,
            events,
            Heartbeat::default(),
        ));
    })
}

#[allow(clippy::too_many_arguments)]
async fn run_control_server(
    listener: std::net::TcpListener,
    targets: Arc<TargetRegistry>,
    stopped: Arc<AtomicBool>,
    config_state: Arc<ConfigState>,
    groups: Arc<GroupLayout>,
    peers: ControlPeers,
    events: EventBus,
    heartbeat: Heartbeat,
) {
    let listener = match TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("control listener error: {error}");
            return;
        }
    };
    loop {
        let accepted = tokio::select! {
            accepted = listener.accept() => Some(accepted),
            _ = tokio::time::sleep(Duration::from_millis(100)) => None,
        };
        if stopped.load(Ordering::Relaxed) {
            break;
        }
        // The accept loop already ticks a few times a second, so it is also the clock that removes
        // registered targets whose control channel stayed gone past the grace period.
        for address in targets.sweep_expired(Instant::now()) {
            println!("control registration {address} removed after the grace period");
        }
        match accepted {
            None => continue,
            Some(accepted) => match accepted {
                Ok((stream, peer)) => {
                    let targets = Arc::clone(&targets);
                    let config_state = Arc::clone(&config_state);
                    let groups = Arc::clone(&groups);
                    let peers = Arc::clone(&peers);
                    let events = events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_control_connection(
                            stream,
                            peer,
                            targets,
                            config_state,
                            groups,
                            peers,
                            events,
                            heartbeat,
                        )
                        .await
                        {
                            eprintln!("control connection {peer} error: {error}");
                        }
                    });
                }
                Err(error) => eprintln!("control accept error: {error}"),
            },
        }
    }
}

/// Removes one connection from the registry when the task ends, for any reason, and emits the
/// matching disconnect event on the last one. A guard keeps the bookkeeping correct across every
/// early return in the connection loop.
///
/// On the last connection for an IP, the peer's registered target is marked disconnected: it is
/// not removed here, because the grace period decides that, and it starts now. A connection whose
/// client vanished without closing is ended by the [Heartbeat], so its guard drops too.
struct PeerGuard {
    address: SocketAddr,
    peers: ControlPeers,
    targets: Arc<TargetRegistry>,
    events: EventBus,
}

impl Drop for PeerGuard {
    fn drop(&mut self) {
        let ip = self.address.ip();
        let mut peers = self.peers.lock().expect("control peer registry poisoned");
        match peers.get_mut(&ip) {
            Some(count) if *count > 1 => {
                *count -= 1;
            }
            _ => {
                peers.remove(&ip);
                drop(peers);
                self.targets.mark_disconnected(ip, Instant::now());
                self.events.emit(EngineEvent::ClientDisconnected {
                    address: self.address,
                });
            }
        }
    }
}

/// Whether a connection with `peer` should be sent its musician's mix because of `event`.
///
/// Only a mix change for the same IP counts, and never one this very connection made: the phone
/// already holds that mix and got `mix_ack` for it. A change from the engine, or from another
/// socket of the same phone, is pushed so the phone's screen follows.
fn should_push_mix(event: &EngineEvent, peer: SocketAddr) -> bool {
    match event {
        EngineEvent::MixChanged { address, origin } => {
            address.ip() == peer.ip() && *origin != MixOrigin::Client { peer }
        }
        _ => false,
    }
}

/// One connection's view of the live event bus: it only ever answers "push the mix now".
///
/// It owns the recovery rules, so the connection loop has a single branch for them:
///
/// - A change of this musician's mix that this connection did not make asks for a push.
/// - A feed that lagged behind the bus asks for a push too: an event for this musician may be
///   among the missed ones, and the push carries the whole mix, so sending it is always correct.
/// - A closed bus turns the feed off for good. The connection carries on without it, and the feed
///   parks instead of resolving at once on every poll, so the loop cannot spin on it. The
///   connection holds a sender of the bus itself, so today this can only follow a refactor.
struct MixPushFeed {
    events: broadcast::Receiver<EngineEvent>,
    peer: SocketAddr,
    open: bool,
}

impl MixPushFeed {
    fn new(events: broadcast::Receiver<EngineEvent>, peer: SocketAddr) -> Self {
        Self {
            events,
            peer,
            open: true,
        }
    }

    /// Resolves when this connection's phone must be sent its musician's whole mix.
    ///
    /// Cancel-safe, as a `select!` branch must be: it only awaits the broadcast's own cancel-safe
    /// `recv`, and it returns as soon as an event asks for a push, so dropping it loses nothing that
    /// mattered.
    async fn next_push(&mut self) {
        while self.open {
            match self.events.recv().await {
                Ok(event) if should_push_mix(&event, self.peer) => return,
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => return,
                Err(RecvError::Closed) => self.open = false,
            }
        }
        std::future::pending::<()>().await;
    }
}

/// What became of a write that had to finish by the heartbeat deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BoundedSend {
    Sent,
    /// The socket refused the write: the peer is gone.
    Failed,
    /// The write could not finish in time, which is as good as a missed heartbeat.
    TimedOut,
}

/// Sends one message on the shared writer, giving up at `deadline`.
///
/// A write to a vanished peer can stall once its buffers fill, and so can waiting for the writer
/// while another write is stuck on it, so both are bounded. The connection loop ends the
/// connection on anything but [BoundedSend::Sent].
async fn send_by<S>(
    writer: &tokio::sync::Mutex<S>,
    message: Message,
    deadline: tokio::time::Instant,
) -> BoundedSend
where
    S: Sink<Message> + Unpin,
{
    let send = async { writer.lock().await.send(message).await };
    match tokio::time::timeout_at(deadline, send).await {
        Ok(Ok(())) => BoundedSend::Sent,
        Ok(Err(_)) => BoundedSend::Failed,
        Err(_) => BoundedSend::TimedOut,
    }
}

/// Registers a live connection and returns the guard that deregisters it, so the caller never has
/// to remember to undo this on every exit path.
fn register_peer(
    peers: &ControlPeers,
    events: &EventBus,
    targets: &Arc<TargetRegistry>,
    address: SocketAddr,
) -> PeerGuard {
    let ip = address.ip();
    {
        let mut peers = peers.lock().expect("control peer registry poisoned");
        let count = peers.entry(ip).or_insert(0);
        *count += 1;
        if *count == 1 {
            events.emit(EngineEvent::ClientConnected { address });
        }
    }
    PeerGuard {
        address,
        peers: Arc::clone(peers),
        targets: Arc::clone(targets),
        events: events.clone(),
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_control_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    targets: Arc<TargetRegistry>,
    config_state: Arc<ConfigState>,
    groups: Arc<GroupLayout>,
    peers: ControlPeers,
    events: EventBus,
    heartbeat: Heartbeat,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let websocket = accept_async(stream).await?;
    let _peer = register_peer(&peers, &events, &targets, peer);
    // Subscribed before `config` goes out, so a client that has seen `config` cannot miss a mix
    // change made after it.
    let mut mix_pushes = MixPushFeed::new(events.subscribe_live(), peer);
    let (writer, mut reader) = websocket.split();

    // The sink is shared between the read loop and the config watcher through an async mutex. The
    // read loop only holds it while sending, never while parked in a read.
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let sent_channels = Arc::new(AtomicU8::new(config_state.current()));
    writer
        .lock()
        .await
        .send(Message::Text(
            config_message(sent_channels.load(Ordering::SeqCst), &groups)?.into(),
        ))
        .await?;

    // The watcher notices a device switch while the connection is idle and re-sends `config` on the
    // SAME socket, so a client rebuilds its per-channel controls without losing the control channel.
    let watcher_writer = Arc::clone(&writer);
    let watcher_state = Arc::clone(&config_state);
    let watcher_groups = Arc::clone(&groups);
    let watcher_sent = Arc::clone(&sent_channels);
    let watcher_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(CONFIG_POLL_INTERVAL).await;
            let current = watcher_state.current();
            if current == watcher_sent.load(Ordering::SeqCst) {
                continue;
            }
            watcher_sent.store(current, Ordering::SeqCst);
            let Ok(config) = config_message(current, &watcher_groups) else {
                break;
            };
            if watcher_writer
                .lock()
                .await
                .send(Message::Text(config.into()))
                .await
                .is_err()
            {
                break;
            }
        }
    });

    // Whether this connection has registered yet. Only its first registration starts a new link
    // session; a later one on the same socket is a rename.
    let mut session_registered = false;
    // The heartbeat: a ping every interval, and the connection is closed once nothing at all has
    // arrived for the dead-after window. A client answers pings on its own, so only a vanished one
    // (a phone off Wi-Fi leaves its socket half-open) goes that long without a frame.
    let mut last_inbound = tokio::time::Instant::now();
    let mut ping_timer = tokio::time::interval_at(
        last_inbound + heartbeat.ping_interval,
        heartbeat.ping_interval,
    );
    ping_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        // Safety net for a runtime that does not drive the watcher's timer while this task is parked
        // on a read: re-check the channel count before parking again, so the next client message also
        // re-sends `config`. It is a cheap atomic load when nothing changed.
        let current = config_state.current();
        if current != sent_channels.load(Ordering::SeqCst) {
            sent_channels.store(current, Ordering::SeqCst);
            writer
                .lock()
                .await
                .send(Message::Text(config_message(current, &groups)?.into()))
                .await?;
        }

        let dead_at = last_inbound + heartbeat.dead_after;
        // Biased so a frame that already arrived always counts before the deadline is judged.
        let message = tokio::select! {
            biased;
            message = reader.next() => message,
            _ = tokio::time::sleep_until(dead_at) => {
                log_missed_heartbeat(peer, heartbeat.dead_after);
                break;
            }
            // The feed applies the recovery rules (a lagged feed pushes, a closed one parks).
            _ = mix_pushes.next_push() => {
                // A peer whose target is gone has no mix to follow; it keeps its channel as before.
                let Some(mix_state) = targets.mix_state(peer.ip()) else {
                    continue;
                };
                let message = mix_state_message(mix_state.snapshot())?;
                // Bounded like the ping: a push that cannot be written in time means a dead peer.
                match send_by(&writer, Message::Text(message.into()), dead_at).await {
                    BoundedSend::Sent => continue,
                    BoundedSend::Failed => break,
                    BoundedSend::TimedOut => {
                        log_missed_heartbeat(peer, heartbeat.dead_after);
                        break;
                    }
                }
            }
            _ = ping_timer.tick() => {
                // A ping that cannot be sent in time is as good as no answer.
                match send_by(&writer, Message::Ping(Default::default()), dead_at).await {
                    BoundedSend::Sent => continue,
                    BoundedSend::Failed => break,
                    BoundedSend::TimedOut => {
                        log_missed_heartbeat(peer, heartbeat.dead_after);
                        break;
                    }
                }
            }
        };
        let Some(message) = message else {
            break;
        };
        let message = message?;
        last_inbound = tokio::time::Instant::now();
        match message {
            Message::Text(text) => {
                let message_type = control_message_type(&text).unwrap_or_default();
                let response = if message_type == "register" {
                    // A registration is the client announcing itself: the address comes from the
                    // socket and the port and name come from the message. Success is answered with
                    // nothing, so a client talking to a server that predates registration (which
                    // answers with its usual error or ignores it) behaves exactly as it did. The
                    // shapes of the existing messages are untouched; this only adds a type.
                    match parse_register_command(&text) {
                        Ok(command) => {
                            let name = registered_name(&command.name);
                            // A phone that announces a client id the registry already holds at
                            // another IP came back from a new address (DHCP after a Wi-Fi blip): its
                            // row moves here instead of a second one appearing. Nothing else changes
                            // for this connection: the moved row now lives at this peer's IP, and the
                            // old IP's leftover connection finds no row of its own when it closes.
                            let registration = targets.register(
                                peer.ip(),
                                command.udp_port,
                                name.clone(),
                                command.client_id.as_deref(),
                                &groups,
                            );
                            let address = registration.address;
                            // A new session starts with no link report: the one left by an earlier
                            // session describes a link that is gone, and kept it would turn stale
                            // while this session is connected and read as a lost link. A rename on
                            // this same session re-registers too, and keeps the current report.
                            if !session_registered {
                                targets.clear_link_report(peer.ip());
                                session_registered = true;
                            }
                            let moved = registration
                                .moved_from
                                .map(|from| format!(", moved from {from}"))
                                .unwrap_or_default();
                            match &name {
                                Some(name) => println!(
                                    "control registration from {peer}: {address} as \"{name}\"{moved}"
                                ),
                                None => println!(
                                    "control registration from {peer}: {address} (unnamed){moved}"
                                ),
                            }
                            None
                        }
                        Err(error) => Some(control_error(error)?),
                    }
                } else if message_type == "stats" {
                    // A link report joins on the peer IP exactly like a mix, so an IP with no target
                    // gets the same error and nothing is stored. Success is answered with nothing,
                    // like a registration: the phone sends one every second and needs no echo. The
                    // lock is only held for the store, never across an await.
                    match targets.link_report(peer.ip()) {
                        None => Some(control_error(NO_TARGET_ERROR.to_owned())?),
                        Some(latest) => match parse_stats_command(&text) {
                            Ok(command) => {
                                let report = LinkReport::from_command(&command, Instant::now());
                                *latest.lock().expect("link report lock poisoned") = Some(report);
                                None
                            }
                            Err(error) => Some(control_error(error)?),
                        },
                    }
                } else {
                    // The join is live: a peer whose target was removed at runtime finds no mix
                    // state here and gets the same error an IP that was never configured has always
                    // received. The control channel itself is deliberately left open.
                    //
                    // A `mix` may carry the phone's number for it; the answer, ack or error, carries
                    // it back. Any other type that ends up here is never numbered.
                    let seq = if message_type == "mix" {
                        mix_sequence(&text)
                    } else {
                        None
                    };
                    Some(match targets.mix_state(peer.ip()) {
                        None => mix_error(NO_TARGET_ERROR.to_owned(), seq)?,
                        Some(mix_state) => {
                            match parse_mix_update(&text)
                                .and_then(|update| mix_state.apply(&update))
                            {
                                Ok(values) => {
                                    events.emit(EngineEvent::MixChanged {
                                        address: peer,
                                        origin: MixOrigin::Client { peer },
                                    });
                                    mix_ack(values, seq)?
                                }
                                Err(error) => mix_error(error, seq)?,
                            }
                        }
                    })
                };
                if let Some(response) = response {
                    if writer
                        .lock()
                        .await
                        .send(Message::Text(response.into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(payload) => {
                if writer
                    .lock()
                    .await
                    .send(Message::Pong(payload))
                    .await
                    .is_err()
                {
                    break;
                }
            }
            _ => {}
        }
    }

    watcher_task.abort();
    Ok(())
}

/// The one line a connection closed by the heartbeat leaves in the log.
fn log_missed_heartbeat(peer: SocketAddr, dead_after: Duration) {
    println!(
        "control connection {peer} closed: nothing received for {dead_after:?}, heartbeat missed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::Group;

    fn drums() -> GroupLayout {
        GroupLayout::new(vec![Group {
            name: "Drums".to_owned(),
            channels: vec![0, 1],
        }])
    }

    #[test]
    fn config_message_carries_the_source_channels_and_groups() {
        let json = config_message(4, &drums()).expect("config should serialize");

        assert!(json.contains(r#""source_channels":4"#));
        assert!(json.contains(r#""groups":[{"name":"Drums","channels":[0,1]}]"#));
    }

    #[test]
    fn config_state_reports_the_latest_channel_count() {
        let state = ConfigState::new(2);

        assert_eq!(state.current(), 2);

        state.set_channels(4);

        assert_eq!(state.current(), 4);
    }

    use tokio_tungstenite::connect_async;

    type ClientStream = tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >;

    async fn read_source_channels(client: &mut ClientStream) -> Option<u8> {
        while let Some(message) = client.next().await {
            if let Ok(Message::Text(text)) = message {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                    if value.get("type").and_then(|kind| kind.as_str()) == Some("config") {
                        return value
                            .get("source_channels")
                            .and_then(|channels| channels.as_u64())
                            .map(|channels| channels as u8);
                    }
                }
            }
        }
        None
    }

    /// Reads until a message of `kind` arrives, returning its parsed body. Bounded by a deadline so
    /// a missing answer fails the test instead of hanging it.
    async fn read_message_of_type(
        client: &mut ClientStream,
        kind: &str,
    ) -> Option<serde_json::Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match tokio::time::timeout(remaining, client.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                        if value.get("type").and_then(|kind| kind.as_str()) == Some(kind) {
                            return Some(value);
                        }
                    }
                }
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
        None
    }

    /// Exercises the re-send over a real socket, without audio hardware: a client that connected
    /// with two channels learns about four on the same connection.
    ///
    /// The server runs on its own runtime thread, exactly as the engine runs it, so the client's
    /// runtime and the accept loop never share a scheduler.
    #[tokio::test]
    async fn a_channel_count_change_is_pushed_to_a_connected_client() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind control listener");
        let address = listener.local_addr().expect("listener address");
        let stopped = Arc::new(AtomicBool::new(false));
        let config_state = Arc::new(ConfigState::new(2));

        let server_stopped = Arc::clone(&stopped);
        let server_config = Arc::clone(&config_state);
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime should build");
            runtime.block_on(run_control_server(
                listener,
                Arc::new(
                    TargetRegistry::new(&[], &GroupLayout::default())
                        .expect("registry should build"),
                ),
                server_stopped,
                server_config,
                Arc::new(drums()),
                Arc::new(Mutex::new(HashMap::new())),
                EventBus::default(),
                Heartbeat::default(),
            ));
        });

        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), read_source_channels(&mut client))
                .await
                .expect("first config should arrive"),
            Some(2)
        );

        // The equivalent of the engine publishing a new device's channel count.
        config_state.set_channels(4);

        // The connection re-checks the channel count before each read, so any client message makes
        // it re-send `config`. A mix command is the message a musician's client would send anyway.
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");

        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), read_source_channels(&mut client))
                .await
                .expect("re-sent config should arrive"),
            Some(4)
        );

        stopped.store(true, Ordering::SeqCst);
        // The accept loop re-checks `stopped` on the next accept, so nudge it with one connection so
        // the server thread can exit deterministically.
        let _ = std::net::TcpStream::connect(address);
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || server.join()),
        )
        .await;
    }

    /// The removal semantics, exercised over a real socket: removing a target stops its audio but
    /// does NOT close its control channel. The next `mix` gets the existing no-target error, and the
    /// connection stays open for the one after it; adding the address back joins the peer again.
    #[tokio::test]
    async fn a_removed_targets_control_peer_keeps_its_channel_and_gets_the_no_udp_target_error() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind control listener");
        let address = listener.local_addr().expect("listener address");
        let stopped = Arc::new(AtomicBool::new(false));
        // The client connects from 127.0.0.1, so this target's IP is the one its control messages
        // join against.
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &GroupLayout::default(),
            )
            .expect("registry should build"),
        );

        let server_stopped = Arc::clone(&stopped);
        let server_targets = Arc::clone(&targets);
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime should build");
            runtime.block_on(run_control_server(
                listener,
                server_targets,
                server_stopped,
                Arc::new(ConfigState::new(2)),
                Arc::new(drums()),
                Arc::new(Mutex::new(HashMap::new())),
                EventBus::default(),
                Heartbeat::default(),
            ));
        });

        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), read_source_channels(&mut client))
                .await
                .expect("config should arrive"),
            Some(2)
        );

        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(&mut client, "mix_ack").await.is_some(),
            "a configured target's peer should be acknowledged"
        );

        targets
            .remove("127.0.0.1:50000")
            .expect("configured target should be removed");

        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        let error = read_message_of_type(&mut client, "error")
            .await
            .expect("the removed peer should get an error");
        assert_eq!(
            error.get("message").and_then(|message| message.as_str()),
            Some("no UDP target configured for client IP")
        );

        // The channel was not closed: a second message on the same socket gets the same answer.
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(&mut client, "error").await.is_some(),
            "the control channel should still be open after the removal"
        );

        // Adding the address back joins the peer again, with no reconnect.
        targets
            .add("127.0.0.1:50000", &GroupLayout::default())
            .expect("fresh address should be accepted");
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(&mut client, "mix_ack").await.is_some(),
            "a re-added target should be acknowledged on the same socket"
        );

        stopped.store(true, Ordering::SeqCst);
        let _ = std::net::TcpStream::connect(address);
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || server.join()),
        )
        .await;
    }

    /// Starts a control server on an ephemeral port and returns its address, the shared target
    /// registry, and the handles needed to stop it. Shared by the registration tests.
    fn start_registration_server(
        targets: Arc<TargetRegistry>,
    ) -> (SocketAddr, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        start_server(
            targets,
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
        )
    }

    /// Starts a control server like [start_registration_server], with the peer registry and the
    /// heartbeat supplied by the test, so a test can watch the per-IP connection count and run the
    /// heartbeat in milliseconds.
    fn start_server(
        targets: Arc<TargetRegistry>,
        peers: ControlPeers,
        heartbeat: Heartbeat,
    ) -> (SocketAddr, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        start_server_with_events(targets, peers, heartbeat, EventBus::default())
    }

    /// Starts a control server like [start_server] on the event bus the test supplies, so a test
    /// can change a mix through the engine path and watch what the connections do with it.
    fn start_server_with_events(
        targets: Arc<TargetRegistry>,
        peers: ControlPeers,
        heartbeat: Heartbeat,
        events: EventBus,
    ) -> (SocketAddr, Arc<AtomicBool>, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind control listener");
        let address = listener.local_addr().expect("listener address");
        // Non-blocking, as `spawn_control_thread` sets it. On Windows an accepted socket inherits
        // the listener's mode, and a blocking one would park the connection task inside its read,
        // where no heartbeat timer can wake it.
        listener
            .set_nonblocking(true)
            .expect("listener should be non-blocking");
        let stopped = Arc::new(AtomicBool::new(false));
        let server_stopped = Arc::clone(&stopped);
        let server = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("control runtime should build");
            runtime.block_on(run_control_server(
                listener,
                targets,
                server_stopped,
                Arc::new(ConfigState::new(2)),
                Arc::new(drums()),
                peers,
                events,
                heartbeat,
            ));
        });
        (address, stopped, server)
    }

    async fn stop_server(
        address: SocketAddr,
        stopped: &Arc<AtomicBool>,
        server: std::thread::JoinHandle<()>,
    ) {
        stopped.store(true, Ordering::SeqCst);
        // The accept loop re-checks `stopped` on the next accept, so nudge it with one connection.
        let _ = std::net::TcpStream::connect(address);
        let _ = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || server.join()),
        )
        .await;
    }

    /// The full path over real sockets: a registration creates a target at the socket's address
    /// (even when the message claims another one), the control join starts working, and the UDP
    /// send thread the engine runs delivers the next packet to the announced port.
    #[tokio::test]
    async fn a_registration_creates_a_target_that_starts_receiving_audio() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));

        // The "phone": its port is what the registration announces, and where its audio must land.
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let phone_port = phone.local_addr().expect("phone address").port();
        let target: SocketAddr = format!("127.0.0.1:{phone_port}")
            .parse()
            .expect("target address");

        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), read_source_channels(&mut client))
                .await
                .expect("config should arrive"),
            Some(2)
        );

        client
            .send(Message::Text(
                format!(
                    r#"{{"type":"register","name":"Ana","udp_port":{phone_port},"address":"10.0.0.99:9"}}"#
                )
                .into(),
            ))
            .await
            .expect("client should send");

        // The claimed address is ignored: the target is built from the socket's 127.0.0.1.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !targets.read().iter().any(|entry| entry.address == target) {
            assert!(
                std::time::Instant::now() < deadline,
                "the registration should create a target"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        {
            let entries = targets.read();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].address, target);
            assert_eq!(entries[0].origin.name(), Some("Ana"));
        }

        // The join is live: the registered peer's mix is acknowledged on the same socket.
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(&mut client, "mix_ack").await.is_some(),
            "a registered peer should be acknowledged"
        );

        // The same UDP send thread the engine runs: one packet in, one packet at the phone.
        let (sender, receiver) = std::sync::mpsc::sync_channel(4);
        let network = crate::network::spawn_network_thread(
            receiver,
            Arc::clone(&targets),
            Arc::new(crate::network::PacketStats {
                sent: std::sync::atomic::AtomicU64::new(0),
                discarded: std::sync::atomic::AtomicU64::new(0),
            }),
        )
        .expect("network thread should spawn");
        sender
            .send(crate::protocol::AudioPacket {
                channels: 2,
                sample_rate: 48_000,
                sequence: 0,
                samples: vec![0, 0],
            })
            .expect("packet should queue");
        let received = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                phone.set_read_timeout(Some(Duration::from_secs(4))).ok();
                let mut buffer = [0u8; 4096];
                phone
                    .recv(&mut buffer)
                    .map(|length| length > 0)
                    .unwrap_or(false)
            }),
        )
        .await
        .expect("audio should arrive within 5s")
        .expect("recv task should finish");
        assert!(received, "the registered phone should receive audio");

        drop(sender);
        network.join().expect("network thread should exit");
        stop_server(address, &stopped, server).await;
    }

    /// The rename path over real sockets, the exact sequence the Android client performs: register
    /// with one name, stream audio, then re-register with a different name on the same open socket
    /// and the same UDP port. The target keeps its mix state and the phone keeps receiving audio.
    ///
    /// The server already replaces the stored name on a re-registration (it rebuilds the origin, so
    /// the name cannot go stale while the grace period is only cancelled). This test pins that
    /// behaviour at the socket level, where the Android client actually meets it.
    #[tokio::test]
    async fn a_re_registration_updates_the_reported_name_while_the_audio_keeps_flowing() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));

        // The "phone": its port is what the registration announces, and where its audio must land.
        let phone = std::net::UdpSocket::bind("127.0.0.1:0").expect("phone socket");
        let phone_port = phone.local_addr().expect("phone address").port();
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");

        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert!(
            read_message_of_type(&mut client, "config").await.is_some(),
            "config should arrive"
        );

        client
            .send(Message::Text(
                format!(r#"{{"type":"register","name":"Ana","udp_port":{phone_port}}}"#).into(),
            ))
            .await
            .expect("client should send");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !targets
            .read()
            .iter()
            .any(|entry| entry.address.ip() == target_ip)
        {
            assert!(
                std::time::Instant::now() < deadline,
                "the registration should create a target"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mix_before = targets.mix_state(target_ip).expect("registered target");
        println!(
            "before rename: name={:?}",
            registered_name(&targets, target_ip)
        );

        // The same UDP send thread the engine runs: one packet in, one packet at the phone.
        let (sender, receiver) = std::sync::mpsc::sync_channel(8);
        let network = crate::network::spawn_network_thread(
            receiver,
            Arc::clone(&targets),
            Arc::new(crate::network::PacketStats {
                sent: std::sync::atomic::AtomicU64::new(0),
                discarded: std::sync::atomic::AtomicU64::new(0),
            }),
        )
        .expect("network thread should spawn");
        sender
            .send(crate::protocol::AudioPacket {
                channels: 2,
                sample_rate: 48_000,
                sequence: 0,
                samples: vec![0, 0],
            })
            .expect("packet should queue");
        let (phone, first_length) = receive_one_packet(phone).await;
        println!(
            "audio before rename: {} bytes at the phone",
            first_length.expect("the phone should receive audio before the rename")
        );

        // The rename: the same open WebSocket sends a second registration with the same UDP port.
        client
            .send(Message::Text(
                format!(r#"{{"type":"register","name":"Bea","udp_port":{phone_port}}}"#).into(),
            ))
            .await
            .expect("client should send");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while registered_name(&targets, target_ip).as_deref() != Some("Bea") {
            assert!(
                std::time::Instant::now() < deadline,
                "the re-registration should replace the stored name"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        println!(
            "after rename: name={:?}",
            registered_name(&targets, target_ip)
        );

        // The rename changed nothing but the label: same target, same mix allocation, same port.
        let entries = targets.read();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].address,
            format!("127.0.0.1:{phone_port}")
                .parse()
                .expect("target address")
        );
        drop(entries);
        assert!(Arc::ptr_eq(
            &mix_before,
            &targets.mix_state(target_ip).expect("kept target")
        ));

        // The audio keeps flowing to the same socket, with no reconnect anywhere in between.
        sender
            .send(crate::protocol::AudioPacket {
                channels: 2,
                sample_rate: 48_000,
                sequence: 1,
                samples: vec![0, 0],
            })
            .expect("packet should queue");
        let (_phone, second_length) = receive_one_packet(phone).await;
        println!(
            "audio after rename: {} bytes at the phone",
            second_length.expect("the phone should receive audio after the rename")
        );

        // The control channel is still the same live socket: its mix is acknowledged after the rename.
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(&mut client, "mix_ack").await.is_some(),
            "the renamed peer should still be acknowledged on the same socket"
        );

        client.close(None).await.expect("the client should close");
        drop(client);
        drop(sender);
        network.join().expect("network thread should exit");
        stop_server(address, &stopped, server).await;
    }

    /// The stored name for an IP, read straight from the registry the engine's status is built from.
    fn registered_name(targets: &TargetRegistry, ip: IpAddr) -> Option<String> {
        targets
            .read()
            .iter()
            .find(|entry| entry.address.ip() == ip)
            .and_then(|entry| entry.origin.name().map(str::to_owned))
    }

    /// Blocks on the phone socket until one datagram arrives, returning the socket and its length so
    /// the caller can keep listening. Bounded by a read timeout so a missing packet fails the test.
    async fn receive_one_packet(
        phone: std::net::UdpSocket,
    ) -> (std::net::UdpSocket, Option<usize>) {
        tokio::time::timeout(
            Duration::from_secs(5),
            tokio::task::spawn_blocking(move || {
                phone
                    .set_read_timeout(Some(Duration::from_secs(4)))
                    .expect("phone read timeout should be set");
                let mut buffer = [0u8; 4096];
                let length = phone.recv(&mut buffer).ok();
                (phone, length)
            }),
        )
        .await
        .expect("audio should arrive within 5s")
        .expect("recv task should finish")
    }

    /// Reads the next reply that is not a `config` push, so a test can tell which message answered.
    /// Bounded by a deadline so a missing answer fails the test instead of hanging it.
    async fn read_next_reply(client: &mut ClientStream) -> Option<serde_json::Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match tokio::time::timeout(remaining, client.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let value = serde_json::from_str::<serde_json::Value>(&text).ok()?;
                    if value.get("type").and_then(|kind| kind.as_str()) != Some("config") {
                        return Some(value);
                    }
                }
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
        None
    }

    async fn connect_client(address: SocketAddr) -> ClientStream {
        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert!(
            read_message_of_type(&mut client, "config").await.is_some(),
            "config should arrive"
        );
        client
    }

    const CLEAN_STATS: &str = r#"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":3,"underruns":1,"receive_timeouts":0}"#;

    /// A `stats` report from a target's peer is stored on that target and answered with nothing,
    /// like a registration; the `mix` that follows on the same socket is still acknowledged, so its
    /// acknowledgement is the very next reply.
    #[tokio::test]
    async fn a_stats_report_is_stored_on_the_peers_target_without_a_reply() {
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &GroupLayout::default(),
            )
            .expect("registry should build"),
        );
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");
        let slot = targets.link_report(target_ip).expect("configured target");
        let mut client = connect_client(address).await;
        let sent_at = Instant::now();

        client
            .send(Message::Text(CLEAN_STATS.into()))
            .await
            .expect("client should send");

        let deadline = Instant::now() + Duration::from_secs(5);
        let report = loop {
            if let Some(report) = *slot.lock().expect("report lock") {
                break report;
            }
            assert!(Instant::now() < deadline, "the report should be stored");
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert_eq!(report.interval_ms, 1_000);
        assert_eq!(report.packets_received, 200);
        assert_eq!(report.packets_lost, 3);
        assert_eq!(report.underruns, 1);
        assert_eq!(report.receive_timeouts, 0);
        assert!(report.received_at >= sent_at, "stamped on arrival");

        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        let reply = read_next_reply(&mut client).await.expect("a reply to mix");
        assert_eq!(
            reply.get("type").and_then(|kind| kind.as_str()),
            Some("mix_ack"),
            "the stats report must not be answered, so the next reply is the mix ack: {reply}"
        );

        stop_server(address, &stopped, server).await;
    }

    /// A `stats` report from an IP with no target gets the same error a `mix` gets, and nothing is
    /// created or stored for it.
    #[tokio::test]
    async fn a_stats_report_from_an_ip_with_no_target_gets_the_no_target_error() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let mut client = connect_client(address).await;

        client
            .send(Message::Text(CLEAN_STATS.into()))
            .await
            .expect("client should send");

        let reply = read_next_reply(&mut client).await.expect("an error reply");
        assert_eq!(
            reply.get("type").and_then(|kind| kind.as_str()),
            Some("error")
        );
        assert_eq!(
            reply.get("message").and_then(|message| message.as_str()),
            Some("no UDP target configured for client IP")
        );
        assert!(targets.read().is_empty(), "a report never creates a target");

        stop_server(address, &stopped, server).await;
    }

    /// An invalid `stats` report is answered with an error naming the problem and stores nothing.
    #[tokio::test]
    async fn an_invalid_stats_report_gets_an_error_and_stores_nothing() {
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &GroupLayout::default(),
            )
            .expect("registry should build"),
        );
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let slot = targets
            .link_report("127.0.0.1".parse().expect("target IP"))
            .expect("configured target");
        let mut client = connect_client(address).await;

        client
            .send(Message::Text(
                r#"{"type":"stats","interval_ms":0,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}"#.into(),
            ))
            .await
            .expect("client should send");

        let reply = read_next_reply(&mut client).await.expect("an error reply");
        assert_eq!(
            reply.get("type").and_then(|kind| kind.as_str()),
            Some("error")
        );
        let message = reply
            .get("message")
            .and_then(|message| message.as_str())
            .unwrap_or_default();
        assert!(
            message.contains("interval_ms"),
            "the error names the field: {message}"
        );
        assert!(slot.lock().expect("report lock").is_none());

        stop_server(address, &stopped, server).await;
    }

    async fn send_text(client: &mut ClientStream, text: &str) {
        client
            .send(Message::Text(text.into()))
            .await
            .expect("client should send");
    }

    /// A phone numbers its mixes with `seq`, and the answer to each one, an ack or an error, carries
    /// the same number back. A mix without one is answered without one, exactly as before.
    #[tokio::test]
    async fn the_answer_to_a_mix_echoes_its_sequence() {
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &GroupLayout::default(),
            )
            .expect("registry should build"),
        );
        let (address, stopped, server) = start_registration_server(targets);
        let mut client = connect_client(address).await;

        send_text(
            &mut client,
            r#"{"type":"mix","seq":7,"volume_percent":80,"max_level_percent":90,"muted":false}"#,
        )
        .await;
        let ack = read_next_reply(&mut client).await.expect("a reply to mix");
        assert_eq!(ack["type"], "mix_ack");
        assert_eq!(ack["seq"], 7);

        // More channels than the engine mixes, so the mix is refused.
        let oversized = format!(
            r#"{{"type":"mix","seq":8,"volume_percent":80,"max_level_percent":90,"muted":false,"channels":[{}]}}"#,
            vec!["100"; 33].join(",")
        );
        send_text(&mut client, &oversized).await;
        let refused = read_next_reply(&mut client).await.expect("a reply to mix");
        assert_eq!(refused["type"], "error");
        assert_eq!(refused["seq"], 8);

        send_text(
            &mut client,
            r#"{"type":"mix","volume_percent":70,"max_level_percent":90,"muted":false}"#,
        )
        .await;
        let unnumbered = read_next_reply(&mut client).await.expect("a reply to mix");
        assert_eq!(unnumbered["type"], "mix_ack");
        assert!(unnumbered.get("seq").is_none(), "{unnumbered}");

        stop_server(address, &stopped, server).await;
    }

    /// Only an answer to a mix carries a sequence. An error answering a registration, a stats report
    /// or a message of another type never does, whatever that message carried; a mix refused because
    /// the peer has no target still gets its own number back.
    #[tokio::test]
    async fn errors_answering_other_messages_never_carry_a_sequence() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(targets);
        let mut client = connect_client(address).await;

        for other in [
            r#"{"type":"stats","seq":4,"interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}"#,
            r#"{"type":"register","seq":5}"#,
            r#"{"type":"mix_preview","seq":6}"#,
        ] {
            send_text(&mut client, other).await;
            let reply = read_next_reply(&mut client).await.expect("an error reply");
            assert_eq!(reply["type"], "error", "{other}");
            assert!(reply.get("seq").is_none(), "{other} -> {reply}");
        }

        send_text(
            &mut client,
            r#"{"type":"mix","seq":9,"volume_percent":80,"max_level_percent":90,"muted":false}"#,
        )
        .await;
        let refused = read_next_reply(&mut client).await.expect("an error reply");
        assert_eq!(refused["type"], "error");
        assert_eq!(refused["message"], NO_TARGET_ERROR);
        assert_eq!(refused["seq"], 9);

        stop_server(address, &stopped, server).await;
    }

    /// The grace period over a real socket: dropping the control channel keeps the target, and the
    /// drop is what arms its eventual removal. The exact boundary is covered by the pure tests in
    /// `targets`; this proves the wiring between the connection guard and the sweep.
    #[tokio::test]
    async fn a_dropped_registration_keeps_the_target_for_the_grace_period_and_then_removes_it() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let target: SocketAddr = "127.0.0.1:50123".parse().expect("target address");

        let (mut client, _) = tokio::time::timeout(
            Duration::from_secs(5),
            connect_async(format!("ws://{address}")),
        )
        .await
        .expect("client should connect within 5s")
        .expect("client should connect");
        assert!(
            read_message_of_type(&mut client, "config").await.is_some(),
            "config should arrive"
        );
        client
            .send(Message::Text(
                r#"{"type":"register","name":"Ana","udp_port":50123}"#.into(),
            ))
            .await
            .expect("client should send");
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !targets.read().iter().any(|entry| entry.address == target) {
            assert!(
                std::time::Instant::now() < deadline,
                "the registration should create a target"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        client.close(None).await.expect("the client should close");
        drop(client);

        // The grace keeps the target: a sweep at the present time removes nothing.
        assert!(targets.sweep_expired(std::time::Instant::now()).is_empty());
        assert!(
            targets.read().iter().any(|entry| entry.address == target),
            "a brief drop must not cut the audio"
        );

        // Once the grace is past, the drop's mark makes the sweep remove the target.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let removed = loop {
            let removed = targets.sweep_expired(
                std::time::Instant::now()
                    + crate::targets::REGISTRATION_GRACE_PERIOD
                    + Duration::from_secs(1),
            );
            if !removed.is_empty() {
                break removed;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the dropped socket should mark the target for removal"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        };

        assert_eq!(removed, vec![target]);
        assert!(targets.read().is_empty());

        stop_server(address, &stopped, server).await;
    }

    const REGISTER_ANA: &str = r#"{"type":"register","name":"Ana","udp_port":50123}"#;

    /// Sends `text`, then a mix, and waits for the mix acknowledgement. A connection handles its
    /// messages in order, so once the acknowledgement arrives `text` has been fully handled.
    async fn send_then_sync(client: &mut ClientStream, text: &str) {
        client
            .send(Message::Text(text.into()))
            .await
            .expect("client should send");
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        assert!(
            read_message_of_type(client, "mix_ack").await.is_some(),
            "the mix sent after {text} should be acknowledged"
        );
    }

    /// A phone that comes back on a new control session starts as unknown, not as a lost link: the
    /// report from its earlier session is forgotten when it registers, so neither a phone that is
    /// about to report nor a build that never reports is shown red. The first report on the new
    /// session is judged as usual.
    #[tokio::test]
    async fn a_new_control_session_starts_with_no_link_report() {
        use crate::health::{link_status, LinkHealth, REPORT_STALE_AFTER};

        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");

        // The earlier session registers and reports, then goes away.
        let mut first = connect_client(address).await;
        send_then_sync(&mut first, REGISTER_ANA).await;
        send_then_sync(&mut first, CLEAN_STATS).await;
        let slot = targets.link_report(target_ip).expect("registered target");
        let earlier = (*slot.lock().expect("report lock")).expect("the earlier report is stored");
        first.close(None).await.expect("the client should close");
        drop(first);

        // The phone comes back on a new session. Judged once the earlier report is stale, with this
        // session connected, a kept report would read as a lost link.
        let mut second = connect_client(address).await;
        send_then_sync(&mut second, REGISTER_ANA).await;
        let later = earlier.received_at + REPORT_STALE_AFTER + Duration::from_secs(1);
        let report = *slot.lock().expect("report lock");
        assert_eq!(
            link_status(report.as_ref(), true, later).health,
            LinkHealth::Unknown,
            "a new session is grey until its first report, not red"
        );
        assert_eq!(report, None, "the earlier session's report is forgotten");

        // The first report on the new session is judged on its numbers as usual.
        send_then_sync(&mut second, CLEAN_STATS).await;
        let report = *slot.lock().expect("report lock");
        let fresh = report.expect("the new session's report is stored");
        let status = link_status(report.as_ref(), true, fresh.received_at);
        assert_eq!(
            status.health,
            LinkHealth::Warning,
            "one underrun is a warning"
        );
        assert!(
            status.underruns_per_second.is_some(),
            "a fresh report has rates"
        );

        stop_server(address, &stopped, server).await;
    }

    /// A rename re-registers on the same session, so it keeps the current report: a musician typing
    /// a name while streaming must not blank their light.
    #[tokio::test]
    async fn a_rename_on_the_same_session_keeps_the_link_report() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");
        let mut client = connect_client(address).await;
        send_then_sync(&mut client, REGISTER_ANA).await;
        send_then_sync(&mut client, CLEAN_STATS).await;
        let slot = targets.link_report(target_ip).expect("registered target");
        let before = *slot.lock().expect("report lock");
        assert!(before.is_some(), "the report is stored");

        send_then_sync(
            &mut client,
            r#"{"type":"register","name":"Bea","udp_port":50123}"#,
        )
        .await;

        assert_eq!(registered_name(&targets, target_ip).as_deref(), Some("Bea"));
        assert_eq!(
            *slot.lock().expect("report lock"),
            before,
            "a rename keeps the report"
        );

        stop_server(address, &stopped, server).await;
    }

    const RETURNING_PHONE_ID: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    /// The live DHCP case over a real socket: a phone registered from one IP comes back from
    /// another after a Wi-Fi blip, announcing the same client id. Its row moves to the new address
    /// with its mix and counters, the new session starts with no link report, and the old IP's
    /// half-open connection closing later (the heartbeat drops it) leaves the moved musician alone.
    #[tokio::test]
    async fn a_phone_returning_from_a_new_ip_with_its_client_id_keeps_its_row() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
        // The earlier session came from an IP this test cannot open a socket from, so its
        // registration and its still-open connection are placed in the registries directly, as the
        // connection task would have left them.
        let old_peer: SocketAddr = "192.168.1.10:57874".parse().expect("old peer");
        let old_connection = register_peer(&peers, &EventBus::default(), &targets, old_peer);
        targets.register(
            old_peer.ip(),
            50000,
            Some("Ana".to_owned()),
            Some(RETURNING_PHONE_ID),
            &GroupLayout::default(),
        );
        let mix = targets.mix_state(old_peer.ip()).expect("registered target");
        let counters = Arc::clone(&targets.read()[0].counters);
        *targets
            .link_report(old_peer.ip())
            .expect("registered target")
            .lock()
            .expect("report lock") = Some(LinkReport {
            interval_ms: 1_000,
            packets_received: 200,
            packets_lost: 0,
            underruns: 0,
            receive_timeouts: 0,
            received_at: Instant::now(),
        });
        let (address, stopped, server) = start_server(
            Arc::clone(&targets),
            Arc::clone(&peers),
            Heartbeat::default(),
        );
        let new_ip: IpAddr = "127.0.0.1".parse().expect("loopback IP");

        let mut client = connect_client(address).await;
        let register = format!(
            r#"{{"type":"register","name":"Ana","udp_port":50123,"client_id":"{RETURNING_PHONE_ID}"}}"#
        );
        send_then_sync(&mut client, &register).await;

        {
            let entries = targets.read();
            assert_eq!(entries.len(), 1, "one row, not a second one for the new IP");
            assert_eq!(
                entries[0].address,
                "127.0.0.1:50123"
                    .parse::<SocketAddr>()
                    .expect("target address")
            );
            assert!(Arc::ptr_eq(&mix, &entries[0].mix_state), "mix kept");
            assert!(
                Arc::ptr_eq(&counters, &entries[0].counters),
                "counters kept"
            );
            assert_eq!(
                *entries[0].link_report.lock().expect("report lock"),
                None,
                "a new session starts with no link report"
            );
        }
        assert!(targets.mix_state(old_peer.ip()).is_none());

        // The heartbeat later closes the old IP's half-open connection.
        drop(old_connection);

        assert_eq!(control_connections(&peers, old_peer.ip()), 0);
        assert_eq!(control_connections(&peers, new_ip), 1);
        assert!(
            !marked_disconnected(&targets, new_ip),
            "the moved musician stays connected"
        );
        assert!(targets
            .sweep_expired(Instant::now() + crate::targets::REGISTRATION_GRACE_PERIOD * 2)
            .is_empty());

        stop_server(address, &stopped, server).await;
    }

    /// The heartbeat at test speed: a ping every 50 ms, and a connection that receives nothing for
    /// 500 ms is closed.
    const TEST_HEARTBEAT: Heartbeat = Heartbeat {
        ping_interval: Duration::from_millis(50),
        dead_after: Duration::from_millis(500),
    };

    /// How many live control connections the peer registry counts for `ip`.
    fn control_connections(peers: &ControlPeers, ip: IpAddr) -> usize {
        peers
            .lock()
            .expect("peer registry lock")
            .get(&ip)
            .copied()
            .unwrap_or(0)
    }

    /// Whether the registered target at `ip` is marked disconnected, i.e. its grace period runs.
    fn marked_disconnected(targets: &TargetRegistry, ip: IpAddr) -> bool {
        targets.read().iter().any(|entry| {
            entry.address.ip() == ip
                && matches!(
                    entry.origin,
                    crate::targets::TargetOrigin::Registered {
                        disconnected_at: Some(_),
                        ..
                    }
                )
        })
    }

    /// Waits until `done` holds, reading no client meanwhile. False if it did not hold within 5 s.
    async fn wait_until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !done() {
            if Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        true
    }

    /// Keeps reading `client` until `limit` passes or `done` holds, and returns how many pings
    /// arrived. Reading is all a WebSocket client needs to answer the server's pings, exactly as
    /// the phone's library does on its own. Panics if the server closes the connection.
    async fn answer_pings_until(
        client: &mut ClientStream,
        limit: Duration,
        done: impl Fn() -> bool,
    ) -> usize {
        let deadline = Instant::now() + limit;
        let mut pings = 0;
        while Instant::now() < deadline && !done() {
            match tokio::time::timeout(Duration::from_millis(10), client.next()).await {
                Err(_) => {}
                Ok(Some(Ok(Message::Ping(_)))) => pings += 1,
                Ok(Some(Ok(_))) => {}
                Ok(other) => {
                    panic!("the server closed a connection that answers its pings: {other:?}")
                }
            }
        }
        pings
    }

    /// A phone that drops off Wi-Fi never closes its socket and never answers a ping again; a
    /// client that stops reading behaves the same. Its connection is closed once nothing has
    /// arrived for the dead-after window, so the IP has no control connection left and the target's
    /// grace period starts.
    #[tokio::test]
    async fn a_client_that_stops_answering_the_heartbeat_is_dropped_and_marked_disconnected() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
        let (address, stopped, server) =
            start_server(Arc::clone(&targets), Arc::clone(&peers), TEST_HEARTBEAT);
        let ip: IpAddr = "127.0.0.1".parse().expect("loopback IP");

        let mut client = connect_client(address).await;
        let last_sent = Instant::now();
        send_then_sync(&mut client, REGISTER_ANA).await;
        assert_eq!(control_connections(&peers, ip), 1);
        assert!(!marked_disconnected(&targets, ip));

        // From here on the client is never read again, so it answers no ping.
        assert!(
            wait_until(|| control_connections(&peers, ip) == 0).await,
            "the silent connection should be closed"
        );
        assert!(
            last_sent.elapsed() >= TEST_HEARTBEAT.dead_after,
            "closed only once the dead-after window passed, after {:?}",
            last_sent.elapsed()
        );
        assert!(
            marked_disconnected(&targets, ip),
            "closing the last connection starts the grace period"
        );

        drop(client);
        stop_server(address, &stopped, server).await;
    }

    /// A client that keeps reading answers every ping by itself, so its connection stays open
    /// across many ping intervals and several dead-after windows while it sends no message at all.
    /// This is how a phone build that knows nothing of the heartbeat keeps working.
    #[tokio::test]
    async fn a_client_that_answers_the_heartbeat_stays_connected() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
        let (address, stopped, server) =
            start_server(Arc::clone(&targets), Arc::clone(&peers), TEST_HEARTBEAT);
        let ip: IpAddr = "127.0.0.1".parse().expect("loopback IP");
        let mut client = connect_client(address).await;
        send_then_sync(&mut client, REGISTER_ANA).await;

        let pings = answer_pings_until(&mut client, TEST_HEARTBEAT.dead_after * 3, || false).await;

        assert!(pings >= 5, "the server pings every interval, got {pings}");
        assert_eq!(control_connections(&peers, ip), 1, "it stays connected");
        assert!(!marked_disconnected(&targets, ip));
        // The connection still carries messages as before.
        send_then_sync(&mut client, CLEAN_STATS).await;

        stop_server(address, &stopped, server).await;
    }

    /// The live scenario: a phone drops off Wi-Fi, leaving its first connection half-open, comes
    /// back on a second connection from the same IP, and then the app is closed, which resets the
    /// second one without a closing handshake. The heartbeat closes the silent first connection
    /// while the second keeps the musician connected, so the reset leaves the IP with no control
    /// connection and the grace period starts.
    #[tokio::test]
    async fn a_half_open_connection_left_by_a_reconnect_does_not_keep_the_musician_connected() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
        let (address, stopped, server) =
            start_server(Arc::clone(&targets), Arc::clone(&peers), TEST_HEARTBEAT);
        let ip: IpAddr = "127.0.0.1".parse().expect("loopback IP");

        let mut older = connect_client(address).await;
        let older_last_sent = Instant::now();
        send_then_sync(&mut older, REGISTER_ANA).await;
        // The older connection goes silent: it is never read again.
        let mut newer = connect_client(address).await;
        send_then_sync(&mut newer, REGISTER_ANA).await;
        assert_eq!(control_connections(&peers, ip), 2);

        // The newer connection keeps answering pings while the heartbeat closes the older one.
        answer_pings_until(&mut newer, Duration::from_secs(5), || {
            control_connections(&peers, ip) < 2
        })
        .await;
        assert_eq!(
            control_connections(&peers, ip),
            1,
            "only the silent connection is closed"
        );
        assert!(older_last_sent.elapsed() >= TEST_HEARTBEAT.dead_after);
        assert!(
            !marked_disconnected(&targets, ip),
            "the newer connection keeps the musician connected"
        );

        // Closing the app resets the newer connection.
        drop(newer);
        assert!(
            wait_until(|| control_connections(&peers, ip) == 0).await,
            "no control connection is left for the IP"
        );
        assert!(
            marked_disconnected(&targets, ip),
            "closing the last connection starts the grace period"
        );

        drop(older);
        stop_server(address, &stopped, server).await;
    }

    #[test]
    fn a_mix_change_is_pushed_to_its_musicians_connections_except_the_one_that_made_it() {
        let phone: SocketAddr = "192.168.1.3:41000".parse().unwrap();
        let second_socket: SocketAddr = "192.168.1.3:41001".parse().unwrap();
        let other_phone: SocketAddr = "192.168.1.4:41000".parse().unwrap();
        let from_engine = EngineEvent::MixChanged {
            address: "192.168.1.3:50000".parse().unwrap(),
            origin: MixOrigin::Engine,
        };
        let from_phone = EngineEvent::MixChanged {
            address: phone,
            origin: MixOrigin::Client { peer: phone },
        };

        assert!(should_push_mix(&from_engine, phone));
        assert!(!should_push_mix(&from_engine, other_phone));
        assert!(
            !should_push_mix(&from_phone, phone),
            "a phone's own change is answered with mix_ack, never pushed back"
        );
        assert!(
            should_push_mix(&from_phone, second_socket),
            "another socket of the same musician still follows the change"
        );
        assert!(!should_push_mix(&from_phone, other_phone));
        assert!(!should_push_mix(
            &EngineEvent::ClientConnected { address: phone },
            phone
        ));
    }

    /// Starts a server with one configured target at the loopback IP the test client connects
    /// from, on an event bus the test shares, and returns the connected client too.
    async fn start_mix_push_server() -> (
        Arc<TargetRegistry>,
        EventBus,
        ClientStream,
        SocketAddr,
        Arc<AtomicBool>,
        std::thread::JoinHandle<()>,
    ) {
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &GroupLayout::default(),
            )
            .expect("registry should build"),
        );
        let events = EventBus::default();
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
        );
        // The connection subscribes before it sends `config`, so once `config` has arrived a change
        // made through the engine cannot be missed.
        let client = connect_client(address).await;
        (targets, events, client, address, stopped, server)
    }

    /// The desktop's path over a real socket: a change made through the engine API reaches the
    /// musician's phone as `mix_state`, carrying the whole resulting mix rather than the delta.
    #[tokio::test]
    async fn a_mix_change_made_through_the_engine_reaches_the_phone_as_mix_state() {
        let (targets, events, mut client, address, stopped, server) = start_mix_push_server().await;

        let applied = crate::engine::set_target_mix(
            &targets,
            &events,
            "127.0.0.1:50000",
            &crate::mix::MixUpdate {
                channel_gains: Some(vec![10, 20]),
                volume_percent: Some(42),
                ..Default::default()
            },
        )
        .expect("the configured target's mix is set");

        let pushed = read_message_of_type(&mut client, "mix_state")
            .await
            .expect("the phone should be told about the change");
        assert_eq!(pushed["volume_percent"], 42);
        assert_eq!(pushed["max_level_percent"], 100);
        assert_eq!(pushed["muted"], false);
        let channels: Vec<u8> = serde_json::from_value(pushed["channels"].clone()).unwrap();
        assert_eq!(channels, applied.channel_gains.to_vec());
        assert_eq!(&channels[..3], &[10, 20, 100]);
        let pans: Vec<u8> = serde_json::from_value(pushed["pans"].clone()).unwrap();
        assert_eq!(pans, applied.pans.to_vec());
        assert_eq!(pushed["mutes"].as_array().map(Vec::len), Some(32));
        assert_eq!(pushed["solos"].as_array().map(Vec::len), Some(32));
        assert_eq!(pushed["group_levels"].as_array().map(Vec::len), Some(16));

        stop_server(address, &stopped, server).await;
    }

    /// A phone's own change is answered with `mix_ack` and is never pushed back to it. Proven by
    /// order: the phone's own change is announced before any later one, so if it were echoed the
    /// echo would be the next message, ahead of the push for the engine change that follows.
    #[tokio::test]
    async fn a_phones_own_mix_is_acknowledged_and_not_pushed_back() {
        let (targets, events, mut client, address, stopped, server) = start_mix_push_server().await;

        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        let ack = read_next_reply(&mut client).await.expect("a reply to mix");
        assert_eq!(ack["type"], "mix_ack", "the phone's change is acknowledged");
        assert_eq!(ack["volume_percent"], 80);

        crate::engine::set_target_mix(
            &targets,
            &events,
            "127.0.0.1:50000",
            &crate::mix::MixUpdate {
                volume_percent: Some(33),
                ..Default::default()
            },
        )
        .expect("the configured target's mix is set");

        let next = read_next_reply(&mut client)
            .await
            .expect("the engine change is pushed");
        assert_eq!(next["type"], "mix_state");
        assert_eq!(
            next["volume_percent"], 33,
            "the next message is the engine's change, not an echo of the phone's own: {next}"
        );
        assert_eq!(next["max_level_percent"], 90, "the phone's values are kept");

        stop_server(address, &stopped, server).await;
    }

    const PHONE: &str = "192.168.1.3:41000";

    fn engine_change(address: &str) -> EngineEvent {
        EngineEvent::MixChanged {
            address: address.parse().unwrap(),
            origin: MixOrigin::Engine,
        }
    }

    /// A feed that fell behind the bus may have missed a change for its musician, so it asks for
    /// the whole current mix, which is always correct to send.
    #[tokio::test]
    async fn a_feed_that_lagged_asks_for_the_whole_mix() {
        let (bus, receiver) = broadcast::channel(1);
        let mut feed = MixPushFeed::new(receiver, PHONE.parse().unwrap());
        for other in [
            "192.168.1.4:50000",
            "192.168.1.5:50000",
            "192.168.1.6:50000",
        ] {
            bus.send(engine_change(other))
                .expect("the feed is subscribed");
        }

        assert!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push())
                .await
                .is_ok(),
            "none of the events was for this phone, but some were missed"
        );
    }

    /// Events that are not a change of this phone's mix by someone else are passed over, and the
    /// feed keeps waiting until one is.
    #[tokio::test]
    async fn a_feed_waits_through_events_that_are_not_for_its_phone() {
        let phone: SocketAddr = PHONE.parse().unwrap();
        let (bus, receiver) = broadcast::channel(16);
        let mut feed = MixPushFeed::new(receiver, phone);
        bus.send(EngineEvent::ClientConnected { address: phone })
            .expect("the feed is subscribed");
        bus.send(EngineEvent::MixChanged {
            address: phone,
            origin: MixOrigin::Client { peer: phone },
        })
        .expect("the feed is subscribed");
        bus.send(engine_change("192.168.1.4:50000"))
            .expect("the feed is subscribed");

        assert!(
            tokio::time::timeout(Duration::from_millis(100), feed.next_push())
                .await
                .is_err(),
            "nothing so far is a change this phone has to be told about"
        );

        bus.send(engine_change("192.168.1.3:50000"))
            .expect("the feed is subscribed");
        assert!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push())
                .await
                .is_ok()
        );
    }

    /// Counts the times a future asks to be polled again.
    struct WakeCounter(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for WakeCounter {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// A closed bus turns the feed off for good without ending anything: the feed parks and never
    /// asks to be polled again, so the connection's loop neither ends nor spins on it.
    ///
    /// The connection itself holds a sender of the bus it listens to, so in the server the bus can
    /// only close if that ever changes; this is the guard for that case.
    #[test]
    fn a_closed_feed_parks_for_good_instead_of_spinning() {
        use std::future::Future;

        let (bus, receiver) = broadcast::channel::<EngineEvent>(16);
        let mut feed = MixPushFeed::new(receiver, PHONE.parse().unwrap());
        drop(bus);
        let wakes = Arc::new(WakeCounter(std::sync::atomic::AtomicUsize::new(0)));
        let waker = std::task::Waker::from(Arc::clone(&wakes));
        let mut context = std::task::Context::from_waker(&waker);

        for _ in 0..3 {
            let next = std::pin::pin!(feed.next_push());
            assert!(
                next.poll(&mut context).is_pending(),
                "a closed feed never resolves"
            );
        }

        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            wakes.0.load(Ordering::SeqCst),
            0,
            "a parked feed must not ask to be polled again"
        );
    }

    /// A sink whose peer is gone: every write fails.
    struct BrokenSink;

    impl Sink<Message> for BrokenSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Err(std::io::ErrorKind::BrokenPipe.into()))
        }

        fn start_send(self: std::pin::Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }
    }

    /// A sink whose peer stopped reading: its buffers are full, so a write never gets going.
    struct StalledSink;

    impl Sink<Message> for StalledSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Pending
        }

        fn start_send(self: std::pin::Pin<&mut Self>, _: Message) -> Result<(), Self::Error> {
            Ok(())
        }

        fn poll_flush(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Pending
        }

        fn poll_close(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Pending
        }
    }

    fn soon() -> tokio::time::Instant {
        tokio::time::Instant::now() + Duration::from_millis(100)
    }

    /// The outcomes the connection loop acts on: a sent push or ping carries on, and a failed or
    /// late one ends the connection (the loop maps both to `break`).
    #[tokio::test]
    async fn a_bounded_send_reports_sent_failed_and_timed_out() {
        let open = tokio::sync::Mutex::new(futures_util::sink::drain());
        let broken = tokio::sync::Mutex::new(BrokenSink);
        let stalled = tokio::sync::Mutex::new(StalledSink);

        assert_eq!(
            send_by(&open, Message::Text("{}".into()), soon()).await,
            BoundedSend::Sent
        );
        assert_eq!(
            send_by(&broken, Message::Text("{}".into()), soon()).await,
            BoundedSend::Failed
        );
        assert_eq!(
            send_by(&stalled, Message::Text("{}".into()), soon()).await,
            BoundedSend::TimedOut
        );
    }

    /// A write that cannot even take the writer before the deadline (another write holds it, stuck
    /// on the same dead peer) is late too.
    #[tokio::test]
    async fn a_bounded_send_waiting_for_a_stuck_writer_times_out() {
        let writer = tokio::sync::Mutex::new(futures_util::sink::drain());
        let _stuck = writer.lock().await;

        assert_eq!(
            send_by(&writer, Message::Ping(Default::default()), soon()).await,
            BoundedSend::TimedOut
        );
    }
}
