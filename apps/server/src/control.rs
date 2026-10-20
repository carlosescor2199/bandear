use std::collections::HashMap;
use std::error::Error;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use futures_util::{Sink, SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio::sync::broadcast::{self, error::RecvError};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::{
    channel_flags, fit_channels, ChannelMeta, EngineEvent, EventBus, GroupStatus, MixOrigin,
};
use crate::health::LinkReport;
use crate::link_history::{LinkMoment, PingClock};
use crate::mix::{GroupLayout, Membership, MixState};
use crate::protocol::{
    control_error, control_message_type, mix_ack, mix_error, mix_sequence, mix_state_message,
    parse_mix_update, parse_register_command, parse_stats_command, playback_message,
    redundancy_message, registered_name, ChannelConfig, ControlConfig, GroupConfig,
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

/// How often a control connection checks whether the live configuration changed.
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
///
/// Each ping carries an 8-byte number, and the pong that brings it back measures the control
/// channel's round trip for the musician's link history (see [crate::link_history]): a client only
/// has to answer pings as WebSocket requires, echoing their payload.
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

/// What `config` tells a phone, as one immutable value: the captured channel count, the groups and
/// the channel details.
///
/// A change of what it says publishes a whole new snapshot with a higher generation (see
/// [ConfigState::publish]), so a reader never sees a channel count paired with the groups of
/// another configuration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConfigSnapshot {
    /// Which published configuration this is: every change has a higher one, so a connection can
    /// tell whether its phone already has it.
    pub(crate) generation: u64,
    pub(crate) source_channels: u8,
    /// The engineer's groups as the status reports them: 0-based channels, and whether the device
    /// has every channel a group names.
    pub(crate) groups: Vec<GroupStatus>,
    /// The membership the mixer applies: an invalid group keeps its position, so group levels stay
    /// aligned with the list, but maps no channel.
    pub(crate) layout: GroupLayout,
    /// The engineer's channel details as clients see them: exactly one entry per source channel,
    /// fitted from [Self::requested_channels] (see [fit_channels]).
    pub(crate) channels: Vec<ChannelMeta>,
    /// The engineer's channel details as set, whatever the device (at most 32 entries, checked).
    ///
    /// They are the source of truth, as the group definitions are: a device switch only changes
    /// what is fitted from them, so the details of inputs a smaller device lacks come back with a
    /// device that has them, names, pairs and hidden spares included. The mixer's flags are taken
    /// from them (see [channel_flags]): a spare stays hidden while its input is missing, and a pair
    /// whose second side is missing is shown as its first input alone but stays linked in every
    /// mix, so it returns with its balance and with both sides equal.
    pub(crate) requested_channels: Vec<ChannelMeta>,
}

/// Every snapshot field but the generation, which [ConfigState] assigns.
struct ConfigParts {
    source_channels: u8,
    groups: Vec<GroupStatus>,
    layout: GroupLayout,
    channels: Vec<ChannelMeta>,
    requested_channels: Vec<ChannelMeta>,
}

/// A published change: the configuration it replaced and the one now in force.
pub(crate) type Published = (Arc<ConfigSnapshot>, Arc<ConfigSnapshot>);

/// The live control configuration, shared by the engine and every connection.
///
/// The engine publishes a new snapshot whenever what `config` says changes: the engineer edits the
/// groups or the channel details, or a device switch changes the channel count (and with it which
/// groups are valid and which channel details remain). Each
/// connection notices the higher generation and re-sends `config` on the same socket, so a client
/// rebuilds its controls without losing the control channel, and a phone that registers takes the
/// live membership for its mix. The generation is an atomic, so a connection's poll is one load; the
/// snapshot behind its lock is only read when it changed.
pub(crate) struct ConfigState {
    current: RwLock<Arc<ConfigSnapshot>>,
    generation: AtomicU64,
}

impl ConfigState {
    /// The configuration an engine starts with: its channel count and groups, and no channel
    /// details yet (every input has the defaults).
    pub(crate) fn new(source_channels: u8, groups: Vec<GroupStatus>, layout: GroupLayout) -> Self {
        let first = ConfigSnapshot {
            generation: 1,
            source_channels,
            groups,
            layout,
            channels: fit_channels(&[], source_channels),
            requested_channels: Vec::new(),
        };
        Self {
            current: RwLock::new(Arc::new(first)),
            generation: AtomicU64::new(1),
        }
    }

    /// The generation of the configuration in force: one atomic load, for a connection's poll.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// The configuration in force.
    pub(crate) fn snapshot(&self) -> Arc<ConfigSnapshot> {
        Arc::clone(
            &self
                .current
                .read()
                .expect("control configuration lock poisoned"),
        )
    }

    /// Publishes a channel count and groups, fitting the engineer's channel details to the inputs
    /// the count has (see [ConfigSnapshot::requested_channels]). Returns the configuration it
    /// replaced and the one now in force, or `None` when nothing changed: an unchanged
    /// configuration keeps its generation, so no connection re-sends `config` for nothing.
    pub(crate) fn publish(
        &self,
        source_channels: u8,
        groups: Vec<GroupStatus>,
        layout: GroupLayout,
    ) -> Option<Published> {
        self.publish_with(|current| ConfigParts {
            source_channels,
            groups,
            layout,
            channels: fit_channels(&current.requested_channels, source_channels),
            requested_channels: current.requested_channels.clone(),
        })
    }

    /// Publishes the engineer's channel details: they are kept whole and fitted to the channel
    /// count in force (see [fit_channels]), as [Self::publish] publishes groups.
    pub(crate) fn publish_channels(&self, channels: &[ChannelMeta]) -> Option<Published> {
        self.publish_with(|current| ConfigParts {
            source_channels: current.source_channels,
            groups: current.groups.clone(),
            layout: current.layout.clone(),
            channels: fit_channels(channels, current.source_channels),
            requested_channels: channels.to_vec(),
        })
    }

    /// Builds the next configuration from the one in force and publishes it unless it is the same.
    ///
    /// The whole step runs under the write lock, and the generation is raised while the new
    /// snapshot is held there, so a reader that sees the new generation always reads the new
    /// snapshot and two publishes never build on the same old one. The generation only rises when
    /// what `config` says changed: details kept for inputs the device lacks are stored with the
    /// generation in force, since no client would see a difference.
    fn publish_with(&self, next: impl FnOnce(&ConfigSnapshot) -> ConfigParts) -> Option<Published> {
        let mut current = self
            .current
            .write()
            .expect("control configuration lock poisoned");
        let parts = next(&current);
        let seen_unchanged = current.source_channels == parts.source_channels
            && current.groups == parts.groups
            && current.layout == parts.layout
            && current.channels == parts.channels;
        if seen_unchanged && current.requested_channels == parts.requested_channels {
            return None;
        }
        let generation = if seen_unchanged {
            current.generation
        } else {
            current.generation + 1
        };
        let live = Arc::new(ConfigSnapshot {
            generation,
            source_channels: parts.source_channels,
            groups: parts.groups,
            layout: parts.layout,
            channels: parts.channels,
            requested_channels: parts.requested_channels,
        });
        let previous = std::mem::replace(&mut *current, Arc::clone(&live));
        self.generation.store(live.generation, Ordering::SeqCst);
        Some((previous, live))
    }
}

/// A phone registering, or a target added by address, gets the membership and the channel flags in
/// force at that moment.
impl Membership for ConfigState {
    fn new_mix(&self) -> MixState {
        let config = self.snapshot();
        MixState::configured(&config.layout, channel_flags(&config.requested_channels))
    }
}

/// Builds the `config` control message for one configuration.
///
/// Free of connection state so it can be sent on connect and re-sent on a change from the same
/// code, which is what keeps the two messages identical in shape. The groups are the mixer's: an
/// invalid group is announced in its position with no channel, so the phone's group levels, which
/// are positional, stay aligned with the server's. The channel details hold one entry per source
/// channel (see [ChannelConfig]).
fn config_message(config: &ConfigSnapshot) -> Result<String, serde_json::Error> {
    serde_json::to_string(&ControlConfig {
        message_type: "config",
        source_channels: config.source_channels,
        sample_rate: TARGET_SAMPLE_RATE,
        groups: config
            .layout
            .groups()
            .iter()
            .map(|group| GroupConfig {
                id: group.id.clone(),
                name: group.name.clone(),
                channels: group.channels.clone(),
            })
            .collect(),
        channels: config
            .channels
            .iter()
            .map(|channel| ChannelConfig {
                name: channel.name.clone(),
                kind: channel.kind.name(),
                hidden: channel.hidden,
                link: channel.linked_to_next,
            })
            .collect(),
    })
}

/// One connection's view of the live configuration: the generation its phone was last sent.
///
/// The connection loop and its watcher share it, and both send through [send_with_config], which
/// checks and sends while holding the connection's writer, so each configuration goes out once and
/// in order.
struct ConfigFeed {
    state: Arc<ConfigState>,
    sent: AtomicU64,
}

impl ConfigFeed {
    /// A feed whose phone has been sent nothing yet: generations start at 1, so the first check
    /// always finds `config` to send.
    fn new(state: Arc<ConfigState>) -> Self {
        Self {
            state,
            sent: AtomicU64::new(0),
        }
    }

    /// Whether the phone lacks the configuration in force; one atomic load each side.
    fn is_behind(&self) -> bool {
        self.state.generation() != self.sent.load(Ordering::SeqCst)
    }

    /// The `config` for the configuration in force when the phone lacks it, marked as sent. Called
    /// with the connection's writer held, by the code that then sends it.
    fn take_pending(&self) -> Result<Option<String>, serde_json::Error> {
        if !self.is_behind() {
            return Ok(None);
        }
        let snapshot = self.state.snapshot();
        let message = config_message(&snapshot)?;
        self.sent.store(snapshot.generation, Ordering::SeqCst);
        Ok(Some(message))
    }
}

/// Sends `message` on the shared writer, preceded by `config` when the phone lacks the
/// configuration in force, in one hold of the writer so nothing can come between them. With no
/// message it only brings the phone's `config` up to date.
///
/// Every report of the musician's mix (`mix_state`, `mix_ack`) goes through here, read before the
/// call and under the mix's writer lock ([crate::mix::MixState::current],
/// [crate::mix::MixState::apply]). That order is what keeps a phone's group levels on the right
/// groups, because the phone keeps its levels by position: an edit of the groups publishes the new
/// configuration before it moves any level (see [crate::engine::apply_groups]), so a mix read with
/// moved levels was read after the publish, the check here finds the new configuration, and the
/// phone gets the new `config` ahead of the levels placed for it.
async fn send_with_config<S>(
    writer: &tokio::sync::Mutex<S>,
    config: &ConfigFeed,
    message: Option<Message>,
) -> Result<(), Box<dyn Error + Send + Sync>>
where
    S: Sink<Message> + Unpin,
    S::Error: Error + Send + Sync + 'static,
{
    let mut sink = writer.lock().await;
    if let Some(update) = config.take_pending()? {
        sink.send(Message::Text(update.into())).await?;
    }
    if let Some(message) = message {
        sink.send(message).await?;
    }
    Ok(())
}

pub(crate) fn spawn_control_thread(
    control_port: u16,
    targets: Arc<TargetRegistry>,
    stopped: Arc<AtomicBool>,
    config_state: Arc<ConfigState>,
    peers: ControlPeers,
    events: EventBus,
) -> JoinHandle<()> {
    thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(("0.0.0.0", control_port)) {
            Ok(listener) => listener,
            Err(error) => {
                crate::log_error!("control", "control listener bind error: {error}");
                return;
            }
        };
        if let Err(error) = listener.set_nonblocking(true) {
            crate::log_error!("control", "control listener setup error: {error}");
            return;
        }
        let runtime = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                crate::log_error!("control", "control runtime error: {error}");
                return;
            }
        };
        runtime.block_on(run_control_server(
            listener,
            targets,
            stopped,
            config_state,
            peers,
            events,
            Heartbeat::default(),
        ));
    })
}

async fn run_control_server(
    listener: std::net::TcpListener,
    targets: Arc<TargetRegistry>,
    stopped: Arc<AtomicBool>,
    config_state: Arc<ConfigState>,
    peers: ControlPeers,
    events: EventBus,
    heartbeat: Heartbeat,
) {
    let listener = match TcpListener::from_std(listener) {
        Ok(listener) => listener,
        Err(error) => {
            crate::log_error!("control", "control listener error: {error}");
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
        // And the clock that keeps a second with no link report as a gap in the musicians' link
        // histories (see [crate::link_history]).
        targets.fill_link_gaps(LinkMoment::now());
        for address in targets.sweep_expired(Instant::now()) {
            crate::log_info!(
                "control",
                "control registration {address} removed after the grace period"
            );
        }
        match accepted {
            None => continue,
            Some(accepted) => match accepted {
                Ok((stream, peer)) => {
                    let targets = Arc::clone(&targets);
                    let config_state = Arc::clone(&config_state);
                    let peers = Arc::clone(&peers);
                    let events = events.clone();
                    tokio::spawn(async move {
                        if let Err(error) = handle_control_connection(
                            stream,
                            peer,
                            targets,
                            config_state,
                            peers,
                            events,
                            heartbeat,
                        )
                        .await
                        {
                            crate::log_warn!("control", "control connection {peer} error: {error}");
                        }
                    });
                }
                Err(error) => crate::log_warn!("control", "control accept error: {error}"),
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
                self.targets.end_link_session(ip);
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

/// Whether a connection with `peer` should be sent its musician's playback buffer because of
/// `event`: a change of the buffer kept for the musician at the same IP. Whether the phone takes
/// one at all is checked when it is sent ([TargetRegistry::playback_buffer_for_phone]).
fn should_push_buffer(event: &EngineEvent, peer: SocketAddr) -> bool {
    matches!(event, EngineEvent::PlaybackBufferChanged { address } if address.ip() == peer.ip())
}

/// Whether a connection with `peer` should be sent the hold its musician's redundancy needs because
/// of `event`: a change of the redundancy kept for the musician at the same IP. Whether the phone
/// holds packets back at all is checked when it is sent ([TargetRegistry::redundancy_for_phone]).
fn should_push_redundancy(event: &EngineEvent, peer: SocketAddr) -> bool {
    matches!(event, EngineEvent::RedundancyChanged { address } if address.ip() == peer.ip())
}

/// What a connection's phone must be sent now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Push {
    /// The musician's whole mix (`mix_state`).
    Mix,
    /// The musician's playback buffer (`playback`).
    Buffer,
    /// The hold the musician's redundancy needs (`redundancy`).
    Redundancy,
    /// All of them, after the feed missed events and cannot tell which of them were for this phone.
    Everything,
}

impl Push {
    fn mix(self) -> bool {
        matches!(self, Push::Mix | Push::Everything)
    }

    fn buffer(self) -> bool {
        matches!(self, Push::Buffer | Push::Everything)
    }

    fn redundancy(self) -> bool {
        matches!(self, Push::Redundancy | Push::Everything)
    }
}

/// One connection's view of the live event bus: it only ever answers what to push now (see
/// [Push]).
///
/// It owns the recovery rules, so the connection loop has a single branch for them:
///
/// - A change of this musician's mix that this connection did not make asks for the mix.
/// - A change of this musician's playback buffer asks for the buffer.
/// - A change of this musician's redundancy asks for the hold it needs.
/// - A feed that lagged behind the bus asks for all of them: an event for this musician may be among the
///   missed ones, and each push carries the whole value, so sending it is always correct.
/// - A closed bus turns the feed off for good. The connection carries on without it, and the feed
///   parks instead of resolving at once on every poll, so the loop cannot spin on it. The
///   connection holds a sender of the bus itself, so today this can only follow a refactor.
struct PushFeed {
    events: broadcast::Receiver<EngineEvent>,
    peer: SocketAddr,
    open: bool,
}

impl PushFeed {
    fn new(events: broadcast::Receiver<EngineEvent>, peer: SocketAddr) -> Self {
        Self {
            events,
            peer,
            open: true,
        }
    }

    /// Resolves with what this connection's phone must be sent now.
    ///
    /// Cancel-safe, as a `select!` branch must be: it only awaits the broadcast's own cancel-safe
    /// `recv`, and it returns as soon as an event asks for a push, so dropping it loses nothing that
    /// mattered.
    async fn next_push(&mut self) -> Push {
        while self.open {
            match self.events.recv().await {
                Ok(event) if should_push_mix(&event, self.peer) => return Push::Mix,
                Ok(event) if should_push_buffer(&event, self.peer) => return Push::Buffer,
                Ok(event) if should_push_redundancy(&event, self.peer) => return Push::Redundancy,
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => return Push::Everything,
                Err(RecvError::Closed) => self.open = false,
            }
        }
        std::future::pending::<Push>().await
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

/// Sends one message on the shared writer as [send_with_config] does (after `config` when the
/// phone lacks the configuration in force), giving up at `deadline`.
///
/// A write to a vanished peer can stall once its buffers fill, and so can waiting for the writer
/// while another write is stuck on it, so both are bounded. The connection loop ends the
/// connection on anything but [BoundedSend::Sent].
async fn send_by<S>(
    writer: &tokio::sync::Mutex<S>,
    config: &ConfigFeed,
    message: Message,
    deadline: tokio::time::Instant,
) -> BoundedSend
where
    S: Sink<Message> + Unpin,
    S::Error: Error + Send + Sync + 'static,
{
    let send = send_with_config(writer, config, Some(message));
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

async fn handle_control_connection(
    stream: tokio::net::TcpStream,
    peer: SocketAddr,
    targets: Arc<TargetRegistry>,
    config_state: Arc<ConfigState>,
    peers: ControlPeers,
    events: EventBus,
    heartbeat: Heartbeat,
) -> Result<(), Box<dyn Error + Send + Sync>> {
    let websocket = accept_async(stream).await?;
    let _peer = register_peer(&peers, &events, &targets, peer);
    // Subscribed before `config` goes out, so a client that has seen `config` cannot miss a mix
    // change made after it.
    let mut pushes = PushFeed::new(events.subscribe_live(), peer);
    let (writer, mut reader) = websocket.split();

    // The sink is shared between the read loop and the config watcher through an async mutex. The
    // read loop only holds it while sending, never while parked in a read.
    let writer = Arc::new(tokio::sync::Mutex::new(writer));
    let config = Arc::new(ConfigFeed::new(Arc::clone(&config_state)));
    send_with_config(&writer, &config, None).await?;

    // The watcher notices a new configuration (a device switch) while the connection is idle and
    // re-sends `config` on the SAME socket, so a client rebuilds its controls without losing the
    // control channel.
    let watcher_writer = Arc::clone(&writer);
    let watcher_config = Arc::clone(&config);
    let watcher_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(CONFIG_POLL_INTERVAL).await;
            if watcher_config.is_behind()
                && send_with_config(&watcher_writer, &watcher_config, None)
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
    // Each ping carries a number, and the pong that brings it back measures the control channel's
    // round trip for the musician's link history. Only the control channel: never the audio.
    let mut ping_clock = PingClock::default();
    loop {
        // Safety net for a runtime that does not drive the watcher's timer while this task is parked
        // on a read: re-check the configuration before parking again, so the next client message
        // also re-sends `config`. It is a cheap atomic load when nothing changed.
        if config.is_behind() {
            send_with_config(&writer, &config, None).await?;
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
            push = pushes.next_push() => {
                let mut outgoing: Vec<String> = Vec::with_capacity(3);
                // Only a phone that takes a buffer is sent one; an older phone never sees the
                // message, and a peer whose target is gone has nothing to follow.
                if push.buffer() {
                    if let Some(buffer) = targets.playback_buffer_for_phone(peer.ip()) {
                        outgoing.push(playback_message(buffer)?);
                    }
                }
                // Only a phone that holds packets back for copies is told the hold; any other takes
                // the copies as they come.
                if push.redundancy() {
                    if let Some(redundancy) = targets.redundancy_for_phone(peer.ip()) {
                        outgoing.push(redundancy_message(redundancy)?);
                    }
                }
                // A peer whose target is gone has no mix to follow; it keeps its channel as before.
                if let Some(mix_state) = push.mix().then(|| targets.mix_state(peer.ip())).flatten() {
                    // Read under the mix's writer lock and before the configuration check in the
                    // send, so levels moved by an edit of the groups follow the new `config` (see
                    // [send_with_config]), with the name of the musician it follows, if any.
                    let (values, following) = mix_state.report();
                    outgoing.push(mix_state_message(values, following.as_deref())?);
                }
                // Bounded like the ping: a push that cannot be written in time means a dead peer.
                let mut gone = false;
                for message in outgoing {
                    match send_by(&writer, &config, Message::Text(message.into()), dead_at).await {
                        BoundedSend::Sent => {}
                        BoundedSend::Failed => {
                            gone = true;
                            break;
                        }
                        BoundedSend::TimedOut => {
                            log_missed_heartbeat(peer, heartbeat.dead_after);
                            gone = true;
                            break;
                        }
                    }
                }
                if gone {
                    break;
                }
                continue;
            }
            _ = ping_timer.tick() => {
                // A ping that cannot be sent in time is as good as no answer.
                let payload = ping_clock.ping(Instant::now());
                match send_by(&writer, &config, Message::Ping(payload.to_vec().into()), dead_at).await {
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
                // A second message some answers are followed by.
                let mut then_also: Option<String> = None;
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
                                &*config_state,
                            );
                            let address = registration.address;
                            // What the phone supports belongs to this registration: an older build
                            // announces nothing, which leaves nothing.
                            targets.set_capabilities(peer.ip(), command.capabilities);
                            // The registration put the engineer's limit, locks and mirror for this
                            // musician in force (or took a previous occupant's away): announced as a
                            // change through the engine, so this phone is sent its mix with them
                            // right away, and the desktop follows. A follower's mix took their
                            // leader's, which those who follow them in turn take too, each
                            // announced the same way.
                            if registration.controls_changed {
                                events.emit(EngineEvent::MixChanged {
                                    address,
                                    origin: MixOrigin::Engine,
                                });
                            }
                            if registration.followed {
                                announce_engine_changes(&events, targets.propagate(peer.ip()));
                            }
                            // A new session starts with no link report: the one left by an earlier
                            // session describes a link that is gone, and kept it would turn stale
                            // while this session is connected and read as a lost link. A rename on
                            // this same session re-registers too, and keeps the current report.
                            // The link history is kept across sessions, its next sample marked as
                            // a reconnect.
                            if !session_registered {
                                targets.clear_link_report(peer.ip());
                                targets.begin_link_session(peer.ip());
                                session_registered = true;
                            }
                            let moved = registration
                                .moved_from
                                .map(|from| format!(", moved from {from}"))
                                .unwrap_or_default();
                            match &name {
                                Some(name) => crate::log_info!(
                                    "control",
                                    "control registration from {peer}: {address} as \"{name}\"{moved}"
                                ),
                                None => crate::log_info!(
                                    "control",
                                    "control registration from {peer}: {address} (unnamed){moved}"
                                ),
                            }
                            // A phone that takes a playback buffer is told the one the desk keeps for
                            // it on every registration, Auto included: the engineer's choice survives
                            // a reconnect, and a phone never keeps one an earlier server set. Any
                            // other phone is answered with nothing, as before. A phone that holds
                            // packets back for copies is told the hold its redundancy needs after
                            // it, the same way, none included.
                            then_also = targets
                                .redundancy_for_phone(peer.ip())
                                .map(redundancy_message)
                                .transpose()?;
                            targets
                                .playback_buffer_for_phone(peer.ip())
                                .map(playback_message)
                                .transpose()?
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
                                targets.record_link_report(
                                    peer.ip(),
                                    &report,
                                    &command,
                                    LinkMoment::now(),
                                );
                                if let Some(recovered) = command.recovered {
                                    targets.count_recovered(peer.ip(), recovered);
                                }
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
                    //
                    // The phone's mix never changes what the engineer locked, nor the channels and
                    // groups of a mix that follows another musician's, and the ack carries the
                    // values kept (see [MixState::apply_from_phone]). Every musician who follows
                    // this one's mix takes it at once, announced as a change through the engine.
                    let seq = if message_type == "mix" {
                        mix_sequence(&text)
                    } else {
                        None
                    };
                    Some(match targets.mix_state(peer.ip()) {
                        None => mix_error(NO_TARGET_ERROR.to_owned(), seq)?,
                        Some(mix_state) => {
                            match parse_mix_update(&text)
                                .and_then(|update| mix_state.apply_from_phone_report(&update))
                            {
                                Ok((values, following)) => {
                                    events.emit(EngineEvent::MixChanged {
                                        address: peer,
                                        origin: MixOrigin::Client { peer },
                                    });
                                    announce_engine_changes(&events, targets.propagate(peer.ip()));
                                    mix_ack(values, following.as_deref(), seq)?
                                }
                                Err(error) => mix_error(error, seq)?,
                            }
                        }
                    })
                };
                // An ack carries the mix `apply` stored, read under its writer lock, so it follows
                // a new `config` like a push does (see [send_with_config]).
                let mut gone = false;
                for response in response.into_iter().chain(then_also) {
                    if send_with_config(&writer, &config, Some(Message::Text(response.into())))
                        .await
                        .is_err()
                    {
                        gone = true;
                        break;
                    }
                }
                if gone {
                    break;
                }
            }
            Message::Close(_) => break,
            Message::Pong(payload) => {
                if let Some(round_trip) = ping_clock.pong(&payload, Instant::now()) {
                    targets.record_round_trip(peer.ip(), round_trip);
                }
            }
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

/// Announces each musician at `addresses` as having a mix changed through the engine, so their
/// phone is sent it and the desktop follows: the followers a change of their leader's mix moved.
fn announce_engine_changes(events: &EventBus, addresses: Vec<SocketAddr>) {
    for address in addresses {
        events.emit(EngineEvent::MixChanged {
            address,
            origin: MixOrigin::Engine,
        });
    }
}

/// The one line a connection closed by the heartbeat leaves in the log.
fn log_missed_heartbeat(peer: SocketAddr, dead_after: Duration) {
    crate::log_info!(
        "control",
        "control connection {peer} closed: nothing received for {dead_after:?}, heartbeat missed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::GroupDefinition;
    use crate::engine::{revalidate_groups, ChannelKind};

    fn definition(id: &str, name: &str, channels: &[usize]) -> GroupDefinition {
        GroupDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    fn drums() -> Vec<GroupDefinition> {
        vec![definition("kit", "Drums", &[1, 2])]
    }

    /// Drums on CH 1-2 and Vocals on CH 3-4: a four-channel device has both, a two-channel one
    /// leaves Vocals invalid.
    fn band() -> Vec<GroupDefinition> {
        vec![
            definition("kit", "Drums", &[1, 2]),
            definition("vox", "Vocals", &[3, 4]),
        ]
    }

    /// The live configuration the engine starts with for `definitions` on `channels` inputs.
    fn live_config(definitions: &[GroupDefinition], channels: u8) -> Arc<ConfigState> {
        let groups = revalidate_groups(definitions, usize::from(channels));
        Arc::new(ConfigState::new(
            channels,
            groups.statuses,
            groups.mixer_layout,
        ))
    }

    /// What a device switch to `channels` inputs publishes, as the engine does.
    fn publish_switch(config: &ConfigState, definitions: &[GroupDefinition], channels: u8) -> bool {
        let groups = revalidate_groups(definitions, usize::from(channels));
        config
            .publish(channels, groups.statuses, groups.mixer_layout)
            .is_some()
    }

    #[test]
    fn config_message_carries_the_source_channels_and_groups() {
        let json =
            config_message(&live_config(&drums(), 4).snapshot()).expect("config should serialize");

        assert!(json.contains(r#""source_channels":4"#));
        assert!(json.contains(r#""groups":[{"id":"kit","name":"Drums","channels":[0,1]}]"#));
    }

    #[test]
    fn an_invalid_group_is_announced_in_its_place_with_no_channels() {
        let json =
            config_message(&live_config(&band(), 2).snapshot()).expect("config should serialize");

        assert!(json.contains(
            r#""groups":[{"id":"kit","name":"Drums","channels":[0,1]},{"id":"vox","name":"Vocals","channels":[]}]"#
        ));
    }

    #[test]
    fn a_published_change_takes_a_higher_generation_and_an_unchanged_one_does_not() {
        let state = live_config(&band(), 4);
        let first = state.generation();

        assert!(publish_switch(&state, &band(), 2));
        let switched = state.snapshot();
        assert!(switched.generation > first);
        assert_eq!(state.generation(), switched.generation);
        assert_eq!(switched.source_channels, 2);
        assert!(switched.groups[1].invalid);

        assert!(!publish_switch(&state, &band(), 2), "nothing changed");
        assert_eq!(state.generation(), switched.generation);
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
        let config_state = live_config(&drums(), 2);

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
        assert!(publish_switch(&config_state, &drums(), 4));

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
                live_config(&drums(), 2),
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
        start_server_on(targets, peers, heartbeat, events, live_config(&drums(), 2))
    }

    /// Starts a control server like [start_server_with_events] on the live configuration the test
    /// supplies, so a test can publish a change the way the engine does and watch the phones.
    fn start_server_on(
        targets: Arc<TargetRegistry>,
        peers: ControlPeers,
        heartbeat: Heartbeat,
        events: EventBus,
        config: Arc<ConfigState>,
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
                config,
                peers,
                events,
                heartbeat,
            ));
        });
        (address, stopped, server)
    }

    /// The groups a `config` message announces, as `(name, channels)` pairs in order.
    fn announced_groups(config: &serde_json::Value) -> Vec<(String, Vec<usize>)> {
        config["groups"]
            .as_array()
            .expect("config carries a group list")
            .iter()
            .map(|group| {
                (
                    group["name"].as_str().expect("a group name").to_owned(),
                    serde_json::from_value(group["channels"].clone()).expect("group channels"),
                )
            })
            .collect()
    }

    /// The latent bug of the start-up layout, over a real socket: after a device switch a phone
    /// that stays connected and sends nothing is sent the live groups (an invalid group in its
    /// place with no channels), not the groups the server started with; and any later change of
    /// the groups is sent too, whether or not the channel count moves.
    #[tokio::test]
    async fn after_a_device_switch_a_connected_phone_is_sent_the_live_groups() {
        let config = live_config(&band(), 4);
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_server_on(
            targets,
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            EventBus::default(),
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;

        assert!(publish_switch(&config, &band(), 2));

        let switched = read_message_of_type(&mut client, "config")
            .await
            .expect("the switch is pushed without the phone asking");
        assert_eq!(switched["source_channels"], 2);
        assert_eq!(
            announced_groups(&switched),
            vec![
                ("Drums".to_owned(), vec![0, 1]),
                ("Vocals".to_owned(), vec![]),
            ]
        );

        // The same channel count with other groups: a new generation is pushed all the same.
        let regrouped = vec![definition("keys", "Keys", &[2])];
        assert!(publish_switch(&config, &regrouped, 2));
        let pushed = read_message_of_type(&mut client, "config")
            .await
            .expect("a change of groups alone is pushed");
        assert_eq!(pushed["source_channels"], 2);
        assert_eq!(
            announced_groups(&pushed),
            vec![("Keys".to_owned(), vec![1])]
        );

        stop_server(address, &stopped, server).await;
    }

    /// The other half of the latent bug: a phone that registers after a device switch mixes with
    /// the live membership, so a group the device lost does not scale a channel it no longer names.
    #[tokio::test]
    async fn a_phone_registering_after_a_device_switch_mixes_with_the_live_groups() {
        let config = live_config(&band(), 4);
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            EventBus::default(),
            Arc::clone(&config),
        );
        // The server is up (it answered with `config`) before the switch.
        let mut client = connect_client(address).await;
        assert!(publish_switch(&config, &band(), 2));

        send_then_sync(&mut client, REGISTER_ANA).await;

        let mix = targets
            .mix_state("127.0.0.1".parse().expect("loopback IP"))
            .expect("registered target")
            .snapshot();
        assert_eq!(mix.channel_group[0], Some(0), "Drums still owns CH 1");
        assert_eq!(
            mix.channel_group[2], None,
            "the invalid Vocals owns nothing"
        );

        stop_server(address, &stopped, server).await;
    }

    /// Reads the next text message as JSON, whatever its type. Bounded by a deadline so a missing
    /// message fails the test instead of hanging it.
    async fn read_next_message(client: &mut ClientStream) -> Option<serde_json::Value> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match tokio::time::timeout(remaining, client.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => return serde_json::from_str(&text).ok(),
                Ok(Some(Ok(_))) => continue,
                _ => return None,
            }
        }
        None
    }

    /// The order a phone needs when the engineer edits the groups while it is connected: the new
    /// `config` first, then the `mix_state` whose group levels are placed for it. A phone keeps its
    /// group levels by position when a `config` arrives, and the server's mix then sets them, so
    /// levels placed for the new groups that arrived before the new `config` would land on the
    /// wrong groups.
    #[tokio::test]
    async fn after_a_group_edit_the_phone_gets_the_new_config_before_its_moved_levels() {
        let config = live_config(&band(), 4);
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &config.snapshot().layout,
            )
            .expect("registry should build"),
        );
        targets
            .mix_state("127.0.0.1".parse().expect("loopback IP"))
            .expect("configured target")
            .apply(&crate::mix::MixUpdate {
                group_levels: Some(vec![30, 70]),
                ..Default::default()
            })
            .expect("a valid update applies");
        let events = EventBus::default();
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;

        // The engineer puts Vocals first.
        let swapped = vec![band()[1].clone(), band()[0].clone()];
        crate::engine::edit_groups(&targets, &config, &events, 4, &swapped)
            .expect("the edit is valid");

        let mut new_config = None;
        let moved = loop {
            let message = read_next_message(&mut client)
                .await
                .expect("the edit reaches the phone");
            match message["type"].as_str() {
                Some("config") => new_config = Some(message),
                Some("mix_state") => break message,
                _ => {}
            }
        };
        let new_config = new_config
            .unwrap_or_else(|| panic!("the moved levels {moved} arrived before the new groups"));
        assert_eq!(
            announced_groups(&new_config),
            vec![
                ("Vocals".to_owned(), vec![2, 3]),
                ("Drums".to_owned(), vec![0, 1]),
            ]
        );
        assert_eq!(new_config["groups"][0]["id"], "vox");
        let levels: Vec<u8> = serde_json::from_value(moved["group_levels"].clone()).unwrap();
        assert_eq!(&levels[..2], &[70, 30], "each level moved with its group");

        stop_server(address, &stopped, server).await;
    }

    /// No details, then Keys L and Keys R linked on CH 2 and CH 3 of a four-channel device.
    fn linked_keys() -> Vec<ChannelMeta> {
        vec![
            ChannelMeta::default(),
            ChannelMeta {
                name: "Keys L".to_owned(),
                kind: ChannelKind::Keys,
                hidden: false,
                linked_to_next: true,
            },
            ChannelMeta {
                name: "Keys R".to_owned(),
                kind: ChannelKind::Keys,
                hidden: false,
                linked_to_next: false,
            },
        ]
    }

    #[test]
    fn config_carries_one_channel_entry_per_input_with_its_details() {
        let config = live_config(&drums(), 4);
        let read = |config: &ConfigState| -> serde_json::Value {
            serde_json::from_str(&config_message(&config.snapshot()).expect("serializes"))
                .expect("config is JSON")
        };
        let unnamed =
            serde_json::json!({ "name": "", "kind": "other", "hidden": false, "link": false });

        assert_eq!(
            read(&config)["channels"],
            serde_json::Value::Array(vec![unnamed.clone(); 4])
        );

        assert!(config.publish_channels(&linked_keys()).is_some());
        let channels = read(&config)["channels"].clone();
        assert_eq!(channels.as_array().map(Vec::len), Some(4));
        assert_eq!(
            channels[1],
            serde_json::json!({ "name": "Keys L", "kind": "keys", "hidden": false, "link": true })
        );
        assert_eq!(
            channels[2],
            serde_json::json!({ "name": "Keys R", "kind": "keys", "hidden": false, "link": false })
        );
        assert_eq!(channels[3], unnamed);
        assert!(
            config.publish_channels(&linked_keys()).is_none(),
            "the same details are no change"
        );
    }

    /// A phone on a smaller device is sent the details of the inputs it has, and after the switch
    /// back it is sent every detail again, the pair included.
    #[test]
    fn config_after_a_switch_back_carries_the_details_of_the_inputs_that_returned() {
        let config = live_config(&drums(), 4);
        config.publish_channels(&linked_keys());
        let channels = |config: &ConfigState| -> serde_json::Value {
            let json: serde_json::Value =
                serde_json::from_str(&config_message(&config.snapshot()).expect("serializes"))
                    .expect("config is JSON");
            json["channels"].clone()
        };

        assert!(publish_switch(&config, &drums(), 2));
        let smaller = channels(&config);
        assert_eq!(smaller.as_array().map(Vec::len), Some(2));
        assert_eq!(smaller[1]["name"], "Keys L");
        assert_eq!(smaller[1]["link"], false, "its second side is missing");

        assert!(publish_switch(&config, &drums(), 4));
        let back = channels(&config);
        assert_eq!(
            back[1],
            serde_json::json!({ "name": "Keys L", "kind": "keys", "hidden": false, "link": true })
        );
        assert_eq!(back[2]["name"], "Keys R");
    }

    /// The order a phone needs when the engineer links a pair while it is connected: the `config`
    /// that says CH 2 and CH 3 are one pair first, then the `mix_state` with the pans the link set,
    /// so the phone never shows a pair's pans before it knows the pair.
    #[tokio::test]
    async fn after_linking_a_pair_the_phone_gets_the_channel_details_before_its_moved_pans() {
        let config = live_config(&[], 4);
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &config.snapshot().layout,
            )
            .expect("registry should build"),
        );
        let events = EventBus::default();
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;

        crate::engine::edit_channels(&targets, &config, &events, &linked_keys())
            .expect("the details are valid");

        let mut new_config = None;
        let moved = loop {
            let message = read_next_message(&mut client)
                .await
                .expect("the link reaches the phone");
            match message["type"].as_str() {
                Some("config") => new_config = Some(message),
                Some("mix_state") => break message,
                _ => {}
            }
        };
        let new_config = new_config
            .unwrap_or_else(|| panic!("the moved pans {moved} arrived before the channel details"));
        assert_eq!(new_config["channels"][1]["link"], true);
        assert_eq!(new_config["channels"][1]["name"], "Keys L");
        let pans: Vec<u8> = serde_json::from_value(moved["pans"].clone()).unwrap();
        assert_eq!(&pans[1..3], &[0, 100], "CH 2 hard left, CH 3 hard right");

        // The phone moves one side of the pair; the server keeps the other with it.
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false,"channels":[100,100,40]}"#;
        client
            .send(Message::Text(mix.into()))
            .await
            .expect("client should send");
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");
        let levels: Vec<u8> = serde_json::from_value(ack["channels"].clone()).unwrap();
        assert_eq!(&levels[..4], &[100, 40, 40, 100]);

        stop_server(address, &stopped, server).await;
    }

    /// A phone's whole mix that moves CH 1 and CH 2, sets its own cap to 90 and, as a phone echoing
    /// a report might, carries the engineer's fields too.
    const WHOLE_MIX_MOVING_CH1: &str = r#"{"type":"mix","seq":5,"volume_percent":80,"max_level_percent":90,"muted":false,"channels":[20,40,100,100],"pans":[50,50,0,100],"mutes":[true,false,false,false],"solos":[false,false,false,false],"group_levels":[],"engineer_limit_percent":100,"locked":[false,false,false,false]}"#;

    /// The ack to [WHOLE_MIX_MOVING_CH1] when CH 1 is locked at its defaults and the limit is 60:
    /// CH 1 as it was, everything else the phone's, and the report shows the limit and the lock.
    fn assert_locked_ch1_held(ack: &serde_json::Value) {
        assert_eq!(ack["seq"], 5, "it answers the phone's mix");
        let levels: Vec<u8> = serde_json::from_value(ack["channels"].clone()).unwrap();
        assert_eq!(
            &levels[..2],
            &[100, 40],
            "CH 1 is unchanged, CH 2 is the phone's"
        );
        assert_eq!(ack["pans"][0], 0, "CH 1's pan is kept");
        assert_eq!(ack["mutes"][0], false, "CH 1's mute is kept");
        assert_eq!(
            ack["max_level_percent"], 90,
            "the phone's own cap is its own"
        );
        assert_eq!(ack["volume_percent"], 80);
        assert_eq!(
            ack["engineer_limit_percent"], 60,
            "the limit is not the phone's"
        );
        assert_eq!(ack["locked"][0], true);
        assert_eq!(ack["locked"][1], false);
    }

    /// The engineer locks CH 1 and limits a connected musician to 60: the phone is told at once,
    /// and its next whole mix, which moves CH 1 and its own cap, is acknowledged with CH 1 as it
    /// was. The mixer's ceiling is the lower of the two caps.
    #[tokio::test]
    async fn a_locked_channel_and_the_engineers_limit_hold_against_the_phones_whole_mix() {
        let config = live_config(&[], 4);
        let targets = Arc::new(
            TargetRegistry::new(
                &["127.0.0.1:50000".parse().expect("target address")],
                &config.snapshot().layout,
            )
            .expect("registry should build"),
        );
        let events = EventBus::default();
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;

        crate::engine::set_target_controls(
            &targets,
            &events,
            "127.0.0.1:50000",
            &crate::mix::EngineerUpdate {
                limit_percent: Some(60),
                locked: Some(vec![true]),
            },
        )
        .expect("the controls are valid");

        let pushed = read_message_of_type(&mut client, "mix_state")
            .await
            .expect("the phone is told");
        assert_eq!(pushed["engineer_limit_percent"], 60);
        assert_eq!(pushed["locked"][0], true);
        assert_eq!(pushed["locked"][1], false);

        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");
        assert_locked_ch1_held(&ack);

        let mix = targets
            .mix_state("127.0.0.1".parse().expect("loopback IP"))
            .expect("configured target")
            .current();
        assert_eq!(
            crate::mix::mix_channels(&[i16::MAX, 0, 0, 0], 4, mix),
            [19_660, 0],
            "CH 1 at full scale is held at 60 % of it, below the phone's 90"
        );

        stop_server(address, &stopped, server).await;
    }

    /// A phone the engineer locked and limited drops off the network for longer than the grace
    /// period and comes back from another IP: its row was removed, and the new one gets the
    /// controls kept for its client id when it registers, before its first mix is handled.
    #[tokio::test]
    async fn a_phone_back_from_a_new_ip_after_the_grace_period_meets_the_engineers_controls() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let events = EventBus::default();
        targets.register(
            "192.168.1.10".parse().expect("old IP"),
            50000,
            Some("Ana".to_owned()),
            Some(RETURNING_PHONE_ID),
            &GroupLayout::default(),
        );
        let (key, _) = crate::engine::set_target_controls(
            &targets,
            &events,
            "192.168.1.10:50000",
            &crate::mix::EngineerUpdate {
                limit_percent: Some(60),
                locked: Some(vec![true]),
            },
        )
        .expect("the controls are valid");
        assert_eq!(
            key,
            crate::targets::MusicianKey::Client(RETURNING_PHONE_ID.to_owned())
        );
        let gone = Instant::now();
        targets.mark_disconnected("192.168.1.10".parse().expect("old IP"), gone);
        assert_eq!(
            targets
                .sweep_expired(gone + crate::targets::REGISTRATION_GRACE_PERIOD * 2)
                .len(),
            1,
            "the row is removed"
        );
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events,
        );

        let mut client = connect_client(address).await;
        send_text(
            &mut client,
            &format!(
                r#"{{"type":"register","name":"Ana","udp_port":50123,"client_id":"{RETURNING_PHONE_ID}"}}"#
            ),
        )
        .await;
        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");

        assert_locked_ch1_held(&ack);

        stop_server(address, &stopped, server).await;
    }

    /// A phone whose controls the desk kept registers: it is sent its mix with the limit and the
    /// locks right after the registration, before it sends anything else, and its whole mix is then
    /// held to them.
    #[tokio::test]
    async fn a_registering_phone_is_told_the_engineers_controls_before_it_sends_a_mix() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let events = EventBus::default();
        crate::engine::keep_target_controls(
            &targets,
            &events,
            &crate::targets::MusicianKey::Client(RETURNING_PHONE_ID.to_owned()),
            crate::mix::EngineerControls {
                limit_percent: 60,
                locked: 0b1,
            },
        );
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events,
        );
        let mut client = connect_client(address).await;

        send_text(
            &mut client,
            &format!(
                r#"{{"type":"register","name":"Ana","udp_port":50123,"client_id":"{RETURNING_PHONE_ID}"}}"#
            ),
        )
        .await;
        let told = read_message_of_type(&mut client, "mix_state")
            .await
            .expect("the phone is told after it registers");

        assert_eq!(told["engineer_limit_percent"], 60);
        assert_eq!(told["locked"][0], true);
        assert_eq!(told["locked"][1], false);
        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");
        assert_locked_ch1_held(&ack);

        stop_server(address, &stopped, server).await;
    }

    /// Reads `mix_state` pushes until one satisfies `wanted`, bounded like [read_message_of_type].
    async fn read_mix_state_where(
        client: &mut ClientStream,
        wanted: impl Fn(&serde_json::Value) -> bool,
    ) -> serde_json::Value {
        loop {
            let pushed = read_message_of_type(client, "mix_state")
                .await
                .expect("a mix_state should arrive");
            if wanted(&pushed) {
                return pushed;
            }
        }
    }

    /// The engineer makes the phone's musician follow a rack's mix ("Bass"): the phone is told
    /// whom it follows with the rack's channels, every desk change of the rack reaches it at once,
    /// and its own whole mix moves only its master. Stopping the mirror frees it again.
    #[tokio::test]
    async fn a_followers_phone_is_told_whom_it_follows_and_cannot_move_its_channels() {
        let config = live_config(&[], 4);
        let targets = Arc::new(
            TargetRegistry::new(
                &[
                    "127.0.0.1:50000".parse().expect("phone address"),
                    "192.168.1.40:50000".parse().expect("rack address"),
                ],
                &config.snapshot().layout,
            )
            .expect("registry should build"),
        );
        let events = EventBus::default();
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;
        let rack_gains = |gains: Vec<i32>| crate::mix::MixUpdate {
            channel_gains: Some(gains),
            ..crate::mix::MixUpdate::default()
        };
        crate::engine::set_target_mix(
            &targets,
            &events,
            "192.168.1.40:50000",
            &rack_gains(vec![30, 40]),
        )
        .expect("a valid update");

        crate::engine::set_target_mirror(
            &targets,
            &events,
            "127.0.0.1:50000",
            Some(crate::engine::MirrorSource {
                address: "192.168.1.40:50000",
                name: "Bass",
            }),
        )
        .expect("the mirror is valid");
        let told = read_mix_state_where(&mut client, |pushed| pushed["following"] == "Bass").await;
        assert_eq!(told["channels"][1], 40, "the rack's mix, at once");

        crate::engine::set_target_mix(
            &targets,
            &events,
            "192.168.1.40:50000",
            &rack_gains(vec![55]),
        )
        .expect("a valid update");
        read_mix_state_where(&mut client, |pushed| pushed["channels"][0] == 55).await;

        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");
        assert_eq!(ack["seq"], 5);
        assert_eq!(ack["following"], "Bass");
        assert_eq!(ack["channels"][0], 55, "the channels follow the rack");
        assert_eq!(ack["mutes"][0], false);
        assert_eq!(ack["volume_percent"], 80, "the master is the phone's");
        assert_eq!(ack["max_level_percent"], 90);

        crate::engine::set_target_mirror(&targets, &events, "127.0.0.1:50000", None)
            .expect("the mirror stops");
        read_mix_state_where(&mut client, |pushed| pushed["following"].is_null()).await;
        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let free = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");
        assert_eq!(free["channels"][0], 20, "the phone is free again");

        stop_server(address, &stopped, server).await;
    }

    /// The phone's musician leads a rack's mix: the phone's own change reaches the rack's mix
    /// before the phone is answered, announced as a change through the engine after the phone's.
    #[tokio::test]
    async fn a_leaders_phone_change_reaches_its_follower_at_once() {
        let config = live_config(&[], 4);
        let targets = Arc::new(
            TargetRegistry::new(
                &[
                    "127.0.0.1:50000".parse().expect("phone address"),
                    "192.168.1.41:50000".parse().expect("rack address"),
                ],
                &config.snapshot().layout,
            )
            .expect("registry should build"),
        );
        let events = EventBus::default();
        crate::engine::set_target_mirror(
            &targets,
            &events,
            "192.168.1.41:50000",
            Some(crate::engine::MirrorSource {
                address: "127.0.0.1:50000",
                name: "Ana",
            }),
        )
        .expect("the mirror is valid");
        let observed = events.subscribe();
        let (address, stopped, server) = start_server_on(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events,
            Arc::clone(&config),
        );
        let mut client = connect_client(address).await;

        send_text(&mut client, WHOLE_MIX_MOVING_CH1).await;
        let ack = read_message_of_type(&mut client, "mix_ack")
            .await
            .expect("the mix is acknowledged");

        assert!(ack["following"].is_null(), "the leader follows nobody");
        let rack = targets
            .mix_state("192.168.1.41".parse().expect("rack IP"))
            .expect("the rack is there")
            .current();
        assert_eq!(&rack.channel_gains[..4], &[20, 40, 100, 100]);
        assert_eq!(&rack.pans[..2], &[50, 50]);
        assert!(rack.channel_muted[0]);
        assert_eq!(rack.volume_percent, 100, "the rack keeps its own master");
        let changes: Vec<EngineEvent> = observed
            .try_iter()
            .filter(|event| matches!(event, EngineEvent::MixChanged { .. }))
            .collect();
        assert!(
            matches!(
                changes.as_slice(),
                [
                    EngineEvent::MixChanged {
                        origin: MixOrigin::Client { .. },
                        ..
                    },
                    EngineEvent::MixChanged {
                        origin: MixOrigin::Engine,
                        address,
                    },
                ] if address.ip() == "192.168.1.41".parse::<IpAddr>().unwrap()
            ),
            "{changes:?}"
        );

        stop_server(address, &stopped, server).await;
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
            Arc::new(crate::engine::LevelMeter::new()),
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
            Arc::new(crate::engine::LevelMeter::new()),
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

    /// A registration's capabilities are kept on the musician's row; one from an older phone (no
    /// list) leaves none, and either way the registration is answered with nothing, as before.
    #[tokio::test]
    async fn a_registration_records_the_capabilities_the_phone_announced() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");
        let capabilities = |targets: &TargetRegistry| {
            targets
                .read()
                .iter()
                .find(|entry| entry.address.ip() == target_ip)
                .map(|entry| entry.origin.capabilities().to_vec())
        };

        let mut client = connect_client(address).await;
        send_then_sync(
            &mut client,
            r#"{"type":"register","name":"Ana","udp_port":50123,"capabilities":["stats.v2","playback.buffer",3]}"#,
        )
        .await;
        assert_eq!(
            capabilities(&targets),
            Some(vec!["stats.v2".to_owned(), "playback.buffer".to_owned()])
        );

        send_then_sync(&mut client, REGISTER_ANA).await;
        assert_eq!(
            capabilities(&targets),
            Some(Vec::new()),
            "a registration that announces none has none"
        );

        stop_server(address, &stopped, server).await;
    }

    /// The heartbeat's pongs measure the control channel's round trip, and each report becomes a
    /// sample of the musician's link history carrying it, with the phone's extras when it sends
    /// them. A client answers pings on its own just by reading, as the phone's library does.
    #[tokio::test]
    async fn the_heartbeat_measures_the_round_trip_and_reports_fill_the_link_history() {
        use crate::targets::MusicianKey;

        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers = ControlPeers::default();
        let (address, stopped, server) =
            start_server(Arc::clone(&targets), Arc::clone(&peers), TEST_HEARTBEAT);
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");
        let round_trip = || {
            targets
                .read()
                .iter()
                .find(|entry| entry.address.ip() == target_ip)
                .and_then(|entry| {
                    entry
                        .link_history
                        .lock()
                        .expect("history lock")
                        .round_trip()
                        .last_ms
                })
        };

        let mut client = connect_client(address).await;
        send_then_sync(&mut client, REGISTER_ANA).await;
        answer_pings_until(&mut client, Duration::from_secs(5), || {
            round_trip().is_some()
        })
        .await;
        let measured = round_trip().expect("a pong measured the round trip");
        assert!(
            (0.0..1_000.0).contains(&measured),
            "a loopback round trip: {measured} ms"
        );

        send_then_sync(
            &mut client,
            r#"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0,"jitter_ms":1.5,"cushion_ms":40}"#,
        )
        .await;
        let samples = targets
            .link_history(
                &MusicianKey::Address("127.0.0.1:50123".parse().unwrap()),
                None,
            )
            .expect("the musician has a history")
            .samples;
        let sample = samples.last().expect("the report is a sample");
        assert!(!sample.gap);
        assert_eq!(sample.packets_received, Some(200));
        assert!(sample.rtt_ms.is_some(), "with the round trip measured last");
        assert_eq!(sample.jitter_ms, Some(1.5));
        assert_eq!(sample.cushion_ms, Some(40.0));
        assert_eq!(sample.backlog_ms, None, "not sent");

        stop_server(address, &stopped, server).await;
    }

    /// The link history samples of the phone registered as [REGISTER_ANA]: whether each is a gap.
    fn ana_history_gaps(targets: &TargetRegistry) -> Vec<bool> {
        targets
            .link_history(
                &crate::targets::MusicianKey::Address("127.0.0.1:50123".parse().unwrap()),
                None,
            )
            .map(|read| read.samples.iter().map(|sample| sample.gap).collect())
            .unwrap_or_default()
    }

    /// A phone that stays connected but stops reporting gets a gap for each silent second from the
    /// accept loop's own tick, with no report to end the silence.
    #[tokio::test]
    async fn a_connected_phone_that_stops_reporting_gets_gaps_from_the_accept_loop() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_registration_server(Arc::clone(&targets));

        let mut client = connect_client(address).await;
        send_then_sync(&mut client, REGISTER_ANA).await;
        send_then_sync(&mut client, CLEAN_STATS).await;
        assert_eq!(ana_history_gaps(&targets), vec![false]);

        // Connected (it answers the pings by reading) and silent.
        answer_pings_until(&mut client, Duration::from_secs(5), || {
            ana_history_gaps(&targets).contains(&true)
        })
        .await;
        let gaps = ana_history_gaps(&targets);
        assert_eq!(gaps.first(), Some(&false), "the report");
        assert!(
            gaps.len() >= 2 && gaps[1..].iter().all(|&gap| gap),
            "then gaps from the tick: {gaps:?}"
        );

        stop_server(address, &stopped, server).await;
    }

    /// A moment `later` than now, as the accept loop's tick would pass it then.
    fn moment_in(later: Duration) -> LinkMoment {
        let now = LinkMoment::now();
        LinkMoment {
            at: now.at + later,
            wall_ms: now.wall_ms + u64::try_from(later.as_millis()).expect("a short delay"),
        }
    }

    /// Once the phone's connection closed (its guard ended the link session), the tick keeps no
    /// more gaps: the time away is not a silence of a connected phone. The tick is driven by hand,
    /// seconds ahead: the same tick fills gaps while the phone is connected and none once it left.
    #[tokio::test]
    async fn a_closed_connection_stops_the_gaps() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers = ControlPeers::default();
        let (address, stopped, server) = start_server(
            Arc::clone(&targets),
            Arc::clone(&peers),
            Heartbeat::default(),
        );
        let target_ip: IpAddr = "127.0.0.1".parse().expect("target IP");

        let mut client = connect_client(address).await;
        send_then_sync(&mut client, REGISTER_ANA).await;
        send_then_sync(&mut client, CLEAN_STATS).await;
        targets.fill_link_gaps(moment_in(Duration::from_secs(5)));
        let connected = ana_history_gaps(&targets);
        assert!(
            connected.len() >= 3 && connected[1..].iter().all(|&gap| gap),
            "a tick seconds after the report keeps gaps while connected: {connected:?}"
        );

        client.close(None).await.expect("the client should close");
        drop(client);
        assert!(
            wait_until(|| control_connections(&peers, target_ip) == 0).await,
            "the connection is gone"
        );
        let closed = ana_history_gaps(&targets);

        targets.fill_link_gaps(moment_in(Duration::from_secs(60)));
        assert_eq!(
            ana_history_gaps(&targets),
            closed,
            "a tick a minute later keeps no gap"
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

    const BUFFER_PHONE_ID: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const REGISTER_WITH_BUFFER: &str = r#"{"type":"register","name":"Ana","udp_port":50123,"client_id":"0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b","capabilities":["stats.v2","playback.buffer"]}"#;

    /// A phone that takes a buffer hears the one the desk keeps for it when it registers (Auto
    /// until the desk sets one), when the desk changes it, and again on its next connection.
    #[tokio::test]
    async fn a_phone_that_takes_a_buffer_is_told_it_on_registering_on_a_change_and_after_a_reconnect(
    ) {
        use crate::playback::{BufferOutcome, PlaybackBuffer};

        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let peers: ControlPeers = Arc::new(Mutex::new(HashMap::new()));
        let events = EventBus::default();
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::clone(&peers),
            Heartbeat::default(),
            events.clone(),
        );

        let mut phone = connect_client(address).await;
        send_text(&mut phone, REGISTER_WITH_BUFFER).await;
        let first = read_message_of_type(&mut phone, "playback")
            .await
            .expect("told on registering");
        assert_eq!(
            first,
            serde_json::json!({"type": "playback", "buffer": {"mode": "auto"}})
        );

        let set = crate::engine::set_target_buffer(
            &targets,
            &peers,
            &events,
            &crate::targets::MusicianKey::Client(BUFFER_PHONE_ID.to_owned()),
            PlaybackBuffer::Fixed { ms: 40 },
        )
        .expect("the phone is here");
        assert_eq!(set.outcome, BufferOutcome::Sent);
        assert_eq!(
            set.kept_for,
            crate::targets::MusicianKey::Client(BUFFER_PHONE_ID.to_owned())
        );
        let changed = read_message_of_type(&mut phone, "playback")
            .await
            .expect("told of the change");
        assert_eq!(
            changed["buffer"],
            serde_json::json!({"mode": "fixed", "ms": 40})
        );

        phone.close(None).await.expect("the phone should close");
        drop(phone);
        let mut back = connect_client(address).await;
        send_text(&mut back, REGISTER_WITH_BUFFER).await;
        let again = read_message_of_type(&mut back, "playback")
            .await
            .expect("told again on its next connection");
        assert_eq!(
            again["buffer"],
            serde_json::json!({"mode": "fixed", "ms": 40})
        );

        stop_server(address, &stopped, server).await;
    }

    const REGISTER_WITH_HOLD: &str = r#"{"type":"register","name":"Ana","udp_port":50123,"client_id":"0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b","capabilities":["playback.buffer","redundancy.hold"]}"#;

    /// A phone that holds packets back for copies hears the hold its redundancy needs when it
    /// registers (none until the desk sets a delay), when the desk changes it, and again on its
    /// next connection.
    #[tokio::test]
    async fn a_phone_that_holds_copies_is_told_the_hold_on_registering_on_a_change_and_after_a_reconnect(
    ) {
        use crate::redundancy::{Redundancy, RedundancyOutcome};

        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let events = EventBus::default();
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
        );

        let mut phone = connect_client(address).await;
        send_text(&mut phone, REGISTER_WITH_HOLD).await;
        let first = read_message_of_type(&mut phone, "redundancy")
            .await
            .expect("told on registering");
        assert_eq!(
            first,
            serde_json::json!({"type": "redundancy", "delay_blocks": 0})
        );

        let set = crate::engine::set_target_redundancy(
            &targets,
            &events,
            &crate::targets::MusicianKey::Client(BUFFER_PHONE_ID.to_owned()),
            Redundancy::On { delay_blocks: 2 },
        )
        .expect("the phone is here");
        assert_eq!(set.outcome, RedundancyOutcome::Set);
        let changed = read_message_of_type(&mut phone, "redundancy")
            .await
            .expect("told of the change");
        assert_eq!(changed["delay_blocks"], 2);

        phone.close(None).await.expect("the phone should close");
        drop(phone);
        let mut back = connect_client(address).await;
        send_text(&mut back, REGISTER_WITH_HOLD).await;
        let again = read_message_of_type(&mut back, "redundancy")
            .await
            .expect("told again on its next connection");
        assert_eq!(again["delay_blocks"], 2);

        stop_server(address, &stopped, server).await;
    }

    /// Reads every text message up to and including the next `mix_ack`, and answers their types.
    async fn types_until_mix_ack(client: &mut ClientStream) -> Vec<String> {
        let mut types = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            match tokio::time::timeout(remaining, client.next()).await {
                Ok(Some(Ok(Message::Text(text)))) => {
                    let value: serde_json::Value =
                        serde_json::from_str(&text).expect("the server sends JSON");
                    let kind = value["type"].as_str().unwrap_or_default().to_owned();
                    types.push(kind.clone());
                    if kind == "mix_ack" {
                        return types;
                    }
                }
                Ok(Some(Ok(_))) => continue,
                _ => break,
            }
        }
        panic!("no mix_ack arrived; seen so far: {types:?}");
    }

    /// An older phone never sees the message: not on registering, and not when a change of the
    /// buffer kept for its IP is announced once its registration is in place. The change is
    /// announced only after a round trip proves the registration was handled, and two more round
    /// trips follow it: the connection pushes between handling messages, so a buffer it wrongly
    /// pushed would arrive before one of the two answers.
    #[tokio::test]
    async fn an_older_phone_is_never_sent_a_buffer() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let events = EventBus::default();
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            events.clone(),
        );
        let mix = r#"{"type":"mix","volume_percent":80,"max_level_percent":90,"muted":false}"#;

        let mut phone = connect_client(address).await;
        send_text(&mut phone, REGISTER_ANA).await;
        send_text(&mut phone, mix).await;
        let mut seen = types_until_mix_ack(&mut phone).await;
        assert!(
            targets.mix_state("127.0.0.1".parse().unwrap()).is_some(),
            "the registration is in place before the change is announced"
        );

        events.emit(EngineEvent::PlaybackBufferChanged {
            address: "127.0.0.1:50123".parse().unwrap(),
        });
        events.emit(EngineEvent::RedundancyChanged {
            address: "127.0.0.1:50123".parse().unwrap(),
        });
        for _ in 0..2 {
            send_text(&mut phone, mix).await;
            seen.extend(types_until_mix_ack(&mut phone).await);
        }
        assert!(
            !seen.iter().any(|kind| kind == "playback"),
            "an older phone is never sent a buffer: {seen:?}"
        );
        assert!(
            !seen.iter().any(|kind| kind == "redundancy"),
            "nor a hold: {seen:?}"
        );

        stop_server(address, &stopped, server).await;
    }

    /// The packets a phone reports a redundant copy recovered add up on its musician's counters;
    /// a report without the field adds nothing.
    #[tokio::test]
    async fn the_recovered_packets_a_phone_reports_add_up() {
        let targets =
            Arc::new(TargetRegistry::new(&[], &GroupLayout::default()).expect("empty registry"));
        let (address, stopped, server) = start_server_with_events(
            Arc::clone(&targets),
            Arc::new(Mutex::new(HashMap::new())),
            Heartbeat::default(),
            EventBus::default(),
        );
        let mut phone = connect_client(address).await;
        send_then_sync(&mut phone, REGISTER_ANA).await;
        for recovered in [r#","recovered":3"#, "", r#","recovered":2"#] {
            send_then_sync(
                &mut phone,
                &format!(
                    r#"{{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0{recovered}}}"#
                ),
            )
            .await;
        }
        assert_eq!(
            targets.read()[0].counters.recovered.load(Ordering::Relaxed),
            5
        );

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
        let mut feed = PushFeed::new(receiver, PHONE.parse().unwrap());
        for other in [
            "192.168.1.4:50000",
            "192.168.1.5:50000",
            "192.168.1.6:50000",
        ] {
            bus.send(engine_change(other))
                .expect("the feed is subscribed");
        }

        // Either push may be among the missed events, so both are asked for.
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push()).await,
            Ok(Push::Everything),
            "none of the events was for this phone, but some were missed"
        );
    }

    /// Events that are not a change of this phone's mix by someone else are passed over, and the
    /// feed keeps waiting until one is.
    #[tokio::test]
    async fn a_feed_waits_through_events_that_are_not_for_its_phone() {
        let phone: SocketAddr = PHONE.parse().unwrap();
        let (bus, receiver) = broadcast::channel(16);
        let mut feed = PushFeed::new(receiver, phone);
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
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push()).await,
            Ok(Push::Mix)
        );
        bus.send(EngineEvent::PlaybackBufferChanged {
            address: "192.168.1.4:50000".parse().unwrap(),
        })
        .expect("the feed is subscribed");
        bus.send(EngineEvent::PlaybackBufferChanged {
            address: "192.168.1.3:50000".parse().unwrap(),
        })
        .expect("the feed is subscribed");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push()).await,
            Ok(Push::Buffer),
            "another musician's buffer is passed over, this one's is pushed"
        );
        bus.send(EngineEvent::RedundancyChanged {
            address: "192.168.1.4:50000".parse().unwrap(),
        })
        .expect("the feed is subscribed");
        bus.send(EngineEvent::RedundancyChanged {
            address: "192.168.1.3:50000".parse().unwrap(),
        })
        .expect("the feed is subscribed");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), feed.next_push()).await,
            Ok(Push::Redundancy),
            "another musician's redundancy is passed over, this one's hold is pushed"
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
        let mut feed = PushFeed::new(receiver, PHONE.parse().unwrap());
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

    /// A feed whose phone already has the configuration in force, so a send carries only its own
    /// message.
    fn settled_feed() -> ConfigFeed {
        let feed = ConfigFeed::new(live_config(&drums(), 2));
        feed.take_pending().expect("config serializes");
        feed
    }

    /// The outcomes the connection loop acts on: a sent push or ping carries on, and a failed or
    /// late one ends the connection (the loop maps both to `break`).
    #[tokio::test]
    async fn a_bounded_send_reports_sent_failed_and_timed_out() {
        let open = tokio::sync::Mutex::new(futures_util::sink::drain());
        let broken = tokio::sync::Mutex::new(BrokenSink);
        let stalled = tokio::sync::Mutex::new(StalledSink);
        let feed = settled_feed();

        assert_eq!(
            send_by(&open, &feed, Message::Text("{}".into()), soon()).await,
            BoundedSend::Sent
        );
        assert_eq!(
            send_by(&broken, &feed, Message::Text("{}".into()), soon()).await,
            BoundedSend::Failed
        );
        assert_eq!(
            send_by(&stalled, &feed, Message::Text("{}".into()), soon()).await,
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
            send_by(
                &writer,
                &settled_feed(),
                Message::Ping(Default::default()),
                soon()
            )
            .await,
            BoundedSend::TimedOut
        );
    }

    /// A sink that keeps everything written to it, in order.
    #[derive(Default)]
    struct RecordingSink(Vec<Message>);

    impl Sink<Message> for RecordingSink {
        type Error = std::io::Error;

        fn poll_ready(
            self: std::pin::Pin<&mut Self>,
            _: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Result<(), Self::Error>> {
            std::task::Poll::Ready(Ok(()))
        }

        fn start_send(
            mut self: std::pin::Pin<&mut Self>,
            message: Message,
        ) -> Result<(), Self::Error> {
            self.0.push(message);
            Ok(())
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

    /// The mechanism behind the order: a message to a phone that lacks the configuration in force
    /// goes out right behind a new `config`, and the next one goes out alone.
    #[tokio::test]
    async fn a_message_to_a_phone_behind_the_configuration_follows_a_new_config() {
        let state = live_config(&band(), 4);
        let feed = ConfigFeed::new(Arc::clone(&state));
        let sent = tokio::sync::Mutex::new(RecordingSink::default());
        send_with_config(&sent, &feed, None)
            .await
            .expect("config is sent");
        assert!(publish_switch(&state, &drums(), 4));

        send_with_config(&sent, &feed, Some(Message::Text("first".into())))
            .await
            .expect("the message is sent");
        send_with_config(&sent, &feed, Some(Message::Text("second".into())))
            .await
            .expect("the message is sent");

        let texts: Vec<String> = sent
            .lock()
            .await
            .0
            .iter()
            .map(|message| message.to_text().expect("text").to_owned())
            .collect();
        assert_eq!(texts.len(), 4, "{texts:?}");
        assert!(
            texts[0].contains(r#""id":"vox""#),
            "the first config: {texts:?}"
        );
        assert!(
            texts[1].contains(r#""type":"config""#) && !texts[1].contains("vox"),
            "the new config comes first: {texts:?}"
        );
        assert_eq!(&texts[2..], ["first", "second"]);
    }
}
