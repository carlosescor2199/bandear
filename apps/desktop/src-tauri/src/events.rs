//! The engine's live state, streamed to the window as Tauri events.
//!
//! One pump thread runs while an engine runs: [crate::commands::start_engine] starts it with the
//! engine, and [crate::commands::stop_engine] or the next start stops it, so there is never more
//! than one. The engine knows nothing of Tauri: the pump listens on the engine's event bus and
//! reads the engine through the command layer. The window receives these events (camelCase JSON,
//! peaks 0-100):
//!
//! - `meters`, about 30 times a second:
//!   `{ "channels": [peak, ...], "musicians": { "<address>": { "left": peak, "right": peak } } }`.
//!   `channels` holds the input peaks, index-aligned with the status's `channels`; `musicians`
//!   holds the peak of the stereo mix each musician is sent.
//! - `status`, about 4 times a second: the `EngineStatusDto` that `engine_status` returns, with the
//!   local names applied.
//! - `mix-changed`, as soon as a mix changes:
//!   `{ "address": "<address>", "origin": "phone" | "desktop", "mix": { ...MixDto } }`, the whole
//!   mix stored after the change. A musician removed in between is not announced.
//!
//! `<address>` is always the musician's address as the status rows show it, even for a change from
//! the phone, whose control connection has another port. The window still asks `engine_status`
//! once for its first paint.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use personal_monitoring::{EngineEvent, MixValues};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, Runtime};

use crate::commands::{self, DesktopState};
use crate::dto::{EngineStatusDto, MetersDto, MixChangedDto};

pub const METERS_EVENT: &str = "meters";
pub const STATUS_EVENT: &str = "status";
pub const MIX_CHANGED_EVENT: &str = "mix-changed";

/// About 30 meter passes a second, which reads as smooth motion.
const METER_INTERVAL: Duration = Duration::from_millis(33);
/// About 4 status passes a second, the rate the window used to poll at: counters, link health and
/// the musician list change slowly.
const STATUS_INTERVAL: Duration = Duration::from_millis(250);

/// The `mix-changed` payload for an engine event, or `None` for any other event.
///
/// `lookup` is the engine's join on IP (`EngineHandle::musician_mix`): it gives the musician's
/// address as the status shows it and the mix stored now, so a phone's change, which names its
/// control peer, lands on the right row. A musician removed in between has nothing to announce.
pub fn mix_changed(
    event: &EngineEvent,
    lookup: impl FnOnce(SocketAddr) -> Option<(SocketAddr, MixValues)>,
) -> Option<MixChangedDto> {
    let EngineEvent::MixChanged { address, origin } = event else {
        return None;
    };
    let (address, mix) = lookup(*address)?;
    Some(MixChangedDto::new(address, *origin, &mix))
}

/// What one pass of the pump has to send.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Due {
    meters: bool,
    status: bool,
}

/// When the pump next sends meters and status.
struct Schedule {
    next_meters: Instant,
    next_status: Instant,
}

impl Schedule {
    /// Both are due at once, so the window has levels and a status as soon as the engine starts.
    fn starting(now: Instant) -> Self {
        Self {
            next_meters: now,
            next_status: now,
        }
    }

    /// What is due at `now`, moving each due pass on to its next time.
    fn take_due(&mut self, now: Instant) -> Due {
        Due {
            meters: take_pass(&mut self.next_meters, METER_INTERVAL, now),
            status: take_pass(&mut self.next_status, STATUS_INTERVAL, now),
        }
    }

    /// How long the pump may wait for an engine event before the next pass is due.
    fn wait(&self, now: Instant) -> Duration {
        self.next_meters
            .min(self.next_status)
            .saturating_duration_since(now)
    }
}

/// Whether the pass planned for `next` is due at `now`, planning the following one when it is.
///
/// A pump that fell behind (a long engine lock, a busy machine) skips the passes it missed instead
/// of sending them in a burst: stale levels are worth nothing.
fn take_pass(next: &mut Instant, interval: Duration, now: Instant) -> bool {
    if now < *next {
        return false;
    }
    let following = *next + interval;
    *next = if following <= now {
        now + interval
    } else {
        following
    };
    true
}

/// What the pump reads from the running engine. `None` means there is nothing to read right now,
/// as when no engine runs.
pub trait PumpSource: Send + 'static {
    fn meters(&self) -> Option<MetersDto>;
    fn status(&self) -> Option<EngineStatusDto>;
    /// The engine's join on IP for a mix change (see [mix_changed]).
    fn musician_mix(&self, address: SocketAddr) -> Option<(SocketAddr, MixValues)>;
}

/// Where the pump sends its events: the window in the app, a recorder in tests.
pub trait PumpSink: Send + 'static {
    fn emit<P: Serialize + Clone>(&self, event: &str, payload: P);
}

/// The thread that streams one engine's events to the window.
///
/// Stopping or dropping the pump joins its thread, so nothing is emitted once that returns. The
/// thread reads the engine under the command layer's engine lock, so a pump must never be stopped
/// while that lock is held; [crate::commands] stops it before taking the lock.
pub struct EventPump {
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EventPump {
    /// Starts streaming from `source` to `sink`.
    ///
    /// `events` is the engine's bus (`EngineHandle::subscribe`); the thread also ends on its own
    /// once the engine is gone and the bus closes.
    pub fn start(
        events: Receiver<EngineEvent>,
        source: impl PumpSource,
        sink: impl PumpSink,
    ) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let thread = {
            let running = Arc::clone(&running);
            thread::spawn(move || pump(&events, &running, &source, &sink))
        };
        Self {
            running,
            thread: Some(thread),
        }
    }

    /// Stops streaming and waits for the thread to end, which takes at most one meter interval
    /// plus the read in progress.
    pub fn stop(mut self) {
        self.halt();
    }

    fn halt(&mut self) {
        self.running.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                eprintln!("[mixlink-desktop] the event pump panicked");
            }
        }
    }
}

impl Drop for EventPump {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The pump thread: meters and status on their schedule, and a mix change as soon as the engine
/// announces it.
fn pump(
    events: &Receiver<EngineEvent>,
    running: &AtomicBool,
    source: &impl PumpSource,
    sink: &impl PumpSink,
) {
    let mut schedule = Schedule::starting(Instant::now());
    while running.load(Ordering::Acquire) {
        let due = schedule.take_due(Instant::now());
        if due.meters {
            if let Some(meters) = source.meters() {
                sink.emit(METERS_EVENT, meters);
            }
        }
        if due.status {
            if let Some(status) = source.status() {
                sink.emit(STATUS_EVENT, status);
            }
        }
        // Waiting on the bus is also the pump's sleep: a change goes out at once, and a stop is
        // noticed within one meter interval.
        match events.recv_timeout(schedule.wait(Instant::now())) {
            Ok(event) => {
                if let Some(change) = mix_changed(&event, |address| source.musician_mix(address)) {
                    sink.emit(MIX_CHANGED_EVENT, change);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            // The engine and its bus are gone: there is nothing left to stream.
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// In the app, the pump reads the engine the window's commands use, through the managed
/// [DesktopState].
impl<R: Runtime> PumpSource for AppHandle<R> {
    fn meters(&self) -> Option<MetersDto> {
        commands::engine_meters(self.try_state::<DesktopState>()?.inner()).ok()
    }

    fn status(&self) -> Option<EngineStatusDto> {
        commands::engine_status(self.try_state::<DesktopState>()?.inner()).ok()
    }

    fn musician_mix(&self, address: SocketAddr) -> Option<(SocketAddr, MixValues)> {
        commands::musician_mix(self.try_state::<DesktopState>()?.inner(), address)
    }
}

impl<R: Runtime> PumpSink for AppHandle<R> {
    fn emit<P: Serialize + Clone>(&self, event: &str, payload: P) {
        if let Err(error) = Emitter::emit(self, event, payload) {
            eprintln!("[mixlink-desktop] could not emit `{event}`: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::net::IpAddr;
    use std::sync::mpsc::channel;
    use std::sync::Mutex;

    use crate::dto::CaptureFormatDto;
    use personal_monitoring::MixOrigin;

    fn address(value: &str) -> SocketAddr {
        value.parse().expect("test address")
    }

    const PHONE: &str = "192.168.1.3:41000";
    const MUSICIAN: &str = "192.168.1.3:50000";

    /// The engine's join over one musician at [MUSICIAN], whose mix is at volume 42.
    fn one_musician(peer: SocketAddr) -> Option<(SocketAddr, MixValues)> {
        (peer.ip() == "192.168.1.3".parse::<IpAddr>().unwrap()).then(|| {
            (
                address(MUSICIAN),
                MixValues {
                    volume_percent: 42,
                    ..MixValues::default()
                },
            )
        })
    }

    #[test]
    fn a_phones_change_is_announced_at_the_musicians_address_as_from_the_phone() {
        let peer = address(PHONE);
        let event = EngineEvent::MixChanged {
            address: peer,
            origin: MixOrigin::Client { peer },
        };

        let change = mix_changed(&event, one_musician).expect("a change of a musician's mix");

        assert_eq!(
            change.address, MUSICIAN,
            "the row's address, not the peer's"
        );
        assert_eq!(change.origin, "phone");
        assert_eq!(change.mix.volume_percent, 42, "the mix stored now");
    }

    #[test]
    fn a_desktop_change_is_announced_as_from_the_desktop() {
        let event = EngineEvent::MixChanged {
            address: address(MUSICIAN),
            origin: MixOrigin::Engine,
        };

        let change = mix_changed(&event, one_musician).expect("a change of a musician's mix");

        assert_eq!(change.address, MUSICIAN);
        assert_eq!(change.origin, "desktop");
    }

    #[test]
    fn a_change_for_a_musician_who_is_gone_is_not_announced() {
        let event = EngineEvent::MixChanged {
            address: address("192.168.1.9:50000"),
            origin: MixOrigin::Engine,
        };

        assert!(mix_changed(&event, one_musician).is_none());
    }

    #[test]
    fn a_connection_coming_or_going_is_not_a_mix_change() {
        let never = |_: SocketAddr| -> Option<(SocketAddr, MixValues)> {
            panic!("no mix is looked up for a connection event")
        };

        for event in [
            EngineEvent::ClientConnected {
                address: address(PHONE),
            },
            EngineEvent::ClientDisconnected {
                address: address(PHONE),
            },
        ] {
            assert!(mix_changed(&event, never).is_none());
        }
    }

    #[test]
    fn the_first_pass_sends_both_and_each_then_keeps_its_own_rate() {
        let start = Instant::now();
        let mut schedule = Schedule::starting(start);
        let (mut meters, mut status) = (0, 0);

        // One simulated second, checked every millisecond.
        for millisecond in 0..1_000 {
            let due = schedule.take_due(start + Duration::from_millis(millisecond));
            meters += usize::from(due.meters);
            status += usize::from(due.status);
        }

        assert!(
            (30..=31).contains(&meters),
            "about 30 meter passes a second, got {meters}"
        );
        assert_eq!(status, 4, "about 4 status passes a second");
    }

    #[test]
    fn a_stalled_pump_does_not_burst_to_catch_up() {
        let start = Instant::now();
        let mut schedule = Schedule::starting(start);
        schedule.take_due(start);
        let late = start + Duration::from_secs(2);

        assert_eq!(
            schedule.take_due(late),
            Due {
                meters: true,
                status: true
            }
        );
        assert_eq!(
            schedule.take_due(late),
            Due::default(),
            "the missed passes are skipped, not replayed"
        );
        assert!(schedule.take_due(late + METER_INTERVAL).meters);
    }

    #[test]
    fn the_pump_sleeps_until_the_nearest_pass() {
        let start = Instant::now();
        let mut schedule = Schedule::starting(start);
        assert_eq!(
            schedule.wait(start),
            Duration::ZERO,
            "the first pass is due at once"
        );
        schedule.take_due(start);

        assert_eq!(schedule.wait(start), METER_INTERVAL);
        assert_eq!(
            schedule.wait(start + Duration::from_millis(20)),
            METER_INTERVAL - Duration::from_millis(20)
        );
        assert_eq!(
            schedule.wait(start + Duration::from_secs(1)),
            Duration::ZERO
        );
    }

    /// Records every emitted event as JSON, as the window would receive it.
    #[derive(Clone, Default)]
    struct Recorder(Arc<Mutex<Vec<(String, serde_json::Value)>>>);

    impl PumpSink for Recorder {
        fn emit<P: Serialize + Clone>(&self, event: &str, payload: P) {
            let payload = serde_json::to_value(payload).expect("a payload serializes");
            self.0
                .lock()
                .expect("recorder lock")
                .push((event.to_owned(), payload));
        }
    }

    impl Recorder {
        fn count(&self, event: &str) -> usize {
            self.0
                .lock()
                .expect("recorder lock")
                .iter()
                .filter(|(name, _)| name == event)
                .count()
        }

        fn len(&self) -> usize {
            self.0.lock().expect("recorder lock").len()
        }

        fn last(&self, event: &str) -> Option<serde_json::Value> {
            self.0
                .lock()
                .expect("recorder lock")
                .iter()
                .rev()
                .find(|(name, _)| name == event)
                .map(|(_, payload)| payload.clone())
        }
    }

    fn idle_status() -> EngineStatusDto {
        EngineStatusDto {
            device_name: "Test device".to_owned(),
            capture_format: CaptureFormatDto {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "I16".to_owned(),
                buffer_size: "Default".to_owned(),
            },
            control_port: 50001,
            sample_rate: 48_000,
            source_channels: 2,
            groups: Vec::new(),
            channels: Vec::new(),
            stopped: false,
            samples_received: 0,
            packets_sent: 0,
            packets_discarded: 0,
            musicians: Vec::new(),
        }
    }

    /// An engine with one musician. It holds `alive` so a test can tell when the pump's thread has
    /// ended and dropped it.
    struct FakeEngine {
        _alive: Arc<()>,
    }

    impl PumpSource for FakeEngine {
        fn meters(&self) -> Option<MetersDto> {
            Some(MetersDto {
                channels: vec![50, 0],
                musicians: BTreeMap::new(),
            })
        }

        fn status(&self) -> Option<EngineStatusDto> {
            Some(idle_status())
        }

        fn musician_mix(&self, address: SocketAddr) -> Option<(SocketAddr, MixValues)> {
            one_musician(address)
        }
    }

    /// Polls `done` for up to three seconds.
    fn wait_until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    #[test]
    fn a_running_pump_streams_meters_and_status_and_announces_mix_changes() {
        let (bus, events) = channel();
        let recorder = Recorder::default();
        let pump = EventPump::start(
            events,
            FakeEngine {
                _alive: Arc::new(()),
            },
            recorder.clone(),
        );

        assert!(wait_until(
            || recorder.count(METERS_EVENT) >= 3 && recorder.count(STATUS_EVENT) >= 1
        ));
        assert_eq!(
            recorder.last(METERS_EVENT),
            Some(serde_json::json!({ "channels": [50, 0], "musicians": {} }))
        );
        assert_eq!(
            recorder
                .last(STATUS_EVENT)
                .map(|status| status["deviceName"].clone()),
            Some(serde_json::json!("Test device"))
        );

        let peer = address(PHONE);
        bus.send(EngineEvent::MixChanged {
            address: peer,
            origin: MixOrigin::Client { peer },
        })
        .expect("the pump listens");
        assert!(wait_until(|| recorder.count(MIX_CHANGED_EVENT) == 1));
        let change = recorder.last(MIX_CHANGED_EVENT).expect("announced");
        assert_eq!(change["address"], MUSICIAN);
        assert_eq!(change["origin"], "phone");
        assert_eq!(change["mix"]["volumePercent"], 42);

        pump.stop();
    }

    #[test]
    fn a_stopped_pump_has_ended_its_thread_and_emits_nothing_more() {
        let (_bus, events) = channel();
        let recorder = Recorder::default();
        let alive = Arc::new(());
        let pump = EventPump::start(
            events,
            FakeEngine {
                _alive: Arc::clone(&alive),
            },
            recorder.clone(),
        );
        assert!(wait_until(|| recorder.count(METERS_EVENT) >= 1));

        pump.stop();

        assert_eq!(
            Arc::strong_count(&alive),
            1,
            "the thread ended and dropped the engine it read"
        );
        let emitted = recorder.len();
        thread::sleep(Duration::from_millis(100));
        assert_eq!(recorder.len(), emitted, "nothing is emitted after stop");
    }

    #[test]
    fn a_pump_ends_on_its_own_when_the_engine_goes_away() {
        let (bus, events) = channel();
        let alive = Arc::new(());
        let pump = EventPump::start(
            events,
            FakeEngine {
                _alive: Arc::clone(&alive),
            },
            Recorder::default(),
        );

        drop(bus);

        assert!(wait_until(|| Arc::strong_count(&alive) == 1));
        pump.stop();
    }

    #[test]
    fn dropping_a_pump_stops_it() {
        let (_bus, events) = channel();
        let alive = Arc::new(());
        let pump = EventPump::start(
            events,
            FakeEngine {
                _alive: Arc::clone(&alive),
            },
            Recorder::default(),
        );

        drop(pump);

        assert_eq!(Arc::strong_count(&alive), 1);
    }
}
