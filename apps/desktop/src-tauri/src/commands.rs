//! The engine calls the Tauri commands delegate to.
//!
//! They live apart from the `#[tauri::command]` wrappers in the crate root so that an example
//! binary can drive exactly the same code path without a window. The window is only a rendering of
//! what these functions return.

use std::collections::HashSet;
use std::hash::{BuildHasher, RandomState};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;

use personal_monitoring::{
    check_channels, check_groups, start, AudioHost, ChannelKind, ChannelMeta, EngineConfig,
    EngineEvent, EngineHandle, GroupDefinition, MixValues, MusicianStatus,
};

use crate::dto::{
    CaptureFormatDto, ChannelRequest, ChannelsSetDto, EngineStateDto, EngineStatusDto,
    GroupRequest, GroupsSetDto, InputDeviceDto, MetersDto, MixDto, MixUpdateRequest, StartRequest,
    StartSummary,
};
use crate::events::EventPump;
use crate::session::{SessionChannel, SessionGroup, SessionStore};

/// Starts the event pump for a new engine, given that engine's event bus and run.
pub type PumpStarter = Box<dyn Fn(Receiver<EngineEvent>, u64) -> EventPump + Send + Sync>;

/// The state the desktop commands share: the running engine, its event pump, and the desktop-only
/// session (see [crate::session]).
///
/// The engine handle is behind a [Mutex] because the engine is already internally concurrent: the
/// commands only need exclusive access to start, stop, edit the groups and the channel details and
/// take a snapshot. The session has its own lock. Whenever both are held, the engine lock is taken
/// first and the session's inside it (a status applies the labels, a start reads the groups and the
/// channel details, an edit saves them), never the other way round, so they cannot deadlock.
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
    session: SessionStore,
}

impl DesktopState {
    /// A state with no event pump: the engine runs and the commands answer, but nothing is pushed.
    pub fn new(session: SessionStore) -> Self {
        Self {
            engine: Mutex::new(None),
            run: AtomicU64::new(0),
            pump: Mutex::new(None),
            start_pump: None,
            session,
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
        groups: group_definitions(request.groups),
        buffer_frames: request.buffer_frames,
        host: parse_host(request.host.as_deref()),
    }
}

/// Gives every requested group its definition: the id the window sent back, or a new one for a
/// group the window has just created (no id, or an empty one). Names are trimmed.
fn group_definitions(groups: Vec<GroupRequest>) -> Vec<GroupDefinition> {
    let mut taken: HashSet<String> = groups
        .iter()
        .filter_map(|group| group.id.clone())
        .filter(|id| !id.trim().is_empty())
        .collect();
    groups
        .into_iter()
        .map(|group| GroupDefinition {
            id: match group.id {
                Some(id) if !id.trim().is_empty() => id,
                _ => new_group_id(&mut taken),
            },
            name: group.name.trim().to_owned(),
            channels: group.channels,
        })
        .collect()
}

/// A new group id: `g` and 16 random hex digits, never one in `taken`, which it joins.
///
/// Random rather than counted, so an id is never handed out twice across restarts and a new group
/// never inherits a level kept for a deleted one. The randomness is the standard library's hash
/// seed: an id needs to be unique, not secret.
fn new_group_id(taken: &mut HashSet<String>) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let seed = NEXT.fetch_add(1, Ordering::Relaxed);
        let id = format!("g{:016x}", RandomState::new().hash_one(seed));
        if taken.insert(id.clone()) {
            return id;
        }
    }
}

/// The groups to apply right after a start, through the same live path as an edit.
///
/// A request that names no groups (the window's, which always sends none) gets the session's: they
/// are applied once the engine runs, so a group naming a channel the chosen device lacks is kept and
/// shows as invalid instead of failing the start. A request that names groups starts with exactly
/// those, checked strictly as the command line's are, and the session is neither read nor changed.
fn session_groups_for(request: &StartRequest, session: &SessionStore) -> Vec<GroupDefinition> {
    if !request.groups.is_empty() {
        return Vec::new();
    }
    session
        .groups()
        .into_iter()
        .map(|group| GroupDefinition {
            id: group.id,
            name: group.name,
            channels: group.channels,
        })
        .collect()
}

/// The session's channel details as the engine takes them, applied to every engine the desk starts
/// (see [start_engine]). A kind this build does not know (a session from a newer build) applies as
/// [ChannelKind::Other] rather than failing the start.
fn session_channels(session: &SessionStore) -> Vec<ChannelMeta> {
    session
        .channels()
        .into_iter()
        .map(|channel| ChannelMeta {
            kind: ChannelKind::from_name(&channel.kind).unwrap_or_default(),
            name: channel.name,
            hidden: channel.hidden,
            linked_to_next: channel.link,
        })
        .collect()
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
///
/// The engineer's groups come from the session: a request that names none (the window always sends
/// none) gets the session's groups applied as soon as the engine runs (see [session_groups_for]).
/// The session's channel details are applied the same way to every start ([session_channels]).
/// Groups or details the engine refuses there (a hand-edited session) are logged and the engine
/// runs without them: they are a convenience and must not stop the server.
pub fn start_engine(state: &DesktopState, request: StartRequest) -> Result<StartSummary, String> {
    let mut pump = state
        .pump
        .lock()
        .map_err(|_| "event pump lock was poisoned".to_owned())?;
    stop_pump(&mut pump);
    let mut guard = state
        .engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    // Read under the engine lock (the session's inside it), so a group or channel edit cannot land
    // between this read and the new engine taking them.
    let saved_groups = session_groups_for(&request, &state.session);
    let saved_channels = session_channels(&state.session);
    let config = to_engine_config(request);
    // Under the engine lock, so a snapshot can never pair the old engine with the new run.
    let run = state.run.fetch_add(1, Ordering::SeqCst) + 1;

    if let Some(mut running) = guard.take() {
        running
            .stop()
            .map_err(|error| format!("could not stop the previous engine: {error}"))?;
    }

    match start(config).map_err(|error| error.to_string())? {
        Some(mut handle) => {
            if !saved_groups.is_empty() {
                let count = saved_groups.len();
                match handle.set_groups(saved_groups) {
                    Ok(_) => {
                        println!("[mixlink-desktop] start_engine: {count} saved groups applied")
                    }
                    Err(error) => {
                        eprintln!("[mixlink-desktop] start_engine: the saved groups were not applied: {error}")
                    }
                }
            }
            if !saved_channels.is_empty() {
                let count = saved_channels.len();
                match handle.set_channels(saved_channels) {
                    Ok(_) => println!(
                        "[mixlink-desktop] start_engine: {count} saved channel details applied"
                    ),
                    Err(error) => eprintln!(
                        "[mixlink-desktop] start_engine: the saved channel details were not applied: {error}"
                    ),
                }
            }
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
    apply_names(&mut status, &state.session);
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

/// Fills each musician row's displayed name from the session's local labels: the label kept for
/// the row's phone (its client id) first, then the one kept for its address.
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
pub fn apply_names(status: &mut EngineStatusDto, session: &SessionStore) {
    for musician in &mut status.musicians {
        let local = session.label_for(musician.client_id.as_deref(), &musician.address);
        musician.name_is_local = musician.client_name.is_none() && local.is_some();
        musician.name = musician.client_name.clone().or(local);
    }
}

/// The client id of the musician the status lists at `address` (the address string as the status
/// reports it), or `None` when that row has none or no row is there.
fn client_id_at(musicians: &[MusicianStatus], address: &str) -> Option<String> {
    musicians
        .iter()
        .find(|musician| musician.address.to_string() == address)
        .and_then(|musician| musician.client_id.clone())
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
///
/// `address` is the row's address as the status reports it. When the running engine lists a phone
/// with a client id there, the label is kept for that phone, so it stays with the musician when
/// their IP changes; otherwise it is kept for the address.
pub fn set_musician_name(
    state: &DesktopState,
    address: String,
    name: String,
) -> Result<(), String> {
    // The engine lock is released before the session's is taken, as everywhere else.
    let musicians = {
        let guard = state
            .engine
            .lock()
            .map_err(|_| "engine state lock was poisoned".to_owned())?;
        guard
            .as_ref()
            .map(|handle| handle.status().musicians)
            .unwrap_or_default()
    };
    label_musician(&state.session, &musicians, &address, &name)
}

/// Saves `name` as the label of the musician at `address`, keyed on the client id of the row
/// `musicians` lists there (the running engine's rows, none without an engine), or on the
/// address when that row has none or no row is there.
fn label_musician(
    session: &SessionStore,
    musicians: &[MusicianStatus],
    address: &str,
    name: &str,
) -> Result<(), String> {
    let client_id = client_id_at(musicians, address);
    session.set_label(client_id.as_deref(), address, name)?;
    let kept_for = client_id.map_or_else(|| "address".to_owned(), |id| format!("phone {id}"));
    println!("[mixlink-desktop] set_musician_name: {address} ({kept_for}) -> \"{name}\"");
    Ok(())
}

/// Replaces the engineer's groups: applies them to the running engine, if any, and keeps them in
/// the session. Returns what was done ([GroupsSetDto]), with the groups in their order, each with
/// its id.
///
/// `groups` is the whole list in its order (see [GroupRequest]); a group with no id is new and gets
/// one. The list is checked as a running engine checks it ([check_groups]): at most 16 groups, a
/// name each and no channel in two groups; a group naming a channel the current device lacks is
/// kept and shows as invalid. Then:
///
/// - With an engine running, the engine takes the groups first (every musician's group levels
///   follow their groups by id and every phone is sent them, see [EngineHandle::set_groups]) and
///   only then are they saved. A save that fails then is reported in the answer, not as an error,
///   because the phones already have the groups.
/// - An engine that is stopping refuses them: an error, and nothing changed.
/// - With no engine, they are saved and apply at the next start (`applied: false`).
///
/// A refused list is an error and changes nothing. Each edit is one step under the engine lock, so
/// two edits at once never leave the session and the engine with different groups.
pub fn set_groups(state: &DesktopState, groups: Vec<GroupRequest>) -> Result<GroupsSetDto, String> {
    let outcome = edit_groups(&state.engine, &state.session, groups)?;
    println!(
        "[mixlink-desktop] set_groups: {} groups, applied={} saved={}",
        outcome.groups.len(),
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// What a group edit needs from the running engine: the engine itself in the app, a stand-in in
/// the tests, which run without audio hardware.
trait GroupEngine {
    fn set_groups(&mut self, definitions: Vec<GroupDefinition>) -> Result<(), String>;
}

impl GroupEngine for EngineHandle {
    fn set_groups(&mut self, definitions: Vec<GroupDefinition>) -> Result<(), String> {
        EngineHandle::set_groups(self, definitions)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// [set_groups] over any engine slot and session.
fn edit_groups<E: GroupEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    groups: Vec<GroupRequest>,
) -> Result<GroupsSetDto, String> {
    let definitions = group_definitions(groups);
    check_groups(&definitions)?;
    let kept: Vec<SessionGroup> = definitions
        .iter()
        .map(|group| SessionGroup {
            id: group.id.clone(),
            name: group.name.clone(),
            channels: group.channels.clone(),
        })
        .collect();
    // One step per edit: the engine lock is held from the apply through the save, so two edits
    // never interleave and the session always ends with what the engine has. The session's lock
    // is taken inside it, the order every command that holds both follows.
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let applied = match guard.as_mut() {
        // A refusal (an engine that is stopping) returns here, before anything is saved.
        Some(engine) => {
            engine.set_groups(definitions)?;
            true
        }
        None => false,
    };
    let save_error = match session.set_groups(kept.clone()) {
        Ok(()) => None,
        // The phones already have the groups: say that they were not kept.
        Err(error) if applied => Some(error),
        // Nothing took them, so nothing changed.
        Err(error) => return Err(error),
    };
    Ok(GroupsSetDto {
        groups: kept,
        applied,
        saved: save_error.is_none(),
        save_error,
    })
}

/// Replaces the engineer's channel details: applies them to the running engine, if any, and keeps
/// them in the session, exactly as [set_groups] does for groups. Returns what was done
/// ([ChannelsSetDto]).
///
/// `channels` is the whole list in channel order, the first entry CH 1 (see [ChannelRequest]).
/// Names are trimmed, and the list is checked as the engine checks it ([check_channels]): a kind
/// the engine knows, at most 32 entries, names of at most 32 characters, stereo pairs only between
/// two listed inputs and never overlapping, both sides of a pair hidden or both shown. Then:
///
/// - With an engine running, the engine takes them first (every phone is sent them; a hidden input
///   goes silent in every mix; a new pair is panned apart in every mix, see
///   [EngineHandle::set_channels]) and only then are they saved. A save that fails then is reported
///   in the answer, not as an error, because the phones already have them.
/// - An engine that is stopping refuses them: an error, and nothing changed.
/// - With no engine, they are saved and apply at the next start (`applied: false`).
///
/// A refused list is an error and changes nothing. Each edit is one step under the engine lock,
/// with the session's inside it, so two edits at once never leave the session and the engine with
/// different details.
pub fn set_channels(
    state: &DesktopState,
    channels: Vec<ChannelRequest>,
) -> Result<ChannelsSetDto, String> {
    let outcome = edit_channels(&state.engine, &state.session, channels)?;
    println!(
        "[mixlink-desktop] set_channels: {} channels, applied={} saved={}",
        outcome.channels.len(),
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// What a channel edit needs from the running engine, as [GroupEngine] is for a group edit.
trait ChannelEngine {
    fn set_channels(&mut self, channels: Vec<ChannelMeta>) -> Result<(), String>;
}

impl ChannelEngine for EngineHandle {
    fn set_channels(&mut self, channels: Vec<ChannelMeta>) -> Result<(), String> {
        EngineHandle::set_channels(self, channels)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// The window's channel details as the engine takes them and as the session keeps them, names
/// trimmed; a kind the engine does not know is an error naming it.
fn channel_details(
    requests: Vec<ChannelRequest>,
) -> Result<(Vec<ChannelMeta>, Vec<SessionChannel>), String> {
    let mut details = Vec::with_capacity(requests.len());
    let mut kept = Vec::with_capacity(requests.len());
    for (index, request) in requests.into_iter().enumerate() {
        let kind = ChannelKind::from_name(&request.kind).ok_or_else(|| {
            let known: Vec<&str> = ChannelKind::ALL.iter().map(|kind| kind.name()).collect();
            format!(
                "CH {} has the instrument kind `{}`, which is none of {}",
                index + 1,
                request.kind,
                known.join(", ")
            )
        })?;
        let name = request.name.trim().to_owned();
        details.push(ChannelMeta {
            name: name.clone(),
            kind,
            hidden: request.hidden,
            linked_to_next: request.link,
        });
        kept.push(SessionChannel {
            name,
            kind: kind.name().to_owned(),
            hidden: request.hidden,
            link: request.link,
        });
    }
    check_channels(&details)?;
    Ok((details, kept))
}

/// [set_channels] over any engine slot and session.
fn edit_channels<E: ChannelEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    channels: Vec<ChannelRequest>,
) -> Result<ChannelsSetDto, String> {
    let (details, kept) = channel_details(channels)?;
    // One step per edit, in the lock order every command that holds both follows (see
    // [edit_groups]).
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let applied = match guard.as_mut() {
        // A refusal (an engine that is stopping) returns here, before anything is saved.
        Some(engine) => {
            engine.set_channels(details)?;
            true
        }
        None => false,
    };
    let save_error = match session.set_channels(kept.clone()) {
        Ok(()) => None,
        // The phones already have the details: say that they were not kept.
        Err(error) if applied => Some(error),
        // Nothing took them, so nothing changed.
        Err(error) => return Err(error),
    };
    Ok(ChannelsSetDto {
        channels: kept,
        applied,
        saved: save_error.is_none(),
        save_error,
    })
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
                    client_id: None,
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

    /// An empty scratch config directory for one test's session.
    fn session_directory(test: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-command-tests")
            .join(test);
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    /// The engine's row for the phone `client_id` at `address`, every other value neutral.
    fn engine_row(address: &str, client_id: Option<&str>) -> personal_monitoring::MusicianStatus {
        personal_monitoring::MusicianStatus {
            address: address.parse().expect("test address"),
            name: None,
            client_id: client_id.map(str::to_owned),
            control_connected: true,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
        }
    }

    #[test]
    fn status_rows_take_their_labels_from_the_session() {
        let session = SessionStore::open(&session_directory("labels"));
        session
            .set_label(None, "192.168.1.30:50000", "Ana")
            .expect("setting a label should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000", "192.168.1.31:50000"]);

        apply_names(&mut status, &session);

        assert_eq!(status.musicians[0].name.as_deref(), Some("Ana"));
        assert!(status.musicians[0].name_is_local);
        assert_eq!(status.musicians[1].name, None);
        assert!(!status.musicians[1].name_is_local);
    }

    #[test]
    fn the_clients_own_name_beats_the_local_label() {
        let session = SessionStore::open(&session_directory("client-wins"));
        session
            .set_label(Some(PHONE), "192.168.1.30:50000", "Lead vocal")
            .expect("setting a label should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);
        status.musicians[0].client_name = Some("Ana".to_owned());
        status.musicians[0].client_id = Some(PHONE.to_owned());

        apply_names(&mut status, &session);

        // The phone is the authority on its own name: the desk's stored label must not shadow it,
        // and the row must not report the label as if it were the name on screen.
        assert_eq!(status.musicians[0].name.as_deref(), Some("Ana"));
        assert_eq!(status.musicians[0].client_name.as_deref(), Some("Ana"));
        assert!(!status.musicians[0].name_is_local);
    }

    #[test]
    fn a_local_label_applies_when_the_client_announced_nothing() {
        let session = SessionStore::open(&session_directory("silent-client"));
        session
            .set_label(None, "192.168.1.30:50000", "Rack A")
            .expect("setting a label should persist");
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);

        apply_names(&mut status, &session);

        assert_eq!(status.musicians[0].name.as_deref(), Some("Rack A"));
        assert_eq!(status.musicians[0].client_name, None);
        assert!(status.musicians[0].name_is_local);
    }

    #[test]
    fn a_row_with_no_client_name_and_no_label_stays_unnamed() {
        let session = SessionStore::open(&session_directory("unnamed"));
        let mut status = status_with_musicians(&["192.168.1.30:50000"]);

        apply_names(&mut status, &session);

        assert_eq!(status.musicians[0].name, None);
        assert!(!status.musicians[0].name_is_local);
    }

    #[test]
    fn a_rename_is_kept_for_the_rows_phone_when_it_has_a_client_id() {
        let rows = [
            engine_row("192.168.1.10:50000", Some(PHONE)),
            engine_row("192.168.1.40:50000", None),
        ];

        assert_eq!(
            client_id_at(&rows, "192.168.1.10:50000").as_deref(),
            Some(PHONE)
        );
        assert_eq!(client_id_at(&rows, "192.168.1.40:50000"), None, "no id");
        assert_eq!(client_id_at(&rows, "192.168.1.99:50000"), None, "no row");
    }

    #[test]
    fn a_musician_keeps_their_label_after_their_phones_ip_changes() {
        let session = SessionStore::open(&session_directory("ip-change"));
        // The engineer labels the phone while it is at its first address, as the window does.
        let before = [engine_row("192.168.1.10:50000", Some(PHONE))];
        label_musician(&session, &before, "192.168.1.10:50000", "Ana")
            .expect("setting a label should persist");
        // DHCP gives the phone another IP; the engine moves its row and keeps its client id.
        let mut status = status_with_musicians(&["192.168.1.22:50000"]);
        status.musicians[0].client_id = Some(PHONE.to_owned());

        apply_names(&mut status, &session);

        assert_eq!(status.musicians[0].name.as_deref(), Some("Ana"));
        assert!(status.musicians[0].name_is_local);
    }

    /// The saved session's `musicians` section in `test`'s scratch directory.
    fn saved_musicians(test: &str) -> serde_json::Value {
        let path = std::env::temp_dir()
            .join("mixlink-command-tests")
            .join(test)
            .join(crate::session::SESSION_FILE);
        let text = std::fs::read_to_string(path).expect("the session should be saved");
        serde_json::from_str::<serde_json::Value>(&text).expect("the session is JSON")["musicians"]
            .clone()
    }

    #[test]
    fn naming_a_musician_without_an_engine_keeps_the_label_for_the_address() {
        let state = DesktopState::new(SessionStore::open(&session_directory("name-idle")));

        set_musician_name(&state, "192.168.1.40:50000".to_owned(), "Rack A".to_owned())
            .expect("the label is saved without an engine");

        assert_eq!(
            saved_musicians("name-idle"),
            serde_json::json!({ "address:192.168.1.40:50000": { "label": "Rack A" } })
        );
    }

    #[test]
    fn naming_a_listed_phone_keeps_the_label_for_its_client_id() {
        let session = SessionStore::open(&session_directory("name-phone"));
        let rows = [
            engine_row("192.168.1.10:50000", Some(PHONE)),
            engine_row("192.168.1.40:50000", None),
        ];

        label_musician(&session, &rows, "192.168.1.10:50000", "Ana").expect("saved");
        label_musician(&session, &rows, "192.168.1.40:50000", "Rack A").expect("saved");

        assert_eq!(
            saved_musicians("name-phone"),
            serde_json::json!({
                format!("client:{PHONE}"): { "label": "Ana" },
                "address:192.168.1.40:50000": { "label": "Rack A" }
            })
        );
    }

    #[test]
    fn setting_a_mix_without_a_running_engine_says_so() {
        let state = DesktopState::new(SessionStore::open(&session_directory("set-mix-idle")));

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
        let state = DesktopState::new(SessionStore::open(&session_directory("stop-pump")));
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
        let state = DesktopState::new(SessionStore::open(&session_directory("restart-pump")));
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
        let state = DesktopState::new(SessionStore::open(&session_directory("state-idle")));

        let idle = engine_state(&state).expect("the state is always readable");
        let json = serde_json::to_value(&idle).expect("the state serializes");

        assert_eq!(json, serde_json::json!({ "state": "notRunning", "run": 0 }));
    }

    #[test]
    fn after_a_failed_start_the_state_names_the_run_that_ended() {
        let state = DesktopState::new(SessionStore::open(&session_directory("state-failed")));
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
        let state = DesktopState::new(SessionStore::open(&session_directory("runs")));
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
        let state = DesktopState::new(SessionStore::open(&session_directory("meters-idle")));

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

    fn request(id: Option<&str>, name: &str, channels: &[usize]) -> GroupRequest {
        GroupRequest {
            id: id.map(str::to_owned),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    #[test]
    fn a_new_group_gets_a_fresh_id_and_a_known_one_keeps_its_own() {
        let definitions = group_definitions(vec![
            request(Some("kit"), "Drums", &[1]),
            request(None, "Vocals", &[2]),
            request(Some(" "), "Keys", &[3]),
            request(None, "Bass", &[4]),
        ]);

        assert_eq!(definitions[0].id, "kit");
        let fresh: Vec<&str> = definitions[1..]
            .iter()
            .map(|group| group.id.as_str())
            .collect();
        assert!(fresh.iter().all(|id| !id.trim().is_empty() && *id != "kit"));
        assert_ne!(fresh[0], fresh[1]);
        assert_ne!(fresh[1], fresh[2]);
        assert_ne!(fresh[0], fresh[2]);
        assert_eq!(definitions[3].name, "Bass");
        assert_eq!(definitions[3].channels, vec![4]);
    }

    #[test]
    fn groups_set_without_an_engine_are_kept_with_their_ids_for_the_next_start() {
        let state = DesktopState::new(SessionStore::open(&session_directory("groups-idle")));

        let outcome = set_groups(
            &state,
            vec![
                request(None, " Drums ", &[1, 2]),
                request(None, "Vocals", &[3]),
            ],
        )
        .expect("the groups are kept");

        assert!(
            !outcome.applied,
            "no engine took them: they apply at the next start"
        );
        assert!(outcome.saved);
        assert_eq!(outcome.save_error, None);
        let kept = outcome.groups;
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "Drums", "names are trimmed");
        assert_eq!(state.session.groups(), kept);
        // The window sends the ids back with the next edit, and they stay.
        let renamed = set_groups(
            &state,
            vec![
                request(Some(&kept[1].id), "Lead vocal", &[3]),
                request(Some(&kept[0].id), "Drums", &[1, 2]),
            ],
        )
        .expect("the groups are kept")
        .groups;
        assert_eq!(renamed[0].id, kept[1].id);
        assert_eq!(renamed[1].id, kept[0].id);
    }

    /// Stands in for a running engine in a group edit: it records what it took, can be stopping,
    /// and takes a moment to apply, as the real engine's regroup does. With a [Park] it holds the
    /// next edit that reaches it right after taking its groups, before that edit is saved.
    #[derive(Default)]
    struct StandInEngine {
        applied: Option<Vec<GroupDefinition>>,
        applied_channels: Option<Vec<ChannelMeta>>,
        stopping: bool,
        park: Option<Park>,
    }

    /// Holds one group edit between its apply and its save while another edit runs: the edit
    /// says it is parked, then waits until the session's groups change (another edit saved while
    /// this one was half done) or `wait` passes, and notes which.
    struct Park {
        session: std::sync::Arc<SessionStore>,
        parked: std::sync::mpsc::Sender<()>,
        wait: std::time::Duration,
        saved_meanwhile: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Park {
        fn hold(self) {
            let before = self.session.groups();
            self.parked.send(()).expect("the test waits for the park");
            let deadline = std::time::Instant::now() + self.wait;
            while std::time::Instant::now() < deadline {
                if self.session.groups() != before {
                    self.saved_meanwhile
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }
    }

    impl GroupEngine for StandInEngine {
        fn set_groups(&mut self, definitions: Vec<GroupDefinition>) -> Result<(), String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
            self.applied = Some(definitions);
            if let Some(park) = self.park.take() {
                park.hold();
            }
            Ok(())
        }
    }

    impl ChannelEngine for StandInEngine {
        fn set_channels(&mut self, channels: Vec<ChannelMeta>) -> Result<(), String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.applied_channels = Some(channels);
            Ok(())
        }
    }

    fn running(engine: StandInEngine) -> Mutex<Option<StandInEngine>> {
        Mutex::new(Some(engine))
    }

    fn applied_names(engine: &Mutex<Option<StandInEngine>>) -> Option<Vec<String>> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .and_then(|engine| engine.applied.as_ref())
            .map(|groups| groups.iter().map(|group| group.name.clone()).collect())
    }

    #[test]
    fn a_running_engine_takes_the_groups_and_then_they_are_saved() {
        let session = SessionStore::open(&session_directory("groups-running"));
        let engine = running(StandInEngine::default());

        let outcome = edit_groups(&engine, &session, vec![request(None, "Drums", &[1, 2])])
            .expect("the groups are taken");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(applied_names(&engine), Some(vec!["Drums".to_owned()]));
        assert_eq!(session.groups(), outcome.groups);
    }

    #[test]
    fn an_engine_that_is_stopping_refuses_the_groups_and_nothing_is_saved() {
        let session = SessionStore::open(&session_directory("groups-stopping"));
        session
            .set_groups(vec![SessionGroup {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1],
            }])
            .expect("kept");
        let before = session.groups();
        let engine = running(StandInEngine {
            stopping: true,
            ..StandInEngine::default()
        });

        let error = edit_groups(&engine, &session, vec![request(None, "Vocals", &[3])])
            .expect_err("a stopping engine refuses");

        assert!(error.contains("stopping"), "{error}");
        assert_eq!(session.groups(), before, "a refused list changes nothing");
        assert_eq!(applied_names(&engine), None);
    }

    #[test]
    fn groups_a_running_engine_took_but_the_session_could_not_keep_say_so() {
        let directory = session_directory("groups-unsaved");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            r#"{"version":99}"#,
        )
        .expect("a newer session");
        let session = SessionStore::open(&directory);
        let engine = running(StandInEngine::default());

        let outcome = edit_groups(&engine, &session, vec![request(None, "Drums", &[1])])
            .expect("the engine took them");

        assert!(outcome.applied);
        assert!(!outcome.saved);
        assert!(
            outcome
                .save_error
                .as_deref()
                .is_some_and(|error| error.contains("newer")),
            "{outcome:?}"
        );
        assert_eq!(applied_names(&engine), Some(vec!["Drums".to_owned()]));
    }

    /// Two windows (or two quick clicks) edit the groups at the same moment. Every round forces the
    /// overlap: the first edit is parked right after the engine took its groups, and the second
    /// edit runs meanwhile. Under a save-then-apply order the second edit's groups reach the
    /// session while the first edit is half done (the order that leaves the session and the engine
    /// apart when the applies then land the other way round), and the park reports it. Since each
    /// edit is one step under the engine lock, the second edit waits until the first is saved, and
    /// the session and the engine end with the second edit's groups.
    #[test]
    fn two_group_edits_at_once_leave_the_session_and_the_engine_with_the_same_groups() {
        let session =
            std::sync::Arc::new(SessionStore::open(&session_directory("groups-concurrent")));
        let engine = running(StandInEngine::default());
        for round in 0..6 {
            let (first, second) = if round % 2 == 0 {
                ("First", "Second")
            } else {
                ("Second", "First")
            };
            let (parked, parked_signal) = std::sync::mpsc::channel();
            let saved_meanwhile = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            engine
                .lock()
                .expect("engine lock")
                .as_mut()
                .expect("a running stand-in")
                .park = Some(Park {
                session: std::sync::Arc::clone(&session),
                parked,
                wait: std::time::Duration::from_millis(100),
                saved_meanwhile: std::sync::Arc::clone(&saved_meanwhile),
            });

            std::thread::scope(|scope| {
                let (engine, session) = (&engine, &session);
                scope.spawn(move || {
                    edit_groups(engine, session, vec![request(Some(first), first, &[1])])
                        .expect("a valid edit");
                });
                parked_signal
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .expect("the first edit reached the engine");
                // The second edit runs while the first sits between its apply and its save.
                scope.spawn(move || {
                    edit_groups(engine, session, vec![request(Some(second), second, &[1])])
                        .expect("a valid edit");
                });
            });

            assert!(
                !saved_meanwhile.load(std::sync::atomic::Ordering::SeqCst),
                "round {round}: the second edit was saved while the first was between its apply \
                 and its save"
            );
            let saved: Vec<String> = session
                .groups()
                .iter()
                .map(|group| group.name.clone())
                .collect();
            assert_eq!(
                saved,
                vec![second.to_owned()],
                "round {round}: the later edit wins"
            );
            assert_eq!(applied_names(&engine), Some(saved), "round {round}");
        }
    }

    #[test]
    fn groups_a_running_engine_would_refuse_are_refused_and_nothing_is_saved() {
        let state = DesktopState::new(SessionStore::open(&session_directory("groups-refused")));
        set_groups(&state, vec![request(Some("kit"), "Drums", &[1])]).expect("kept");

        let error = set_groups(
            &state,
            vec![
                request(Some("kit"), "Drums", &[1, 2]),
                request(None, "Vocals", &[2]),
            ],
        )
        .expect_err("channel 2 cannot be in two groups");

        assert!(
            error.contains("Drums") && error.contains("Vocals"),
            "{error}"
        );
        assert_eq!(
            state.session.groups().len(),
            1,
            "the kept groups are unchanged"
        );
    }

    #[test]
    fn a_start_that_names_no_groups_applies_the_sessions_and_one_that_names_some_uses_its_own() {
        let session = SessionStore::open(&session_directory("groups-start"));
        session
            .set_groups(vec![crate::session::SessionGroup {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1, 2],
            }])
            .expect("kept");

        let from_session = session_groups_for(&StartRequest::default(), &session);
        let own = StartRequest {
            groups: vec![request(None, "Vocals", &[3])],
            ..StartRequest::default()
        };

        assert_eq!(
            from_session,
            vec![GroupDefinition {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1, 2],
            }]
        );
        assert!(session_groups_for(&own, &session).is_empty());
        assert_eq!(to_engine_config(own).groups[0].name, "Vocals");
    }

    fn detail(name: &str, kind: &str, hidden: bool, link: bool) -> ChannelRequest {
        ChannelRequest {
            name: name.to_owned(),
            kind: kind.to_owned(),
            hidden,
            link,
        }
    }

    fn kept(name: &str, kind: &str, hidden: bool, link: bool) -> SessionChannel {
        SessionChannel {
            name: name.to_owned(),
            kind: kind.to_owned(),
            hidden,
            link,
        }
    }

    fn applied_channels(engine: &Mutex<Option<StandInEngine>>) -> Option<Vec<ChannelMeta>> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .and_then(|engine| engine.applied_channels.clone())
    }

    #[test]
    fn channels_set_without_an_engine_are_kept_for_the_next_start() {
        let state = DesktopState::new(SessionStore::open(&session_directory("channels-idle")));

        let outcome = set_channels(
            &state,
            vec![
                detail(" Kick ", "drums", false, false),
                detail("Keys L", "keys", false, true),
                detail("Keys R", "keys", false, false),
            ],
        )
        .expect("the details are kept");

        assert!(!outcome.applied, "no engine took them");
        assert!(outcome.saved);
        assert_eq!(outcome.save_error, None);
        assert_eq!(
            outcome.channels,
            vec![
                kept("Kick", "drums", false, false),
                kept("Keys L", "keys", false, true),
                kept("Keys R", "keys", false, false),
            ],
            "names are trimmed"
        );
        assert_eq!(state.session.channels(), outcome.channels);
    }

    #[test]
    fn a_running_engine_takes_the_channels_and_then_they_are_saved() {
        let session = SessionStore::open(&session_directory("channels-running"));
        let engine = running(StandInEngine::default());

        let outcome = edit_channels(
            &engine,
            &session,
            vec![detail("Spare", "other", true, false)],
        )
        .expect("the details are taken");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(
            applied_channels(&engine),
            Some(vec![ChannelMeta {
                name: "Spare".to_owned(),
                kind: ChannelKind::Other,
                hidden: true,
                linked_to_next: false,
            }])
        );
        assert_eq!(session.channels(), outcome.channels);
    }

    #[test]
    fn an_engine_that_is_stopping_refuses_the_channels_and_nothing_is_saved() {
        let session = SessionStore::open(&session_directory("channels-stopping"));
        session
            .set_channels(vec![kept("Kick", "drums", false, false)])
            .expect("kept");
        let before = session.channels();
        let engine = running(StandInEngine {
            stopping: true,
            ..StandInEngine::default()
        });

        let error = edit_channels(
            &engine,
            &session,
            vec![detail("Bass", "bass", false, false)],
        )
        .expect_err("a stopping engine refuses");

        assert!(error.contains("stopping"), "{error}");
        assert_eq!(session.channels(), before, "a refused list changes nothing");
        assert_eq!(applied_channels(&engine), None);
    }

    #[test]
    fn channels_a_running_engine_took_but_the_session_could_not_keep_say_so() {
        let directory = session_directory("channels-unsaved");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            r#"{"version":99}"#,
        )
        .expect("a newer session");
        let session = SessionStore::open(&directory);
        let engine = running(StandInEngine::default());

        let outcome = edit_channels(
            &engine,
            &session,
            vec![detail("Kick", "drums", false, false)],
        )
        .expect("the engine took them");

        assert!(outcome.applied);
        assert!(!outcome.saved);
        assert!(
            outcome
                .save_error
                .as_deref()
                .is_some_and(|error| error.contains("newer")),
            "{outcome:?}"
        );
        assert!(applied_channels(&engine).is_some());
    }

    #[test]
    fn channel_details_the_engine_would_refuse_are_refused_and_nothing_is_saved() {
        let session = SessionStore::open(&session_directory("channels-refused"));
        session
            .set_channels(vec![kept("Kick", "drums", false, false)])
            .expect("kept");
        let engine = running(StandInEngine::default());

        for (refused, named) in [
            (vec![detail("Kazoo", "kazoo", false, false)], "kazoo"),
            (vec![detail("Pad", "keys", false, true)], "CH 1"),
            (
                vec![
                    detail("L", "keys", false, true),
                    detail("R", "keys", true, false),
                ],
                "CH 1",
            ),
            (vec![detail(&"x".repeat(33), "other", false, false)], "CH 1"),
        ] {
            let error = edit_channels(&engine, &session, refused).expect_err("refused");
            assert!(error.contains(named), "names {named}: {error}");
        }

        assert_eq!(
            session.channels(),
            vec![kept("Kick", "drums", false, false)]
        );
        assert_eq!(applied_channels(&engine), None, "the engine took nothing");
    }

    #[test]
    fn a_start_applies_the_sessions_channel_details_reading_an_unknown_kind_as_other() {
        let session = SessionStore::open(&session_directory("channels-start"));
        session
            .set_channels(vec![
                kept("Kick", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ])
            .expect("kept");

        assert_eq!(
            session_channels(&session),
            vec![
                ChannelMeta {
                    name: "Kick".to_owned(),
                    kind: ChannelKind::Drums,
                    hidden: false,
                    linked_to_next: false,
                },
                ChannelMeta {
                    name: "Theremin".to_owned(),
                    kind: ChannelKind::Other,
                    hidden: true,
                    linked_to_next: false,
                },
            ]
        );
    }
}
