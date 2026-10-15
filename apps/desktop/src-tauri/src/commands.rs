//! The engine calls the Tauri commands delegate to.
//!
//! They live apart from the `#[tauri::command]` wrappers in the crate root so that an example
//! binary can drive exactly the same code path without a window. The window is only a rendering of
//! what these functions return.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;

use personal_monitoring::{
    start, AudioHost, EngineConfig, EngineEvent, EngineHandle, GroupDefinition, MixValues,
};

use crate::dto::{
    CaptureFormatDto, EngineStateDto, EngineStatusDto, GroupRequest, InputDeviceDto, MetersDto,
    MixDto, MixUpdateRequest, StartRequest, StartSummary,
};
use crate::events::EventPump;
use crate::names::MusicianNames;

/// Starts the event pump for a new engine, given that engine's event bus and run.
pub type PumpStarter = Box<dyn Fn(Receiver<EngineEvent>, u64) -> EventPump + Send + Sync>;

/// The state the desktop commands share: the running engine, its event pump, and the desktop-only
/// name store.
///
/// The engine handle is behind a [Mutex] because the engine is already internally concurrent: the
/// commands only need exclusive access to start, stop and take a snapshot. The name store has its
/// own lock, and the two are never held in the opposite order, so they cannot deadlock.
///
/// The pump's lock is taken first by start and stop and held throughout, so two quick starts can
/// never leave two pumps running. The pump thread itself takes the engine lock to read, so a pump
/// is always stopped before the engine lock is taken, never while it is held.
pub struct DesktopState {
    engine: Mutex<Option<EngineHandle>>,
    /// The engine run: 0 before the first start, then one more for each start, taken under the
    /// engine lock as the previous engine ends. The engine in `engine`, its pump and every event
    /// and snapshot it produces belong to this run.
    run: AtomicU64,
    pump: Mutex<Option<EventPump>>,
    start_pump: Option<PumpStarter>,
    names: MusicianNames,
}

impl DesktopState {
    /// A state with no event pump: the engine runs and the commands answer, but nothing is pushed.
    pub fn new(names: MusicianNames) -> Self {
        Self {
            engine: Mutex::new(None),
            run: AtomicU64::new(0),
            pump: Mutex::new(None),
            start_pump: None,
            names,
        }
    }

    /// Streams every engine started from now on through a pump that `start_pump` builds, given the
    /// engine's event bus and run.
    pub fn with_event_pump(
        mut self,
        start_pump: impl Fn(Receiver<EngineEvent>, u64) -> EventPump + Send + Sync + 'static,
    ) -> Self {
        self.start_pump = Some(Box::new(start_pump));
        self
    }

    /// The current engine run: the run of the engine now running, or of the last start attempt.
    pub fn run(&self) -> u64 {
        self.run.load(Ordering::SeqCst)
    }
}

/// Stops the running pump, if any, leaving the slot empty. Called with the pump lock held and the
/// engine lock free, because the pump's thread may be waiting on the engine lock.
fn stop_pump(pump: &mut Option<EventPump>) {
    if let Some(running) = pump.take() {
        running.stop();
    }
}

const DEFAULT_CONTROL_PORT: u16 = 50001;

/// Translates the webview's request into the engine's own configuration.
///
/// An empty target list stays empty: the musician list is editable at runtime now, so the window
/// starts with nobody configured and the engineer adds musicians by address. The old implicit
/// `127.0.0.1:50000` target played into the void on the engineer's own machine and made the first
/// step of a real session "remove the placeholder".
fn to_engine_config(request: StartRequest) -> EngineConfig {
    EngineConfig {
        device_filter: request.device_filter,
        targets: request.targets,
        control_port: request.control_port.unwrap_or(DEFAULT_CONTROL_PORT),
        groups: request
            .groups
            .into_iter()
            .map(|group: GroupRequest| GroupDefinition {
                name: group.name,
                channels: group.channels,
            })
            .collect(),
        buffer_frames: request.buffer_frames,
        host: parse_host(request.host.as_deref()),
    }
}

/// Maps a window value to a backend. An unknown or absent value falls back to `auto`, which is what
/// the window sends until the engineer picks something.
fn parse_host(value: Option<&str>) -> AudioHost {
    value
        .and_then(|value| AudioHost::parse(value).ok())
        .unwrap_or(AudioHost::Auto)
}

/// Starts the engine and returns the device and capture format the window shows.
///
/// Starting while one is already running replaces it rather than stacking a second capture stream,
/// and its event pump with it: the previous pump is stopped before anything else, and the new one
/// starts with the new engine.
///
/// Every start, even one that fails, begins a new engine run: the previous engine has ended either
/// way, so the window can tell its late events from the new engine's.
pub fn start_engine(state: &DesktopState, request: StartRequest) -> Result<StartSummary, String> {
    let config = to_engine_config(request);
    let mut pump = state
        .pump
        .lock()
        .map_err(|_| "event pump lock was poisoned".to_owned())?;
    stop_pump(&mut pump);
    let mut guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    // Under the engine lock, so a snapshot can never pair the old engine with the new run.
    let run = state.run.fetch_add(1, Ordering::SeqCst) + 1;

    if let Some(mut running) = guard.take() {
        running
            .stop()
            .map_err(|error| format!("could not stop the previous engine: {error}"))?;
    }

    match start(config).map_err(|error| error.to_string())? {
        Some(handle) => {
            let summary = StartSummary {
                run,
                device_name: handle.device_name().to_owned(),
                capture_format: handle.capture_format().into(),
            };
            println!(
                "[mixlink-desktop] start_engine: device=\"{}\" capture_format={}",
                summary.device_name,
                summary.capture_format.describe()
            );
            // Subscribed before the engine is shared, so the pump misses no event.
            let events = state.start_pump.as_ref().map(|_| handle.subscribe());
            *guard = Some(handle);
            drop(guard);
            if let (Some(start_pump), Some(events)) = (&state.start_pump, events) {
                *pump = Some(start_pump(events, run));
            }
            Ok(summary)
        }
        None => Err(
            "No input device is available. Connect an audio interface and try again.".to_owned(),
        ),
    }
}

/// Whether an engine runs and, if so, its status: what the window's `engine_status` command
/// answers. No engine running is an answer, not an error; only a poisoned lock fails.
pub fn engine_state(state: &DesktopState) -> Result<EngineStateDto, String> {
    // One lock for the check and the read, so the answer is about one engine.
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    Ok(match guard.as_ref() {
        Some(handle) => EngineStateDto::Running(status_of(state, handle)),
        None => EngineStateDto::NotRunning { run: state.run() },
    })
}

/// Returns the engine's current status, naming its run, with each musician row's displayed name
/// resolved by [apply_names]. Fails while no engine runs.
pub fn engine_status(state: &DesktopState) -> Result<EngineStatusDto, String> {
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    Ok(status_of(state, handle))
}

/// The status of `handle`, the engine in `state`, read while the caller holds the engine lock.
fn status_of(state: &DesktopState, handle: &EngineHandle) -> EngineStatusDto {
    let mut status = EngineStatusDto::from(&handle.status());
    status.run = state.run();
    apply_names(&mut status, &state.names);
    status
}

/// Returns the live input and output levels, for the `meters` event.
///
/// The engine's cheap read: it does not build the status, so it can run many times a second.
pub fn engine_meters(state: &DesktopState) -> Result<MetersDto, String> {
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    Ok(MetersDto {
        run: state.run(),
        ..MetersDto::from(&handle.meters())
    })
}

/// The musician at `address`'s IP, at the address the status rows show, and their current mix;
/// `None` when no engine runs or that IP is no musician. The `mix-changed` event's lookup.
pub fn musician_mix(state: &DesktopState, address: SocketAddr) -> Option<(SocketAddr, MixValues)> {
    state.engine.lock().ok()?.as_ref()?.musician_mix(address)
}

/// Fills each musician row's displayed name, joining the local label store on the address string.
///
/// The client's announced name wins: each musician sets their own name in their own app, and a
/// stored label must not shadow it. The engineer's local label is the fallback for a client that
/// never announced a name, such as a fixed rack added by address; `name_is_local` marks that case
/// so the window can tell the two sources apart.
///
/// Kept separate from [EngineStatusDto] so the DTO stays a pure translation of engine state and
/// the desktop-only concern is applied in one place. The client's own name stays in `clientName`,
/// so the window can still show what the phone calls itself. Public so the headless probe can
/// drive exactly the name resolution the window gets, without a webview.
pub fn apply_names(status: &mut EngineStatusDto, names: &MusicianNames) {
    for musician in &mut status.musicians {
        let local = names.get(&musician.address);
        musician.name_is_local = musician.client_name.is_none() && local.is_some();
        musician.name = musician.client_name.clone().or(local);
    }
}

/// Adds a musician by address while the engine runs, returning the resolved address.
///
/// A duplicate IP or an unresolvable address is rejected by the engine with an error naming the
/// value, and nothing changes. On success the next audio packet is sent to the new address; the
/// other musicians, the control server and the discovery beacon are untouched.
pub fn add_target(state: &DesktopState, address: String) -> Result<String, String> {
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let added = handle
        .add_target(&address)
        .map_err(|error| error.to_string())?;
    println!("[mixlink-desktop] add_target: {added}");
    Ok(added.to_string())
}

/// Removes a musician by address while the engine runs, returning the removed address.
///
/// The engine's documented removal semantics apply: their audio stops on the next packet, their
/// control channel stays open and their next `mix` is answered with the existing
/// `no UDP target configured for client IP` error, and no other musician is disturbed.
pub fn remove_target(state: &DesktopState, address: String) -> Result<String, String> {
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let removed = handle
        .remove_target(&address)
        .map_err(|error| error.to_string())?;
    println!("[mixlink-desktop] remove_target: {removed}");
    Ok(removed.to_string())
}

/// Changes a musician's mix from the desk, returning the whole mix now applied.
///
/// `address` is the row's address as the status reports it, and `mix` is partial: absent fields
/// keep their values. The engine applies it to the audio at once and pushes the result to the
/// musician's phone, so the phone's screen follows. An unknown musician, an address that does not
/// parse or a list longer than the mix holds is rejected with an error naming it, and nothing
/// changes. Not logged: a fader sends many of these a second.
pub fn set_mix(
    state: &DesktopState,
    address: String,
    mix: MixUpdateRequest,
) -> Result<MixDto, String> {
    let guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let applied = handle
        .set_mix(&address, &mix.into())
        .map_err(|error| error.to_string())?;
    Ok(MixDto::from(&applied))
}

/// Sets or clears a musician's label. The label never reaches the engine or any client, and it is
/// only shown for a client that has not announced a name of its own: a client's own name wins.
pub fn set_musician_name(
    state: &DesktopState,
    address: String,
    name: String,
) -> Result<(), String> {
    state.names.set(&address, &name)?;
    println!("[mixlink-desktop] set_musician_name: {address} -> \"{name}\"");
    Ok(())
}

/// Stops the engine's event pump, then the engine, and joins their threads. Stopping when idle is
/// a no-op.
pub fn stop_engine(state: &DesktopState) -> Result<(), String> {
    let mut pump = state
        .pump
        .lock()
        .map_err(|_| "event pump lock was poisoned".to_owned())?;
    stop_pump(&mut pump);
    let mut guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    match guard.take() {
        Some(mut handle) => handle.stop().map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

/// Lists the input devices one backend offers, each with the channel count a switch would capture.
/// Independent of the engine, so the window can populate the selector before starting.
pub fn list_input_devices(host: Option<String>) -> Result<Vec<InputDeviceDto>, String> {
    personal_monitoring::list_input_devices_for(parse_host(host.as_deref()))
        .map(|devices| devices.iter().map(InputDeviceDto::from).collect())
        .map_err(|error| error.to_string())
}

/// Switches a running engine to another input device, keeping the network, control and discovery
/// threads alive. A failed switch returns the error and leaves the previous device running.
pub fn switch_device(state: &DesktopState, device: String) -> Result<StartSummary, String> {
    let mut guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let handle = guard
        .as_mut()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    handle
        .switch_device(Some(&device))
        .map_err(|error| error.to_string())?;
    let format = CaptureFormatDto::from(handle.capture_format());
    println!(
        "[mixlink-desktop] switch_device: device=\"{}\" capture_format={}",
        handle.device_name(),
        format.describe()
    );
    Ok(StartSummary {
        run: state.run(),
        device_name: handle.device_name().to_owned(),
        capture_format: format,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::MusicianDto;
    use crate::events::{EventPump, PumpSink, PumpSource};

    fn status_with_musicians(addresses: &[&str]) -> EngineStatusDto {
        EngineStatusDto {
            run: 1,
            device_name: "Test device".to_owned(),
            capture_format: CaptureFormatDto {
                channels: 2,
                sample_rate: 48_000,
                sample_format: "I16".to_owned(),
                buffer_size: "Default".to_owned(),
            },
            control_port: DEFAULT_CONTROL_PORT,
            sample_rate: 48_000,
            source_channels: 2,
            groups: Vec::new(),
            channels: Vec::new(),
            stopped: false,
            samples_received: 0,
            packets_sent: 0,
            packets_discarded: 0,
            musicians: addresses
                .iter()
                .map(|address| MusicianDto {
                    address: (*address).to_owned(),
                    client_name: None,
                    name: None,
                    name_is_local: false,
                    control_connected: false,
                    mix: MixDto::from(&personal_monitoring::MixValues::default()),
                    packets_sent: 0,
                    packets_discarded: 0,
                    link_health: "unknown",
                    packets_per_second: None,
                    underruns_per_second: None,
                    loss_percent: None,
                    receive_timeouts_per_second: None,
                })
                .collect(),
        }
    }

    fn names_path(test: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-command-tests")
            .join(test);
        let _ = std::fs::remove_dir_all(&directory);
        directory.join("musicians.json")
    }

    #[test]
    fn status_rows_take_their_labels_from_the_name_store() {
        let names = MusicianNames::load(names_path("labels"));
        names
            .set("192.168.1.30:50000", "Ana")
            .expect("setting a name should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000", "192.168.1.31:50000"]);

        apply_names(&mut status, &names);

        assert_eq!(status.musicians[0].name.as_deref(), Some("Ana"));
        assert!(status.musicians[0].name_is_local);
        assert_eq!(status.musicians[1].name, None);
        assert!(!status.musicians[1].name_is_local);
    }

    #[test]
    fn the_clients_own_name_beats_the_local_label() {
        let names = MusicianNames::load(names_path("client-wins"));
        names
            .set("192.168.1.30:50000", "Lead vocal")
            .expect("setting a name should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);
        status.musicians[0].client_name = Some("Ana".to_owned());

        apply_names(&mut status, &names);

        // The phone is the authority on its own name: the desk's stored label must not shadow it,
        // and the row must not report the label as if it were the name on screen.
        assert_eq!(status.musicians[0].name.as_deref(), Some("Ana"));
        assert_eq!(status.musicians[0].client_name.as_deref(), Some("Ana"));
        assert!(!status.musicians[0].name_is_local);
    }

    #[test]
    fn a_local_label_applies_when_the_client_announced_nothing() {
        let names = MusicianNames::load(names_path("silent-client"));
        names
            .set("192.168.1.30:50000", "Rack A")
            .expect("setting a name should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);

        apply_names(&mut status, &names);

        assert_eq!(status.musicians[0].name.as_deref(), Some("Rack A"));
        assert_eq!(status.musicians[0].client_name, None);
        assert!(status.musicians[0].name_is_local);
    }

    #[test]
    fn a_row_with_no_client_name_and_no_label_stays_unnamed() {
        let names = MusicianNames::load(names_path("unnamed"));
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);

        apply_names(&mut status, &names);

        assert_eq!(status.musicians[0].name, None);
        assert!(!status.musicians[0].name_is_local);
    }

    #[test]
    fn setting_a_mix_without_a_running_engine_says_so() {
        let state = DesktopState::new(MusicianNames::load(names_path("set-mix-idle")));

        let error = set_mix(
            &state,
            "192.168.1.30:50000".to_owned(),
            MixUpdateRequest::default(),
        )
        .expect_err("there is no engine to set a mix on");

        assert_eq!(error, "the engine is not running");
    }

    /// An engine that reports nothing. It holds `alive` so a test can tell when the pump's thread
    /// has ended and dropped it.
    struct Silent {
        _alive: std::sync::Arc<()>,
    }

    impl PumpSource for Silent {
        fn meters(&self) -> Option<crate::dto::MetersDto> {
            None
        }

        fn status(&self) -> Option<EngineStatusDto> {
            None
        }

        fn musician_mix(
            &self,
            _: std::net::SocketAddr,
        ) -> Option<(std::net::SocketAddr, personal_monitoring::MixValues)> {
            None
        }
    }

    struct Nowhere;

    impl PumpSink for Nowhere {
        fn emit<P: serde::Serialize + Clone>(&self, _: &str, _: P) {}
    }

    /// Puts a running pump in the state, as a start with a window does, and returns its liveness
    /// marker and the sender that keeps its event feed open.
    fn install_pump(
        state: &DesktopState,
    ) -> (
        std::sync::Arc<()>,
        std::sync::mpsc::Sender<personal_monitoring::EngineEvent>,
    ) {
        let alive = std::sync::Arc::new(());
        let (bus, events) = std::sync::mpsc::channel();
        let pump = EventPump::start(
            events,
            1,
            Silent {
                _alive: std::sync::Arc::clone(&alive),
            },
            Nowhere,
        );
        *state.pump.lock().expect("pump lock") = Some(pump);
        (alive, bus)
    }

    #[test]
    fn stopping_the_engine_stops_its_event_pump() {
        let state = DesktopState::new(MusicianNames::load(names_path("stop-pump")));
        let (alive, _bus) = install_pump(&state);

        stop_engine(&state).expect("stopping is fine without an engine");

        assert!(state.pump.lock().expect("pump lock").is_none());
        assert_eq!(
            std::sync::Arc::strong_count(&alive),
            1,
            "the pump's thread has ended"
        );
    }

    #[test]
    fn a_restart_stops_the_previous_event_pump_even_when_the_new_engine_fails() {
        let state = DesktopState::new(MusicianNames::load(names_path("restart-pump")));
        let (alive, _bus) = install_pump(&state);

        // An invalid target fails the start before any audio device is touched.
        let error = start_engine(
            &state,
            StartRequest {
                targets: vec!["not a target".to_owned()],
                ..StartRequest::default()
            },
        )
        .expect_err("the new engine cannot start");

        assert!(error.contains("not a target"), "names the value: {error}");
        assert!(state.pump.lock().expect("pump lock").is_none());
        assert_eq!(
            std::sync::Arc::strong_count(&alive),
            1,
            "the previous pump's thread has ended"
        );
    }

    #[test]
    fn with_no_engine_the_state_says_so_instead_of_failing() {
        let state = DesktopState::new(MusicianNames::load(names_path("state-idle")));

        let idle = engine_state(&state).expect("the state is always readable");
        let json = serde_json::to_value(&idle).expect("the state serializes");

        assert_eq!(json, serde_json::json!({ "state": "notRunning", "run": 0 }));
    }

    #[test]
    fn after_a_failed_start_the_state_names_the_run_that_ended() {
        let state = DesktopState::new(MusicianNames::load(names_path("state-failed")));
        start_engine(
            &state,
            StartRequest {
                targets: vec!["not a target".to_owned()],
                ..StartRequest::default()
            },
        )
        .expect_err("the engine cannot start");

        let json = serde_json::to_value(engine_state(&state).expect("readable"))
            .expect("the state serializes");

        assert_eq!(json, serde_json::json!({ "state": "notRunning", "run": 1 }));
    }

    #[test]
    fn every_start_takes_the_next_engine_run_even_when_it_fails() {
        let state = DesktopState::new(MusicianNames::load(names_path("runs")));
        assert_eq!(state.run(), 0, "no engine has run yet");
        let failing = || StartRequest {
            targets: vec!["not a target".to_owned()],
            ..StartRequest::default()
        };

        start_engine(&state, failing()).expect_err("the engine cannot start");
        assert_eq!(state.run(), 1);
        start_engine(&state, failing()).expect_err("the engine cannot start");
        assert_eq!(
            state.run(),
            2,
            "a failed start still ends the run before it, so it takes a run of its own"
        );
    }

    #[test]
    fn reading_meters_or_a_mix_without_a_running_engine_finds_nothing() {
        let state = DesktopState::new(MusicianNames::load(names_path("meters-idle")));

        assert_eq!(
            engine_meters(&state).expect_err("there is no engine to read"),
            "the engine is not running"
        );
        assert_eq!(
            musician_mix(&state, "192.168.1.30:41000".parse().expect("address")),
            None
        );
    }

    #[test]
    fn an_empty_target_list_stays_empty_so_the_window_starts_with_nobody_configured() {
        let config = to_engine_config(StartRequest::default());

        assert!(config.targets.is_empty());
        assert_eq!(config.control_port, DEFAULT_CONTROL_PORT);
    }
}
