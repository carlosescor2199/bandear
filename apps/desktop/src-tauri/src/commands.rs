//! The engine calls the Tauri commands delegate to.
//!
//! They live apart from the `#[tauri::command]` wrappers in the crate root so that an example
//! binary can drive exactly the same code path without a window. The window is only a rendering of
//! what these functions return. The engineer's local mix and its output have their own module,
//! [local], and so do recording takes, [recording], and playing one back in place of the inputs,
//! [soundcheck].

pub mod backups;
pub mod effects;
pub mod limiter;
pub mod link_history;
pub mod local;
pub mod playback_buffer;
pub mod processing;
pub mod recording;
pub mod redundancy;
pub mod soundcheck;

use std::collections::{BTreeMap, HashSet};
use std::hash::{BuildHasher, RandomState};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use personal_monitoring::{
    check_channels, check_groups, log_info, log_warn, start, AudioHost, ChannelKind, ChannelMeta,
    EngineConfig, EngineEvent, EngineHandle, EngineerControls, EngineerUpdate, GroupDefinition,
    MirrorOutcome, MirrorSource, MixPreset, MixUpdate, MixValues, MusicianKey, MusicianStatus,
    PlaybackBuffer, PresetChannel, Redundancy, StripSettings,
};

use self::local::saver::{LocalMixSaver, SAVE_INTERVAL};
use self::local::{apply_kept_local, kept_local, with_pending, LocalEngine};
use self::processing::{ProcessingEngine, ProcessingSaver};
use self::recording::{FolderOpener, RecordingsFolder, SystemFileManager};
use crate::backups::{BackupStore, BACKUPS_DIRECTORY, KEPT_BACKUPS};
use crate::dto::{
    locked_table, session_key, AudioSettingsRequest, CaptureFormatDto, ChannelRequest,
    ChannelsSetDto, EngineStateDto, EngineStatusDto, EngineerControlsRequest,
    EngineerControlsSetDto, GroupRequest, GroupsSetDto, InputDeviceDto, LoadedMixDto, MetersDto,
    MirrorRequest, MirrorSetDto, MixDto, MixUpdateRequest, MixesSetDto, PresetsDto,
    SessionLoadedDto, SessionSavedDto, SkippedMixDto, StartRequest, StartSummary,
};
use crate::events::EventPump;
use crate::session::{
    address_key, client_key, Session, SessionAudio, SessionChannel, SessionEngineer, SessionGroup,
    SessionLocal, SessionMirror, SessionPreset, SessionPresetChannel, SessionPresetMix,
    SessionStore,
};
use crate::sessions::{
    SessionMix, SnapshotRead, SnapshotStore, SnapshotSummary, SESSIONS_DIRECTORY,
};

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
    /// Shared with [Self::local_saver]'s thread, which keeps the local mix in it.
    session: Arc<SessionStore>,
    /// The engineer's named sessions, in the `sessions` folder beside the session file (see
    /// [crate::sessions]).
    snapshots: SnapshotStore,
    /// Why the local output the session keeps was not opened by the running engine (see
    /// [local]), for the status to say; taken inside the engine lock, never the other way round.
    local_notice: Mutex<Option<String>>,
    /// Keeps the local mix in the session off the engine lock (see [local::saver]). Paused inside
    /// the engine lock by a start and a load; it writes taking only the session's lock.
    local_saver: LocalMixSaver,
    /// Keeps each input's channel processing in the session off the engine lock (see
    /// [processing]), as [Self::local_saver] keeps the local mix, and paused the same way.
    processing_saver: ProcessingSaver,
    /// The desk's default recordings folder (see [recording]).
    recordings: RecordingsFolder,
    /// Opens a take folder in the system's file manager (see [recording::open_take_folder]).
    folder_opener: Box<dyn FolderOpener>,
    /// The musicians' mixes, backed up as they change in the `backups` folder beside the session
    /// file (see [backups]). Read under the engine lock, written off it.
    mix_backups: backups::MixBackups,
}

impl DesktopState {
    /// A state with no event pump: the engine runs and the commands answer, but nothing is pushed.
    /// Takes go to the default [recording::default_recordings_folder] falls back to (never inside
    /// the session's folder, the app's config folder) until [Self::with_recordings_folder] names
    /// the desk's default.
    pub fn new(session: SessionStore) -> Self {
        let recordings = recording::default_recordings_folder(
            None,
            Some(session.directory().to_path_buf()),
            session.directory(),
        );
        let snapshots = SnapshotStore::new(session.directory().join(SESSIONS_DIRECTORY));
        let mix_backups = backups::MixBackups::start(BackupStore::new(
            session.directory().join(BACKUPS_DIRECTORY),
            KEPT_BACKUPS,
        ));
        let session = Arc::new(session);
        let local_saver = {
            let session = Arc::clone(&session);
            LocalMixSaver::start(
                move |mix| session.set_local_mix(mix.clone()),
                |line| log_warn!("desk", "{line}"),
                SAVE_INTERVAL,
            )
        };
        let processing_saver = {
            let session = Arc::clone(&session);
            ProcessingSaver::start(
                move |kept| session.set_processing(kept.clone()),
                |line| log_warn!("desk", "{line}"),
                SAVE_INTERVAL,
            )
        };
        Self {
            engine: Mutex::new(None),
            run: AtomicU64::new(0),
            pump: Mutex::new(None),
            start_pump: None,
            session,
            snapshots,
            local_notice: Mutex::new(None),
            local_saver,
            processing_saver,
            recordings,
            folder_opener: Box::new(SystemFileManager),
            mix_backups,
        }
    }

    /// Makes `recordings` the desk's default recordings folder (see
    /// [recording::default_recordings_folder]).
    pub fn with_recordings_folder(mut self, recordings: RecordingsFolder) -> Self {
        self.recordings = recordings;
        self
    }

    /// Keeps `notice` as what the status says about the kept local output, or clears it.
    fn set_local_notice(&self, notice: Option<String>) {
        if let Ok(mut kept) = self.local_notice.lock() {
            *kept = notice;
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

/// What the crate's tests, which run without an engine, need of the local mix's keeping.
#[cfg(test)]
impl DesktopState {
    /// Marks `mix` to be kept, as a local mix edit does.
    pub(crate) fn mark_local_mix(&self, mix: SessionMix) {
        self.local_saver.mark(mix);
    }

    /// The local section the session keeps now.
    pub(crate) fn kept_local(&self) -> Option<SessionLocal> {
        self.session.local()
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

/// What a start says on a machine with no input device: it never falls back to the demo source on
/// its own, it says how to choose it.
const NO_INPUT_DEVICE: &str = "No input device is available. Connect an audio interface and try \
     again, or choose the Demo driver to run the demo source without one.";

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
    new_id('g', taken)
}

/// A new id: `prefix` and 16 random hex digits, never one in `taken`, which it joins (see
/// [new_group_id]).
fn new_id(prefix: char, taken: &mut HashSet<String>) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let seed = NEXT.fetch_add(1, Ordering::Relaxed);
        let id = format!("{prefix}{:016x}", RandomState::new().hash_one(seed));
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

/// The engineer's controls the session keeps, as the engine takes them at start (see
/// [EngineHandle::keep_engineer_controls]): each under the key it is kept for, a `client:` key as
/// the phone's client id and an `address:` key as that address. A limit above 100 reads as 100 and
/// a channel number outside 1-32 is left out; a key this build cannot read is skipped and logged.
fn session_engineer_controls(session: &SessionStore) -> Vec<(MusicianKey, EngineerControls)> {
    controls_of(&session.current())
}

/// [session_engineer_controls] of a whole session, the one kept or one being loaded.
fn controls_of(session: &Session) -> Vec<(MusicianKey, EngineerControls)> {
    session
        .engineer_controls()
        .into_iter()
        .filter_map(|(key, kept)| {
            let Some(musician) = musician_key_of(&key) else {
                log_warn!(
                    "desk",
                    "the engineer's controls kept for `{key}` are skipped: it names no phone or address"
                );
                return None;
            };
            Some((musician, engineer_controls(&kept)))
        })
        .collect()
}

/// The musician a session key names: a `client:` key as the phone's client id, an `address:` key
/// as that address; `None` for a key this build cannot read.
fn musician_key_of(key: &str) -> Option<MusicianKey> {
    if let Some(id) = key.strip_prefix("client:") {
        return Some(MusicianKey::Client(id.to_owned()));
    }
    key.strip_prefix("address:")
        .and_then(|address| address.parse().ok())
        .map(MusicianKey::Address)
}

/// The mirrors the session keeps, as the engine takes them at start (see
/// [EngineHandle::keep_mirror]): the follower's key, the leader's key and the name the follower's
/// phone is told. A mirror whose follower or leader key this build cannot read is skipped and
/// logged.
fn session_mirrors(session: &SessionStore) -> Vec<(MusicianKey, MusicianKey, String)> {
    mirrors_of(&session.current())
}

/// [session_mirrors] of a whole session, the one kept or one being loaded.
fn mirrors_of(session: &Session) -> Vec<(MusicianKey, MusicianKey, String)> {
    session
        .mirrors()
        .into_iter()
        .filter_map(|(key, mirror)| {
            match (musician_key_of(&key), musician_key_of(&mirror.leader)) {
                (Some(follower), Some(leader)) => Some((follower, leader, mirror.name)),
                _ => {
                    log_warn!(
                        "desk",
                        "the mirror kept for `{key}` on `{}` is skipped: it names no phone or address",
                        mirror.leader
                    );
                    None
                }
            }
        })
        .collect()
}

/// The controls the session keeps, as the engine takes them.
fn engineer_controls(kept: &SessionEngineer) -> EngineerControls {
    EngineerControls {
        limit_percent: kept.limit_percent.min(100),
        locked: kept
            .locked_channels
            .iter()
            .filter(|&&number| (1..=32).contains(&number))
            .fold(0, |mask, &number| mask | 1 << (number - 1)),
    }
}

/// The controls the engine has, as the session keeps them: locked channels by their 1-based
/// numbers.
fn session_engineer(controls: &EngineerControls) -> SessionEngineer {
    SessionEngineer {
        limit_percent: controls.limit_percent,
        locked_channels: (0..32)
            .filter(|&channel| controls.is_locked(channel))
            .map(|channel| channel + 1)
            .collect(),
    }
}

/// What a start needs from the new engine to put the session's mirrors in force: the engine in
/// the app, a stand-in in the tests.
trait MirrorKeeper {
    fn keep_mirror(
        &self,
        follower: &MusicianKey,
        leader: &MusicianKey,
        name: &str,
    ) -> Result<(), String>;
}

impl MirrorKeeper for EngineHandle {
    fn keep_mirror(
        &self,
        follower: &MusicianKey,
        leader: &MusicianKey,
        name: &str,
    ) -> Result<(), String> {
        EngineHandle::keep_mirror(self, follower, leader, name).map_err(|error| error.to_string())
    }
}

/// Hands every mirror in `mirrors` (see [session_mirrors]) to a new engine, in order, and answers a
/// line for each one it refused (a circle or a mirror on oneself in a hand-edited session), for
/// the log: a refused mirror is left out and the engine runs without it.
fn hand_kept_mirrors<E: MirrorKeeper>(
    engine: &E,
    mirrors: Vec<(MusicianKey, MusicianKey, String)>,
) -> Vec<String> {
    mirrors
        .into_iter()
        .filter_map(|(follower, leader, name)| {
            engine
                .keep_mirror(&follower, &leader, &name)
                .err()
                .map(|error| {
                    format!(
                        "the saved mirror of {follower:?} on {leader:?} was not applied: {error}"
                    )
                })
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
/// The session's channel details are applied the same way to every start ([session_channels]),
/// and then each input's channel processing the session keeps, a change not written yet included
/// ([processing::hand_kept_processing]); one the engine refuses is logged and every strip stays
/// neutral. Groups or details the engine refuses there (a hand-edited session) are logged and the engine
/// runs without them: they are a convenience and must not stop the server. The engineer's controls
/// the session keeps for each musician are given to the engine after them
/// ([session_engineer_controls]): a configured target has them at once, and a phone gets them when
/// it registers, from whatever IP, before its first mix is handled. The mirrors the session keeps
/// follow the same way ([session_mirrors]); one the engine refuses (a circle in a hand-edited
/// session) is logged and left out. Last, the engineer's local mix and its output the session
/// keeps ([local::apply_kept_local]): the mix through the desk's path, then the output, opened only
/// when the running driver offers it; one it does not offer, or that cannot be opened, leaves the
/// local output off, never on another device, and the status says why (`localOutput.notice`).
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
    let saved_controls = session_engineer_controls(&state.session);
    let saved_mirrors = session_mirrors(&state.session);
    let saved_buffers = playback_buffer::buffers_of(&state.session.current());
    let saved_redundancies = redundancy::redundancies_of(&state.session.current());
    // The limiters, the musicians' and the local mix's, are handed from it.
    let kept_session = state.session.current();
    // A local mix marked and not written yet is read with the session (it stays marked, and is
    // written later).
    let saved_local = kept_local(&state.session, &state.local_saver);
    // So is a processing change marked and not written yet.
    let saved_processing = processing::kept_processing(&state.session, &state.processing_saver);
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
            hand_session_layout(&mut handle, &state.session, saved_groups, saved_channels);
            // After the channel layout, so a linked pair takes its first side's strip.
            if let Some(refused) = processing::hand_kept_processing(&handle, &saved_processing) {
                log_warn!("desk", "start_engine: {refused}");
            }
            for (key, controls) in saved_controls {
                if let Err(error) = handle.keep_engineer_controls(&key, controls) {
                    log_warn!(
                        "desk",
                        "start_engine: the saved controls for {key:?} were not applied: {error}"
                    );
                }
            }
            for refused in hand_kept_mirrors(&handle, saved_mirrors) {
                log_warn!("desk", "start_engine: {refused}");
            }
            if let Err(error) = handle.replace_playback_buffers(&saved_buffers) {
                log_warn!(
                    "desk",
                    "start_engine: the saved playback buffers were not applied: {error}"
                );
            }
            if let Err(error) = handle.replace_redundancies(&saved_redundancies) {
                log_warn!(
                    "desk",
                    "start_engine: the saved redundancy settings were not applied: {error}"
                );
            }
            for refused in limiter::hand_kept_limiters(&handle, &kept_session) {
                log_warn!("desk", "start_engine: {refused}");
            }
            for refused in effects::hand_kept_effects(&handle, &kept_session) {
                log_warn!("desk", "start_engine: {refused}");
            }
            let local = saved_local.map(|kept| apply_kept_local(&mut handle, &kept));
            let notice = local.and_then(|applied| {
                if let Some(refused) = applied.refused {
                    log_warn!("desk", "start_engine: {refused}");
                }
                applied.notice
            });
            if let Some(notice) = &notice {
                log_warn!("desk", "start_engine: {notice}");
            }
            state.set_local_notice(notice);
            let summary = StartSummary {
                run,
                device_name: handle.device_name().to_owned(),
                capture_format: handle.capture_format().into(),
            };
            log_info!(
                "desk",
                "start_engine: device=\"{}\" capture_format={}",
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
        None => Err(NO_INPUT_DEVICE.to_owned()),
    }
}

/// Hands a new engine the session's `groups` and channel `details`, as every start does (see
/// [start_engine]); one the engine refuses (a hand-edited session) is logged and the engine runs
/// without it. The new engine runs the session's details, so the session is no longer behind the
/// engine's ([SessionStore::channels_behind]).
fn hand_session_layout<E: GroupEngine + ChannelEngine>(
    engine: &mut E,
    session: &SessionStore,
    groups: Vec<GroupDefinition>,
    details: Vec<ChannelMeta>,
) {
    if !groups.is_empty() {
        let count = groups.len();
        match engine.set_groups(groups) {
            Ok(()) => log_info!("desk", "start_engine: {count} saved groups applied"),
            Err(error) => log_warn!(
                "desk",
                "start_engine: the saved groups were not applied: {error}"
            ),
        }
    }
    session.set_channels_behind(false);
    if !details.is_empty() {
        let count = details.len();
        match engine.set_channels(details) {
            Ok(()) => log_info!(
                "desk",
                "start_engine: {count} saved channel details applied"
            ),
            Err(error) => log_warn!(
                "desk",
                "start_engine: the saved channel details were not applied: {error}"
            ),
        }
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
    status_dto(
        &handle.status(),
        state.run(),
        &state.session,
        &state.local_notice,
    )
}

/// The window's status of the engine `status`, of engine run `run`: each row named from
/// `session`'s labels, and the local output with what the desk said about the kept one (`notice`,
/// read inside the engine lock like every write of it).
fn status_dto(
    status: &personal_monitoring::EngineStatus,
    run: u64,
    session: &SessionStore,
    notice: &Mutex<Option<String>>,
) -> EngineStatusDto {
    let mut dto = EngineStatusDto::from(status);
    dto.run = run;
    apply_names(&mut dto, session);
    dto.local_output.notice = notice.lock().ok().and_then(|said| said.clone());
    dto
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

/// The engineer's local mix now; `None` when no engine runs. The `mix-changed` event's read for a
/// change of the local mix.
pub fn local_mix(state: &DesktopState) -> Option<MixValues> {
    state
        .engine
        .lock()
        .ok()?
        .as_ref()
        .map(EngineHandle::local_mix)
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
    log_info!("desk", "add_target: {added}");
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
    log_info!("desk", "remove_target: {removed}");
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

/// Sets what the engineer allows one musician beyond their mix: a limit on what they hear and the
/// channels their phone cannot change. Returns what was done ([EngineerControlsSetDto]).
///
/// `address` is the row's address as the status reports it, `client_id` the client id the row
/// carries when the window knows one (an empty one is none), and `controls` is partial (see
/// [EngineerControlsRequest]). Then, as for the groups ([set_groups]):
///
/// - With an engine running, the engine takes them first (the limit caps the musician's output at
///   once, the phone is told, and from its next mix on it cannot change a locked channel, see
///   [EngineHandle::set_engineer_controls]) and only then are they saved: for the musician's phone
///   when it announced a client id (any kept for the address go), else for the address. A save
///   that fails then is reported in the answer, not as an error.
/// - An engine that is stopping, a musician it does not list or a lock list longer than the mix
///   is refused: an error, and nothing changed.
/// - With no engine, they are merged into those kept for the musician and saved under the
///   strongest key the desk knows, the one the next engine resolves first: the phone's when the
///   window gave its client id (any kept for the address go, so none stands in for the edit), else
///   the address, which must be a musician address. They apply at the next start (`applied:
///   false`). The engine locks both sides of a pair when it applies them. A running engine decides
///   the key itself, so `client_id` only matters here.
///
/// Each change is one step under the engine lock, with the session's inside it.
pub fn set_engineer_controls(
    state: &DesktopState,
    address: String,
    client_id: Option<String>,
    controls: EngineerControlsRequest,
) -> Result<EngineerControlsSetDto, String> {
    let outcome = edit_engineer_controls(
        &state.engine,
        &state.session,
        &address,
        client_id.as_deref(),
        controls,
    )?;
    log_info!(
        "desk",
        "set_engineer_controls: {} limit={} locked={:?} kept_for={} applied={} saved={}",
        outcome.address,
        outcome.limit_percent,
        outcome
            .locked_channels
            .iter()
            .enumerate()
            .filter(|(_, &locked)| locked)
            .map(|(channel, _)| channel + 1)
            .collect::<Vec<_>>(),
        outcome.kept_for,
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// What a change of the engineer's controls needs from the running engine, as [GroupEngine] is for
/// a group edit.
trait ControlsEngine {
    /// Applies the change to the musician at `address`; answers the key the engine keeps the
    /// controls under and the mix now stored.
    fn set_engineer_controls(
        &mut self,
        address: &str,
        update: &EngineerUpdate,
    ) -> Result<(MusicianKey, MixValues), String>;
}

impl ControlsEngine for EngineHandle {
    fn set_engineer_controls(
        &mut self,
        address: &str,
        update: &EngineerUpdate,
    ) -> Result<(MusicianKey, MixValues), String> {
        EngineHandle::set_engineer_controls(self, address, update)
            .map_err(|error| error.to_string())
    }
}

/// `update` merged into the controls `kept`, as the engine merges it, for a desk with no engine: the
/// limit clamped to 0-100, each listed lock replacing the channel's from CH 1 on, and a list
/// longer than the mix refused.
fn merge_kept(kept: EngineerControls, update: &EngineerUpdate) -> Result<EngineerControls, String> {
    let mut locked = kept.locked;
    if let Some(list) = &update.locked {
        if list.len() > 32 {
            return Err(format!(
                "locked accepts at most 32 flags, received {}",
                list.len()
            ));
        }
        for (channel, &lock) in list.iter().enumerate() {
            if lock {
                locked |= 1 << channel;
            } else {
                locked &= !(1 << channel);
            }
        }
    }
    Ok(EngineerControls {
        limit_percent: update
            .limit_percent
            .map_or(kept.limit_percent, |limit| limit.clamp(0, 100) as u8),
        locked,
    })
}

/// [set_engineer_controls] over any engine slot and session.
fn edit_engineer_controls<E: ControlsEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    address: &str,
    client_id: Option<&str>,
    request: EngineerControlsRequest,
) -> Result<EngineerControlsSetDto, String> {
    // An empty client id names no phone: what the window gives for a row without one.
    let client_id = phone_id(client_id);
    let update = EngineerUpdate::from(request);
    // One step per change, in the lock order every command that holds both follows (see
    // [edit_groups]).
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let Some(engine) = guard.as_mut() else {
        // Kept under the strongest key the desk knows for the musician, the one the next engine
        // resolves first: the phone's, which also drops those kept for the address, so nothing at
        // start stands in for this edit.
        let at = address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid musician address `{address}`: {error}"))?
            .to_string();
        let kept = session
            .engineer_for(client_id, &at)
            .map(|kept| engineer_controls(&kept))
            .unwrap_or_default();
        let controls = merge_kept(kept, &update)?;
        // Nothing took them, so a save that fails changes nothing and is the answer.
        session.set_engineer(client_id, &at, session_engineer(&controls))?;
        return Ok(EngineerControlsSetDto {
            address: address.to_owned(),
            limit_percent: controls.limit_percent,
            locked_channels: locked_table(&controls, 32),
            kept_for: if client_id.is_some() {
                "phone"
            } else {
                "address"
            },
            applied: false,
            saved: true,
            save_error: None,
            mix: None,
        });
    };
    // A refusal (an engine that is stopping, an unknown musician) returns here, before anything is
    // saved.
    let (key, mix) = engine.set_engineer_controls(address, &update)?;
    let kept = session_engineer(&mix.engineer);
    let (saved, kept_for) = match &key {
        MusicianKey::Client(id) => (session.set_engineer(Some(id), address, kept), "phone"),
        MusicianKey::Address(at) => (session.set_engineer(None, &at.to_string(), kept), "address"),
    };
    // The phone already has the controls: say that they were not kept.
    let save_error = saved.err();
    Ok(EngineerControlsSetDto {
        address: address.to_owned(),
        limit_percent: mix.engineer.limit_percent,
        locked_channels: locked_table(&mix.engineer, mix.channel_gains.len()),
        kept_for,
        applied: true,
        saved: save_error.is_none(),
        save_error,
        mix: Some(MixDto::from(&mix)),
    })
}

/// The longest preset name the desk keeps, in characters.
pub const MAX_PRESET_NAME_LENGTH: usize = 40;

/// What a preset, a copy or a mirror needs from the running engine: the engine itself in the app, a
/// stand-in in the tests, as [GroupEngine] is for a group edit.
trait MixesEngine {
    /// The mix of the musician at `address` as a preset ([EngineHandle::mix_preset]).
    fn mix_preset(&self, address: &str) -> Result<MixPreset, String>;
    /// Puts `preset` on every musician `musicians` names ([EngineHandle::apply_preset]).
    fn apply_preset(
        &mut self,
        preset: &MixPreset,
        musicians: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, String>;
    /// Copies `from`'s mix onto every musician `to` names ([EngineHandle::copy_mix]).
    fn copy_mix(
        &mut self,
        from: &str,
        to: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, String>;
    /// Starts, changes or stops `follower`'s mirror ([EngineHandle::set_mirror]).
    fn set_mirror(
        &mut self,
        follower: &str,
        leader: Option<MirrorSource<'_>>,
    ) -> Result<MirrorOutcome, String>;
}

impl MixesEngine for EngineHandle {
    fn mix_preset(&self, address: &str) -> Result<MixPreset, String> {
        EngineHandle::mix_preset(self, address).map_err(|error| error.to_string())
    }

    fn apply_preset(
        &mut self,
        preset: &MixPreset,
        musicians: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, String> {
        EngineHandle::apply_preset(self, preset, musicians).map_err(|error| error.to_string())
    }

    fn copy_mix(
        &mut self,
        from: &str,
        to: &[String],
    ) -> Result<Vec<(SocketAddr, MixValues)>, String> {
        EngineHandle::copy_mix(self, from, to).map_err(|error| error.to_string())
    }

    fn set_mirror(
        &mut self,
        follower: &str,
        leader: Option<MirrorSource<'_>>,
    ) -> Result<MirrorOutcome, String> {
        EngineHandle::set_mirror(self, follower, leader).map_err(|error| error.to_string())
    }
}

/// The client id the window gave for a musician's phone, or `None` when it gave none or an empty
/// one: an empty id names no phone.
fn phone_id(client_id: Option<&str>) -> Option<&str> {
    client_id.filter(|id| !id.trim().is_empty())
}

/// A musician's mix as a preset keeps it under `id` and `name` (see [crate::session]).
fn kept_preset(id: String, name: String, preset: &MixPreset) -> SessionPreset {
    SessionPreset {
        id,
        name,
        mix: SessionPresetMix {
            channels: preset
                .channels
                .iter()
                .map(|channel| SessionPresetChannel {
                    gain: channel.gain,
                    pan: channel.pan,
                    muted: channel.muted,
                    solo: channel.solo,
                })
                .collect(),
            groups: preset.groups.iter().cloned().collect(),
        },
    }
}

/// A kept preset's mix as the engine takes it.
fn engine_preset(mix: &SessionPresetMix) -> MixPreset {
    MixPreset {
        channels: mix
            .channels
            .iter()
            .map(|channel| PresetChannel {
                gain: channel.gain,
                pan: channel.pan,
                muted: channel.muted,
                solo: channel.solo,
            })
            .collect(),
        groups: mix
            .groups
            .iter()
            .map(|(id, level)| (id.clone(), *level))
            .collect(),
    }
}

/// `name` trimmed, when it can name a preset among `presets` (the one with the id `renaming`
/// aside): not empty, at most [MAX_PRESET_NAME_LENGTH] characters, and no other preset's name in
/// any case, so the engineer never has two presets to tell apart by id alone.
fn preset_name(
    name: &str,
    presets: &[SessionPreset],
    renaming: Option<&str>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a preset needs a name".to_owned());
    }
    if name.chars().count() > MAX_PRESET_NAME_LENGTH {
        return Err(format!(
            "a preset name is at most {MAX_PRESET_NAME_LENGTH} characters"
        ));
    }
    let taken = presets.iter().any(|preset| {
        Some(preset.id.as_str()) != renaming && preset.name.to_lowercase() == name.to_lowercase()
    });
    if taken {
        return Err(format!("a preset named \"{name}\" already exists"));
    }
    Ok(name.to_owned())
}

/// The position of the preset with the id `id` among `presets`, or the error naming it.
fn preset_position(presets: &[SessionPreset], id: &str) -> Result<usize, String> {
    presets
        .iter()
        .position(|preset| preset.id == id)
        .ok_or_else(|| format!("there is no preset with the id `{id}`"))
}

/// The engineer's mix presets the session keeps, in the order they were saved.
pub fn presets(state: &DesktopState) -> Vec<SessionPreset> {
    state.session.presets()
}

/// Saves the mix of the musician at `address` as a new preset called `name`, and answers every
/// preset now kept ([PresetsDto]).
///
/// The running engine gives the mix ([EngineHandle::mix_preset]): each input's level, pan, mute and
/// solo and the group levels by group id, never the master volume, the phone's level cap or the
/// engineer's controls. The preset gets a new id (`p` and 16 hex digits) and is kept after the
/// others. The name is trimmed and must be new (see [preset_name]). With no engine running there is
/// no mix to save, so it is refused; a refusal, or a save that fails, changes nothing.
pub fn save_preset(
    state: &DesktopState,
    address: String,
    name: String,
) -> Result<PresetsDto, String> {
    let outcome = save_preset_with(&state.engine, &state.session, &address, &name)?;
    if let Some(preset) = &outcome.preset {
        log_info!(
            "desk",
            "save_preset: \"{}\" ({}) from {address}",
            preset.name,
            preset.id
        );
    }
    Ok(outcome)
}

/// [save_preset] over any engine slot and session.
fn save_preset_with<E: MixesEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    address: &str,
    name: &str,
) -> Result<PresetsDto, String> {
    // The lock order every command that holds both follows: the engine's, then the session's.
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard.as_ref().ok_or_else(|| {
        "the engine is not running: a preset is saved from a mix the engine plays".to_owned()
    })?;
    let mut presets = session.presets();
    let name = preset_name(name, &presets, None)?;
    let mix = engine.mix_preset(address)?;
    let mut taken: HashSet<String> = presets.iter().map(|preset| preset.id.clone()).collect();
    let preset = kept_preset(new_id('p', &mut taken), name, &mix);
    presets.push(preset.clone());
    session.set_presets(presets.clone())?;
    Ok(PresetsDto {
        preset: Some(preset),
        presets,
    })
}

/// Renames the preset with the id `id` (the name is checked as [save_preset] checks it), with or
/// without an engine, and answers every preset now kept. A refusal, or a save that fails, changes
/// nothing.
pub fn rename_preset(state: &DesktopState, id: String, name: String) -> Result<PresetsDto, String> {
    let outcome = rename_kept_preset(&state.session, &id, &name)?;
    log_info!("desk", "rename_preset: {id} -> \"{}\"", name.trim());
    Ok(outcome)
}

/// [rename_preset] over a session.
fn rename_kept_preset(session: &SessionStore, id: &str, name: &str) -> Result<PresetsDto, String> {
    let mut presets = session.presets();
    let position = preset_position(&presets, id)?;
    presets[position].name = preset_name(name, &presets, Some(id))?;
    session.set_presets(presets.clone())?;
    Ok(PresetsDto {
        preset: Some(presets[position].clone()),
        presets,
    })
}

/// Deletes the preset with the id `id`, with or without an engine, and answers every preset now
/// kept. The window asks the engineer to confirm first. A preset that is not there, or a save that
/// fails, changes nothing.
pub fn delete_preset(state: &DesktopState, id: String) -> Result<PresetsDto, String> {
    let outcome = delete_kept_preset(&state.session, &id)?;
    log_info!("desk", "delete_preset: {id}");
    Ok(outcome)
}

/// [delete_preset] over a session.
fn delete_kept_preset(session: &SessionStore, id: &str) -> Result<PresetsDto, String> {
    let mut presets = session.presets();
    presets.remove(preset_position(&presets, id)?);
    session.set_presets(presets.clone())?;
    Ok(PresetsDto {
        preset: None,
        presets,
    })
}

/// Puts the preset with the id `id` on the mix of every musician `musicians` names (addresses as
/// the status rows show them), through the running engine ([EngineHandle::apply_preset]): each
/// kept input and every live group the preset names by id, locked channels included, pairs kept
/// equal; never the master volume, the phone's level cap or the engineer's controls. Answers each
/// musician's mix now stored ([MixesSetDto]). With no engine running, for a preset that is not
/// there, or for a musician the engine refuses (one whose mix follows another's), it is an error
/// and nobody's mix changed. Nothing is saved: the mixes live in the engine.
pub fn apply_preset(
    state: &DesktopState,
    id: String,
    musicians: Vec<String>,
) -> Result<MixesSetDto, String> {
    let outcome = apply_preset_with(&state.engine, &state.session, &id, &musicians)?;
    log_info!("desk", "apply_preset: {id} on {musicians:?}");
    Ok(outcome)
}

/// [apply_preset] over any engine slot and session.
fn apply_preset_with<E: MixesEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    id: &str,
    musicians: &[String],
) -> Result<MixesSetDto, String> {
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard.as_mut().ok_or_else(|| {
        "the engine is not running: a preset is put on mixes the engine plays".to_owned()
    })?;
    let presets = session.presets();
    let kept = &presets[preset_position(&presets, id)?];
    let applied = engine.apply_preset(&engine_preset(&kept.mix), musicians)?;
    Ok(MixesSetDto::new(&applied))
}

/// Copies the mix of the musician at `from` onto every musician `to` names, through the running
/// engine ([EngineHandle::copy_mix]): every channel's level, pan, mute and solo and every group
/// level, never the master volume, the phone's level cap or the engineer's controls. Answers each
/// musician's mix now stored. With no engine running, or for a musician the engine refuses, it is
/// an error and nobody's mix changed.
pub fn copy_mix(
    state: &DesktopState,
    from: String,
    to: Vec<String>,
) -> Result<MixesSetDto, String> {
    let outcome = copy_mix_with(&state.engine, &from, &to)?;
    log_info!("desk", "copy_mix: {from} onto {to:?}");
    Ok(outcome)
}

/// [copy_mix] over any engine slot.
fn copy_mix_with<E: MixesEngine>(
    engine: &Mutex<Option<E>>,
    from: &str,
    to: &[String],
) -> Result<MixesSetDto, String> {
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard.as_mut().ok_or_else(|| {
        "the engine is not running: a mix is copied between mixes the engine plays".to_owned()
    })?;
    Ok(MixesSetDto::new(&engine.copy_mix(from, to)?))
}

/// Makes the mix of the musician at `address` follow the mix of `leader`, or stops it with `None`
/// (see [EngineHandle::set_mirror]), and answers what was done ([MirrorSetDto]). `client_id` is the
/// follower's phone's client id when the window knows one (an empty one is none).
///
/// - With an engine running, the engine takes the mirror first (the follower's mix takes the
///   leader's at once and follows it, and their phone is held off its channels and groups and told
///   whom it follows), and only then is it saved: under the follower's phone's client id when it
///   announced one, else their address, with the leader's key as the engine keeps it. A save that
///   fails then is reported in the answer, not as an error.
/// - An engine that is stopping, a musician it does not list, a musician following their own mix or
///   a mirror that would go round in a circle is refused: an error, and nothing changed.
/// - With no engine, it is kept under the strongest key the desk knows for the follower (their
///   phone's when the window gave its client id, else the address, which must be a musician
///   address) and the leader's (likewise), refusing a mirror on oneself or one that would close a
///   circle with those kept ([closes_circle]: every musician on the way known by all the keys the
///   desk has for them, compared as the engine compares them, phones by client id and addresses by
///   IP); it applies at the next start (`applied: false`).
///
/// Either way a phone's key is kept with the address the window gave beside it (the leader's and the
/// follower's), so a later check with no engine knows both keys of that musician.
///
/// Each change is one step under the engine lock, with the session's inside it.
pub fn set_mirror(
    state: &DesktopState,
    address: String,
    client_id: Option<String>,
    leader: Option<MirrorRequest>,
) -> Result<MirrorSetDto, String> {
    let outcome = edit_mirror(
        &state.engine,
        &state.session,
        &address,
        client_id.as_deref(),
        leader,
    )?;
    log_info!(
        "desk",
        "set_mirror: {} follows {:?} kept_for={} applied={} saved={}",
        outcome.address,
        outcome.leader,
        outcome.kept_for,
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// Whether two keys name one musician as the engine's book does: two phones by their client id,
/// two addresses by their IP (the engine keeps an address by its IP and lists one musician per IP).
/// A phone and an address are told apart.
fn same_musician(one: &MusicianKey, other: &MusicianKey) -> bool {
    match (one, other) {
        (MusicianKey::Client(one), MusicianKey::Client(other)) => one == other,
        (MusicianKey::Address(one), MusicianKey::Address(other)) => one.ip() == other.ip(),
        _ => false,
    }
}

/// The keys the window gave for one musician, the strongest first: their phone's when it gave a
/// client id, then their address's.
fn keys_of(address: SocketAddr, client_id: Option<&str>) -> Vec<MusicianKey> {
    phone_id(client_id)
        .map(|id| MusicianKey::Client(id.to_owned()))
        .into_iter()
        .chain([MusicianKey::Address(address)])
        .collect()
}

/// The keys a kept mirror knows for one musician: the `key` it names them by, and the `address`
/// kept beside a phone's key. `None` when the key names no phone or address.
fn kept_keys(key: &str, address: Option<&str>) -> Option<Vec<MusicianKey>> {
    let address = address
        .and_then(|address| address.parse().ok())
        .map(MusicianKey::Address);
    Some(
        std::iter::once(musician_key_of(key)?)
            .chain(address)
            .collect(),
    )
}

/// Adds to `known` every key of each of `groups` (keys that name one musician) that shares a key
/// with it as the engine's book compares them ([same_musician]), until no group adds more.
fn widen(known: &mut Vec<MusicianKey>, groups: &[&[MusicianKey]]) {
    loop {
        let before = known.len();
        for group in groups {
            if group
                .iter()
                .any(|key| known.iter().any(|own| same_musician(key, own)))
            {
                for key in group.iter() {
                    if !known.contains(key) {
                        known.push(key.clone());
                    }
                }
            }
        }
        if known.len() == before {
            return;
        }
    }
}

/// Whether following the session's `mirrors` from the musician known by `leader`'s keys reaches the
/// musician known by `follower`'s keys: then the follower following that leader would close a
/// circle.
///
/// At every step the musician is known by all the keys the desk has for them: the keys the window
/// gave for the follower and the leader, and each kept mirror's pairs (the key it is kept under with
/// the follower's address kept beside a phone's key, and the leader's key with theirs), a pair
/// joining them when one of its keys names them as the engine's book does ([same_musician]: phones
/// by client id, addresses by IP). The mirror kept for the musician is then looked up by every one
/// of those keys, phones first, as a registration resolves it, so a mirror kept under either key of
/// a musician is followed. A chain longer than the mirrors can only go round already.
fn closes_circle(
    mirrors: &[(String, SessionMirror)],
    follower: &[MusicianKey],
    leader: &[MusicianKey],
) -> bool {
    let kept: Vec<(Vec<MusicianKey>, Vec<MusicianKey>)> = mirrors
        .iter()
        .filter_map(|(key, mirror)| {
            Some((
                kept_keys(key, mirror.follower_address.as_deref())?,
                kept_keys(&mirror.leader, mirror.leader_address.as_deref())?,
            ))
        })
        .collect();
    let groups: Vec<&[MusicianKey]> = [follower, leader]
        .into_iter()
        .chain(
            kept.iter()
                .flat_map(|(follows, leads)| [follows.as_slice(), leads.as_slice()]),
        )
        .collect();
    let mut known = leader.to_vec();
    for _ in 0..=kept.len() {
        widen(&mut known, &groups);
        if known
            .iter()
            .any(|key| follower.iter().any(|own| same_musician(key, own)))
        {
            return true;
        }
        known.sort_by_key(|key| matches!(key, MusicianKey::Address(_)));
        let next = known.iter().find_map(|key| {
            kept.iter()
                .find(|(follows, _)| same_musician(&follows[0], key))
                .map(|(_, leads)| leads.clone())
        });
        match next {
            Some(leads) => known = leads,
            None => return false,
        }
    }
    true
}

/// [set_mirror] over any engine slot and session.
fn edit_mirror<E: MixesEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    address: &str,
    client_id: Option<&str>,
    leader: Option<MirrorRequest>,
) -> Result<MirrorSetDto, String> {
    let client_id = phone_id(client_id);
    // One step per change, in the lock order every command that holds both follows (see
    // [edit_groups]).
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let Some(engine) = guard.as_mut() else {
        let at = address
            .parse::<SocketAddr>()
            .map_err(|error| format!("invalid musician address `{address}`: {error}"))?;
        let follower = keys_of(at, client_id);
        let kept = match &leader {
            None => None,
            Some(request) => {
                let leader_at = request.address.parse::<SocketAddr>().map_err(|error| {
                    format!("invalid musician address `{}`: {error}", request.address)
                })?;
                let leading = keys_of(leader_at, request.client_id.as_deref());
                // Kept under the leader's strongest key, as the engine keeps it.
                let leader_key = session_key(&leading[0]);
                if leading
                    .iter()
                    .any(|key| follower.iter().any(|own| same_musician(key, own)))
                {
                    return Err(format!("{address} cannot follow their own mix"));
                }
                if closes_circle(&session.mirrors(), &follower, &leading) {
                    return Err(format!(
                        "{} already follows {address}'s mix, directly or through another musician: a mirror cannot go round in a circle",
                        request.address
                    ));
                }
                let name = request.name.trim();
                Some(SessionMirror {
                    leader: leader_key,
                    name: if name.is_empty() {
                        leader_at.ip().to_string()
                    } else {
                        name.to_owned()
                    },
                    leader_address: matches!(leading[0], MusicianKey::Client(_))
                        .then(|| leader_at.to_string()),
                    follower_address: client_id.map(|_| at.to_string()),
                })
            }
        };
        // Nothing took it, so a save that fails changes nothing and is the answer.
        session.set_mirror(client_id, &at.to_string(), kept.clone())?;
        return Ok(MirrorSetDto {
            address: address.to_owned(),
            leader: leader.map(|request| request.address),
            name: kept.map(|mirror| mirror.name),
            kept_for: if client_id.is_some() {
                "phone"
            } else {
                "address"
            },
            applied: false,
            saved: true,
            save_error: None,
            mix: None,
        });
    };
    // A refusal (an engine that is stopping, an unknown musician, a circle) returns here, before
    // anything is saved.
    let outcome = engine.set_mirror(
        address,
        leader.as_ref().map(|request| MirrorSource {
            address: &request.address,
            name: &request.name,
        }),
    )?;
    // The address the session keys an address entry by, written as the status writes it, so the
    // entry the phone's replaces is found however the window wrote the address.
    let at = address
        .parse::<SocketAddr>()
        .map_or_else(|_| address.to_owned(), |at| at.to_string());
    let leader_at = leader
        .as_ref()
        .and_then(|request| request.address.parse::<SocketAddr>().ok());
    let kept = outcome.leader.as_ref().map(|key| SessionMirror {
        leader: session_key(key),
        name: outcome.name.clone().unwrap_or_default(),
        leader_address: match key {
            MusicianKey::Client(_) => leader_at.map(|at| at.to_string()),
            MusicianKey::Address(_) => None,
        },
        follower_address: matches!(outcome.follower, MusicianKey::Client(_)).then(|| at.clone()),
    });
    let (saved, kept_for) = match &outcome.follower {
        MusicianKey::Client(id) => (session.set_mirror(Some(id), &at, kept), "phone"),
        MusicianKey::Address(at) => (session.set_mirror(None, &at.to_string(), kept), "address"),
    };
    // The phone already follows: say that it was not kept.
    let save_error = saved.err();
    Ok(MirrorSetDto {
        address: address.to_owned(),
        leader: leader.map(|request| request.address),
        name: outcome.name,
        kept_for,
        applied: true,
        saved: save_error.is_none(),
        save_error,
        mix: Some(MixDto::from(&outcome.mix)),
    })
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
    log_info!(
        "desk",
        "set_musician_name: {address} ({kept_for}) -> \"{name}\""
    );
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
    log_info!(
        "desk",
        "set_groups: {} groups, applied={} saved={}",
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
/// different details. A kind this build does not know is taken for an input only when the session
/// keeps exactly that kind for it (a newer build wrote it, and [channel_table] read it back as
/// written): it is kept as written and applies as `other`. An explicit `other` replaces it.
pub fn set_channels(
    state: &DesktopState,
    channels: Vec<ChannelRequest>,
) -> Result<ChannelsSetDto, String> {
    let outcome = edit_channels(&state.engine, &state.session, channels)?;
    log_info!(
        "desk",
        "set_channels: {} channels, applied={} saved={}",
        outcome.channels.len(),
        outcome.applied,
        outcome.saved
    );
    Ok(outcome)
}

/// The engineer's whole channel table, as the window edits it: every entry kept, in channel order
/// (the first is CH 1), details of inputs the current device lacks included. The engine status
/// lists only the device's inputs, so a window that edited from it would send a shorter list as
/// the whole table and lose the rest; it reads this instead and sends it back with its change.
///
/// With an engine running this is the engine's table ([EngineHandle::channel_table]), which is
/// what it took last even when saving it failed; with none, the session's. A kind this build does
/// not know, kept in the session for an input (a newer build wrote it; it applies as `other`), is
/// read as written, so the window sends it back untouched with an edit of anything else and only
/// an explicit choice replaces it (see [set_channels]). With an engine running that holds only
/// while the session holds what the engine took last ([SessionStore::channels_behind]): after a
/// save that failed, the engine's `other` is read, so a kind it replaced never comes back.
pub fn channel_table(state: &DesktopState) -> Result<Vec<SessionChannel>, String> {
    read_channel_table(&state.engine, &state.session)
}

/// [channel_table] over any engine slot and session.
fn read_channel_table<E: ChannelEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
) -> Result<Vec<SessionChannel>, String> {
    // The lock order every command that holds both follows: the engine's, then the session's.
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let stored = session.channels();
    let Some(engine) = guard.as_ref() else {
        return Ok(stored);
    };
    // The session's kinds stand in for the engine's `other` only while the session holds what the
    // engine took last: after a save that failed, the engine's are the newer. Accepted: while
    // behind, every input reads as the engine applies it, also one no one edited, since an
    // untouched unknown kind and an explicit `other` look alike to the engine.
    let stored = if session.channels_behind() {
        Vec::new()
    } else {
        stored
    };
    Ok(engine
        .channel_table()
        .into_iter()
        .enumerate()
        .map(|(index, channel)| SessionChannel {
            kind: stored
                .get(index)
                .filter(|kept| {
                    channel.kind == ChannelKind::Other
                        && ChannelKind::from_name(&kept.kind).is_none()
                })
                .map_or_else(|| channel.kind.name().to_owned(), |kept| kept.kind.clone()),
            name: channel.name,
            hidden: channel.hidden,
            link: channel.linked_to_next,
        })
        .collect())
}

/// What a channel edit needs from the running engine, as [GroupEngine] is for a group edit.
trait ChannelEngine {
    fn set_channels(&mut self, channels: Vec<ChannelMeta>) -> Result<(), String>;
    /// The whole table the engine keeps (see [channel_table]).
    fn channel_table(&self) -> Vec<ChannelMeta>;
}

impl ChannelEngine for EngineHandle {
    fn set_channels(&mut self, channels: Vec<ChannelMeta>) -> Result<(), String> {
        EngineHandle::set_channels(self, channels)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn channel_table(&self) -> Vec<ChannelMeta> {
        EngineHandle::channel_table(self)
    }
}

/// The window's channel details as the engine takes them and as the session keeps them, names
/// trimmed. A kind the engine does not know is an error naming it, unless `stored` (the session's
/// details) keeps exactly that kind for that input: a newer build's kind, sent back untouched, is
/// kept as written and applies as `other`.
fn channel_details(
    requests: Vec<ChannelRequest>,
    stored: &[SessionChannel],
) -> Result<(Vec<ChannelMeta>, Vec<SessionChannel>), String> {
    let mut details = Vec::with_capacity(requests.len());
    let mut kept = Vec::with_capacity(requests.len());
    for (index, request) in requests.into_iter().enumerate() {
        let untouched = stored
            .get(index)
            .is_some_and(|previous| previous.kind == request.kind);
        let (kind, written) = match ChannelKind::from_name(&request.kind) {
            Some(kind) => (kind, kind.name().to_owned()),
            None if untouched => (ChannelKind::Other, request.kind.clone()),
            None => {
                let known: Vec<&str> = ChannelKind::ALL.iter().map(|kind| kind.name()).collect();
                return Err(format!(
                    "CH {} has the instrument kind `{}`, which is none of {}",
                    index + 1,
                    request.kind,
                    known.join(", ")
                ));
            }
        };
        let name = request.name.trim().to_owned();
        details.push(ChannelMeta {
            name: name.clone(),
            kind,
            hidden: request.hidden,
            linked_to_next: request.link,
        });
        kept.push(SessionChannel {
            name,
            kind: written,
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
    // One step per edit, in the lock order every command that holds both follows (see
    // [edit_groups]); the details are checked against the session's under it.
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    // A kind the session keeps stands for an untouched input only while the session holds what a
    // running engine took last (see [read_channel_table]).
    let trusted = if guard.is_some() && session.channels_behind() {
        Vec::new()
    } else {
        session.channels()
    };
    let (details, kept) = channel_details(channels, &trusted)?;
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
        // The phones already have the details: say that they were not kept, and from now on the
        // engine's are the newer.
        Err(error) if applied => {
            session.set_channels_behind(true);
            Some(error)
        }
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

/// Stops the engine's event pump, then the engine, and joins their threads; then, with the engine
/// lock released, writes the local mix and the channel processing still waiting to be kept (see
/// [local::saver]) and hands a
/// backup of the musicians' mixes as the engine left them to its writer (see [backups]). Stopping
/// when idle is a no-op.
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
    // The musicians' mixes as the engine leaves them, backed up once the lock is released.
    let desk = guard.as_ref().map(DeskReader::desk);
    let stopped = match guard.take() {
        Some(mut handle) => handle.stop().map_err(|error| error.to_string()),
        None => Ok(()),
    };
    drop(guard);
    state.local_saver.flush();
    state.processing_saver.flush();
    if let Some(desk) = &desk {
        backups::back_up_desk(state, desk);
    }
    stopped
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
    log_info!(
        "desk",
        "switch_device: device=\"{}\" capture_format={}",
        handle.device_name(),
        format.describe()
    );
    Ok(StartSummary {
        run: state.run(),
        device_name: handle.device_name().to_owned(),
        capture_format: format,
    })
}

/// The audio input the desk last ran with, as the window starts every engine from it (see
/// [crate::session::SessionAudio]): `None` until one was kept. It is read as kept; the window
/// treats a choice that names ASIO without a driver as no ASIO.
pub fn audio_settings(state: &DesktopState) -> Option<SessionAudio> {
    state.session.audio()
}

/// Keeps the audio input the running engine runs with for every later start, and answers it as
/// kept. The window calls it only once an engine started or switched with the choice, never for the
/// system's default input it fell back to when the kept input was missing, so a missing interface
/// never replaces the engineer's choice.
///
/// The driver must be `auto`, `wasapi`, `asio` or `demo` (kept by its own lowercase name; `demo` is
/// the demo source, kept like any driver with its one device), and `asio` needs
/// the ASIO driver the engineer picked: ASIO is never kept without one. Names are trimmed and an
/// empty one is none. With no engine running nothing is kept: only settings an engine runs with
/// are. A refused choice, or a save that fails, changes nothing.
pub fn set_audio_settings(
    state: &DesktopState,
    settings: AudioSettingsRequest,
) -> Result<SessionAudio, String> {
    let kept = keep_audio_settings(&state.engine, &state.session, settings)?;
    log_info!(
        "desk",
        "set_audio_settings: host={} device={:?} buffer={:?} asio_driver={:?}",
        kept.host,
        kept.device,
        kept.buffer_frames,
        kept.asio_driver
    );
    Ok(kept)
}

/// [set_audio_settings] over any engine slot and session.
fn keep_audio_settings<E>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    settings: AudioSettingsRequest,
) -> Result<SessionAudio, String> {
    let kept = session_audio(settings)?;
    // The lock order every command that holds both follows: the engine's, then the session's.
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    if guard.is_none() {
        return Err(
            "the engine is not running: only the audio input an engine runs with is kept"
                .to_owned(),
        );
    }
    session.set_audio(kept.clone())?;
    Ok(kept)
}

/// The window's audio input as the session keeps it, or why it cannot be kept.
fn session_audio(settings: AudioSettingsRequest) -> Result<SessionAudio, String> {
    let named = |name: Option<String>| {
        name.map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
    };
    let host = AudioHost::parse(settings.host.trim()).map_err(|_| {
        format!(
            "the audio driver `{}` is none of auto, wasapi, asio, demo",
            settings.host
        )
    })?;
    let asio_driver = named(settings.asio_driver);
    if host == AudioHost::Asio && asio_driver.is_none() {
        return Err(
            "ASIO is kept only with the ASIO driver the engineer picked, and none is named"
                .to_owned(),
        );
    }
    Ok(SessionAudio {
        host: host.as_str().to_owned(),
        device: named(settings.device),
        buffer_frames: settings.buffer_frames,
        asio_driver,
    })
}

/// What went wrong or changed when the desk opened its session (a damaged section set aside, a
/// damaged file moved aside, a session from a newer BandEar left alone, labels imported), for the
/// window to say; `None` when it simply loaded (see [SessionStore::notice]).
pub fn session_notice(state: &DesktopState) -> Option<String> {
    state.session.notice()
}

/// The engineer's saved sessions, the newest first, each with why it cannot be loaded when it
/// cannot (see [crate::sessions]).
pub fn sessions(state: &DesktopState) -> Result<Vec<SnapshotSummary>, String> {
    state.snapshots.list()
}

/// What a session save or load reads of the running engine: the device's input count, the ids of
/// the live groups in their order (every musician's group levels are placed by them), every
/// musician the engine lists, with their mix, their phone's client id and whom they follow, the
/// engineer's local mix and every channel's strip (CH 1 first).
struct DeskView {
    inputs: usize,
    groups: Vec<String>,
    musicians: Vec<MusicianStatus>,
    local: MixValues,
    processing: Vec<StripSettings>,
}

/// Reads the running engine for a session save or load: the engine in the app, a stand-in in the
/// tests.
trait DeskReader {
    fn desk(&self) -> DeskView;
}

impl DeskReader for EngineHandle {
    fn desk(&self) -> DeskView {
        let status = self.status();
        DeskView {
            inputs: usize::from(status.source_channels),
            groups: status.groups.into_iter().map(|group| group.id).collect(),
            musicians: status.musicians,
            local: status.local_mix,
            processing: EngineHandle::processing(self),
        }
    }
}

/// What loading a session needs from the running engine, besides the group, channel, local mix and
/// processing edits every other command uses.
trait SessionEngine:
    DeskReader
    + GroupEngine
    + ChannelEngine
    + LocalEngine
    + ProcessingEngine
    + limiter::LimiterEngine
    + effects::EffectsEngine
{
    /// Replaces every control and mirror the engine keeps ([EngineHandle::replace_kept]); answers
    /// the mirrors it refused.
    fn replace_kept(
        &self,
        controls: &[(MusicianKey, EngineerControls)],
        mirrors: &[(MusicianKey, MusicianKey, String)],
    ) -> Result<Vec<String>, String>;
    /// Replaces every playback buffer the engine keeps ([EngineHandle::replace_playback_buffers]).
    fn replace_playback_buffers(
        &self,
        kept: &[(MusicianKey, PlaybackBuffer)],
    ) -> Result<(), String>;
    /// Replaces every redundancy setting the engine keeps ([EngineHandle::replace_redundancies]).
    fn replace_redundancies(&self, kept: &[(MusicianKey, Redundancy)]) -> Result<(), String>;
    /// Changes the mix of the musician at `address` from the desk ([EngineHandle::set_mix]).
    fn push_mix(&mut self, address: &str, update: &MixUpdate) -> Result<MixValues, String>;
}

impl SessionEngine for EngineHandle {
    fn replace_kept(
        &self,
        controls: &[(MusicianKey, EngineerControls)],
        mirrors: &[(MusicianKey, MusicianKey, String)],
    ) -> Result<Vec<String>, String> {
        EngineHandle::replace_kept(self, controls, mirrors).map_err(|error| error.to_string())
    }

    fn replace_playback_buffers(
        &self,
        kept: &[(MusicianKey, PlaybackBuffer)],
    ) -> Result<(), String> {
        EngineHandle::replace_playback_buffers(self, kept).map_err(|error| error.to_string())
    }

    fn replace_redundancies(&self, kept: &[(MusicianKey, Redundancy)]) -> Result<(), String> {
        EngineHandle::replace_redundancies(self, kept).map_err(|error| error.to_string())
    }

    fn push_mix(&mut self, address: &str, update: &MixUpdate) -> Result<MixValues, String> {
        self.set_mix(address, update)
            .map_err(|error| error.to_string())
    }
}

/// The key the desk keeps what it knows about the musician on `row` under: their phone's client id
/// when it announced one, else their address as the status writes it.
fn row_key(row: &MusicianStatus) -> String {
    match phone_id(row.client_id.as_deref()) {
        Some(id) => client_key(id),
        None => address_key(&row.address.to_string()),
    }
}

/// Every musician's mix as a named session keeps it (see [crate::sessions::SessionMix]), under the
/// key the desk keeps the musician under: the level, pan, mute and solo of the device's inputs,
/// the group levels by group id, the master volume and the master mute. Never the phone's own
/// level cap.
fn saved_mixes(desk: &DeskView) -> BTreeMap<String, SessionMix> {
    desk.musicians
        .iter()
        .map(|row| (row_key(row), session_mix_of(&row.mix, desk)))
        .collect()
}

/// `mix` as the desk keeps a whole mix ([SessionMix]): the level, pan, mute and solo of the inputs
/// the device has, the group levels by the ids of the live groups, the master volume and the master
/// mute. Never the phone's own level cap or the engineer's controls.
fn session_mix_of(mix: &MixValues, desk: &DeskView) -> SessionMix {
    let inputs = desk.inputs.min(mix.channel_gains.len());
    SessionMix {
        channels: (0..inputs)
            .map(|channel| SessionPresetChannel {
                gain: mix.channel_gains[channel],
                pan: mix.pans[channel],
                muted: mix.channel_muted[channel],
                solo: mix.channel_solo[channel],
            })
            .collect(),
        groups: desk
            .groups
            .iter()
            .zip(mix.group_levels)
            .map(|(id, level)| (id.clone(), level))
            .collect(),
        volume_percent: mix.volume_percent,
        muted: mix.muted,
    }
}

/// Saves the desk's whole session as a named session called `name` (trimmed, at most 40
/// characters, no other saved session's name in any case), with every musician's mix when an
/// engine runs, and answers what was saved ([SessionSavedDto]).
///
/// The session is everything the desk keeps (labels, the engineer's controls and mirrors, channel
/// details, groups, presets, the audio input, the local mix and its output, and each input's
/// channel processing), but how this desk records and its strip presets; each musician's mix is
/// read from the running engine under the key the desk keeps them by (see [saved_mixes]), and so
/// are the local mix and the processing the engine plays. With no engine running the session is saved without
/// mixes, with the local mix as the desk keeps it. A refused name, or a save that fails, saves
/// nothing.
pub fn save_session(state: &DesktopState, name: String) -> Result<SessionSavedDto, String> {
    let outcome = save_session_with(&state.engine, &state.session, &state.snapshots, &name)?;
    log_info!(
        "desk",
        "save_session: \"{}\" ({}) with {} mixes",
        outcome.session.name,
        outcome.session.id,
        outcome.session.mixes
    );
    Ok(outcome)
}

/// [save_session] over any engine slot, session and folder of named sessions.
fn save_session_with<E: DeskReader>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    snapshots: &SnapshotStore,
    name: &str,
) -> Result<SessionSavedDto, String> {
    // The mixes and the session are read under the engine lock (the session's inside it), so no
    // edit lands between them.
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let desk = guard.as_ref().map(DeskReader::desk);
    let mixes = desk.as_ref().map(saved_mixes).unwrap_or_default();
    let mut current = session.current();
    // How this desk records, and its own strip presets, belong to the desk, not to a show.
    current.recording = None;
    current.strip_presets = Vec::new();
    if let Some(desk) = &desk {
        // The processing the engine plays: a change not written yet is the engine's already.
        current.processing = processing::session_processing(&desk.processing);
        // The local mix the engine plays: an edit whose save failed, or levels a group or channel
        // change moved, are the engine's, not yet the session's.
        let kept = current.local.take().unwrap_or_default();
        current.local = Some(SessionLocal {
            mix: Some(session_mix_of(&desk.local, desk)),
            ..kept
        });
    }
    let saved = snapshots.save(name, current, mixes, |taken| new_id('s', taken))?;
    Ok(SessionSavedDto {
        session: saved,
        engine_running: desk.is_some(),
    })
}

/// Renames the saved session `id` (the name checked as [save_session] checks it) and answers it as
/// listed. Only its name changes; one that cannot be loaded is not renamed.
pub fn rename_session(
    state: &DesktopState,
    id: String,
    name: String,
) -> Result<SnapshotSummary, String> {
    let renamed = state.snapshots.rename(&id, &name)?;
    log_info!("desk", "rename_session: {id} -> \"{}\"", renamed.name);
    Ok(renamed)
}

/// Deletes the saved session `id`, whether or not it can be read. The window asks the engineer to
/// confirm first.
pub fn delete_session(state: &DesktopState, id: String) -> Result<(), String> {
    state.snapshots.delete(&id)?;
    log_info!("desk", "delete_session: {id}");
    Ok(())
}

/// Loads the saved session `id`: it replaces the desk's session (labels, the engineer's controls
/// and mirrors, channel details, groups, presets and the audio input) and, with an engine running,
/// is applied live. Answers what was applied, saved and skipped ([SessionLoadedDto]).
///
/// - Before anything changes, the saved groups and channel details are checked as the engine
///   checks them, and a session file this build must not replace (one a newer BandEar wrote) is
///   refused: an error, and nothing changed. A section of the saved session that cannot be read is
///   left out (it loads empty) and named in the answer.
/// - With an engine running, in this order: the groups and the channel details, as a start applies
///   them (every phone is sent its `config` before any mix the change moves); then the engineer's
///   controls and the mirrors, so a phone that is held is held before its mix arrives; then each
///   saved mix is pushed through the desk's path to the musician the engine lists under the key it
///   was saved by (their phone's client id first, then their address), so their phone is sent it
///   at once (`mix_state`). Only musicians who are here get it: a phone whose control channel is
///   down is skipped, as is a saved mix with no musician here, because when a phone connects its
///   own mix wins (as it always has); a configured target with no phone gets it, since it plays
///   that mix. A push the engine refuses is named with its reason. A musician who follows
///   another's mix gets only their master volume and mute (their channels and groups follow the
///   leader's). The loaded session's controls and mirrors replace every one the engine keeps
///   ([EngineHandle::replace_kept]): one it no longer keeps is lifted from the musicians who are
///   here and forgotten for those who are not, so it never comes back when they register. Its
///   playback buffers replace the engine's the same way ([EngineHandle::replace_playback_buffers]),
///   so a phone on a fixed buffer the loaded session does not keep goes back to Auto. Its redundancy
///   settings too ([EngineHandle::replace_redundancies]): a musician it does not keep goes back to
///   being sent once. Its channel processing replaces every channel's strip
///   ([EngineHandle::replace_processing]), right after the controls and mirrors: a channel it keeps
///   none for goes back to no processing, and a change still waiting to be kept is dropped. An
///   engine that refuses them keeps playing its own strips, and the session is to keep those
///   instead. A mirror the engine refuses (on oneself, or closing a circle) is named in the
///   answer; the session keeps it, as a start would refuse it again.
/// - With no engine running, the session is replaced and the saved mixes go to nobody (each is
///   named in the answer as not pushed); the rest applies at the next start.
/// - The audio input of the saved session applies only at the next start: the running engine is
///   never restarted. It is taken only as the engineer could choose it (a known driver, and ASIO
///   only with the ASIO driver the engineer picked); one that is not is skipped and said, and the
///   desk keeps its own, as it does when the saved session has none.
/// - The saved local mix and its output replace the desk's (`localLoaded`); a saved session from
///   before the local mix leaves the desk's own as they are. With an engine running, after the
///   musicians' mixes, the local mix is pushed through the desk's path (`localMix`) and the output
///   is put in force as a start does it ([local::apply_kept_local]): none turns the local output
///   off, and one the running driver does not offer, or that cannot be opened, leaves it off and is
///   said (`localOutputNotice`), never replaced by another device.
///
/// The load is one step under the engine lock, with the session's inside it; what the status says
/// about the loaded output is written inside it too, as a start and an output choice write it. A
/// local mix still waiting to be kept is kept with a session that leaves the desk's own local mix,
/// and dropped for one whose saved local mix replaced it. A save that fails after the engine took
/// the session is reported in the answer, not as an error.
pub fn load_session(state: &DesktopState, id: String) -> Result<SessionLoadedDto, String> {
    let engine = &state.engine;
    let outcome = load_session_with(
        engine,
        &state.session,
        &state.snapshots,
        &state.local_saver,
        &state.processing_saver,
        &state.local_notice,
        &id,
    )?;
    log_info!(
        "desk",
        "load_session: \"{}\" ({id}) applied={} saved={} pushed={} skipped={} refused={}",
        outcome.session.name,
        outcome.applied,
        outcome.saved,
        outcome.mixes.len(),
        outcome.skipped.len(),
        outcome.refused.len()
    );
    for line in outcome.damaged.iter().chain(&outcome.refused) {
        log_warn!("desk", "load_session: {line}");
    }
    Ok(outcome)
}

/// The engine's definitions of a session's groups.
fn definitions_of(groups: &[SessionGroup]) -> Vec<GroupDefinition> {
    groups
        .iter()
        .map(|group| GroupDefinition {
            id: group.id.clone(),
            name: group.name.clone(),
            channels: group.channels.clone(),
        })
        .collect()
}

/// The engine's details of a session's channels, a kind this build does not know as
/// [ChannelKind::Other] (see [session_channels]).
fn details_of(channels: &[SessionChannel]) -> Vec<ChannelMeta> {
    channels
        .iter()
        .map(|channel| ChannelMeta {
            name: channel.name.clone(),
            kind: ChannelKind::from_name(&channel.kind).unwrap_or_default(),
            hidden: channel.hidden,
            linked_to_next: channel.link,
        })
        .collect()
}

/// The audio input a loaded session leaves the desk with: the saved session's when the engineer
/// could have chosen it (see [session_audio]: a known driver, and ASIO only with a picked driver),
/// else `current`, with why the saved one was skipped. Answers the audio input, whether it is the
/// saved session's, and the reason it was skipped.
fn loaded_audio(
    saved: Option<SessionAudio>,
    current: Option<SessionAudio>,
) -> (Option<SessionAudio>, bool, Option<String>) {
    let Some(saved) = saved else {
        return (current, false, None);
    };
    let request = AudioSettingsRequest {
        host: saved.host,
        device: saved.device,
        buffer_frames: saved.buffer_frames,
        asio_driver: saved.asio_driver,
    };
    match session_audio(request) {
        Ok(audio) => (Some(audio), true, None),
        Err(why) => (current, false, Some(why)),
    }
}

/// [load_session] over any engine slot, session, folder of named sessions, local mix and
/// processing savers and notice.
fn load_session_with<E: SessionEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    snapshots: &SnapshotStore,
    saver: &LocalMixSaver,
    processing_saver: &ProcessingSaver,
    notice: &Mutex<Option<String>>,
    id: &str,
) -> Result<SessionLoadedDto, String> {
    let SnapshotRead {
        id,
        snapshot,
        damaged,
    } = snapshots.read(id)?;
    let definitions = definitions_of(&snapshot.session.groups);
    check_groups(&definitions)
        .map_err(|error| format!("the saved groups cannot be used: {error}"))?;
    let details = details_of(&snapshot.session.channels);
    check_channels(&details)
        .map_err(|error| format!("the saved channel details cannot be used: {error}"))?;
    let summary = SnapshotSummary {
        id: id.clone(),
        name: snapshot.name.clone(),
        saved_at: snapshot.saved_at,
        mixes: snapshot.mixes.len(),
        problem: None,
    };
    // One step, in the lock order every command that holds both follows (see [edit_groups]).
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    // A session the desk must not replace refuses the load before the engine takes anything.
    session.check_saves()?;
    // No local mix is written while the session is read and replaced: one still waiting is the
    // desk's own, newer than the session's.
    let paused = saver.pause();
    // Nor any processing: the loaded session's replaces the change still waiting.
    let processing_paused = processing_saver.pause();
    let previous = session.current();
    let (audio, audio_loaded, audio_skipped) =
        loaded_audio(snapshot.session.audio.clone(), previous.audio.clone());
    // A saved session from before the local mix leaves the desk's own local mix and output.
    let saved_local = snapshot.session.local.clone();
    let next = Session {
        audio: audio.clone(),
        local: saved_local
            .clone()
            .or_else(|| with_pending(previous.local, paused.pending())),
        // How this desk records is the desk's own, whatever the saved session says: the store keeps
        // it as it holds it when the loaded session is written (see [SessionStore::replace]), so a
        // change saved while this load runs, which does not wait for the engine lock, is kept.
        recording: None,
        ..snapshot.session.clone()
    };
    let live = match guard.as_mut() {
        // A refusal (an engine that is stopping) returns here, before anything is saved.
        Some(engine) => Some(apply_session(
            engine,
            &next,
            definitions,
            details,
            &snapshot.mixes,
            saved_local.as_ref(),
        )?),
        None => None,
    };
    let applied = live.is_some();
    let save_error = match session.replace(next.clone()) {
        Ok(()) => None,
        // The engine already runs the loaded session: say that it was not kept, and from now on
        // the engine's channel details are the newer.
        Err(error) if applied => {
            session.set_channels_behind(true);
            Some(error)
        }
        // Nothing took it, so nothing changed: a local mix waiting is still the desk's.
        Err(error) => return Err(error),
    };
    // The local mix waiting is in the session now, or the engine plays the loaded one instead.
    if save_error.is_none() || saved_local.is_some() {
        paused.clear();
    }
    drop(paused);
    // The session or the engine holds the loaded processing now. When the engine refused it, it
    // still plays its own strips (a change waiting included), so the session is to keep those.
    match live.as_ref().and_then(|live| live.processing_kept.clone()) {
        Some(kept) => processing_saver.mark(kept),
        None => processing_paused.clear(),
    }
    drop(processing_paused);
    let (mixes, skipped, refused, local) = match live {
        Some(live) => (live.pushed, live.skipped, live.refused, live.local),
        None => (
            Vec::new(),
            snapshot
                .mixes
                .keys()
                .map(|key| skipped_mix(&next, key, "notRunning", None))
                .collect(),
            Vec::new(),
            None,
        ),
    };
    let (local_mix, local_output_notice) = match local {
        Some(local) => (local.mix.as_ref().map(MixDto::from), local.notice),
        None => (None, None),
    };
    // What the status says about the loaded output, written inside the engine lock like every
    // other write of it, so a later output choice is never undone by it.
    if applied && saved_local.is_some() {
        if let Ok(mut said) = notice.lock() {
            said.clone_from(&local_output_notice);
        }
    }
    drop(guard);
    Ok(SessionLoadedDto {
        session: summary,
        applied,
        saved: save_error.is_none(),
        save_error,
        mixes,
        skipped,
        refused,
        damaged,
        audio,
        audio_loaded,
        audio_skipped,
        presets: next.presets,
        local_loaded: saved_local.is_some(),
        local_mix,
        local_output_notice,
    })
}

/// What a running engine did with a loaded session.
struct LiveLoad {
    pushed: Vec<LoadedMixDto>,
    skipped: Vec<SkippedMixDto>,
    refused: Vec<String>,
    /// What it did with the saved local section; `None` when the saved session had none.
    local: Option<local::LocalApplied>,
    /// The processing the engine still plays when it refused the loaded one, for the session to
    /// keep instead; `None` when it took the loaded processing.
    processing_kept: Option<crate::session::SessionProcessing>,
}

/// A saved mix that went to nobody, named by the musician's key and the label `session` keeps for
/// them.
fn skipped_mix(
    session: &Session,
    key: &str,
    reason: &'static str,
    error: Option<String>,
) -> SkippedMixDto {
    SkippedMixDto {
        key: key.to_owned(),
        label: session
            .musicians
            .get(key)
            .and_then(|entry| entry.label.clone()),
        reason,
        error,
    }
}

/// Applies the session `next` to a running engine, in the order [load_session] gives: groups and
/// channel details, the engineer's controls and the mirrors, the saved mixes, then the saved
/// `local` mix and output when the saved session has them.
fn apply_session<E: SessionEngine>(
    engine: &mut E,
    next: &Session,
    definitions: Vec<GroupDefinition>,
    details: Vec<ChannelMeta>,
    mixes: &BTreeMap<String, SessionMix>,
    local: Option<&SessionLocal>,
) -> Result<LiveLoad, String> {
    // In the order a start applies them. Both were checked already, so only an engine that is
    // stopping refuses them, and then the first refusal changed nothing.
    engine.set_groups(definitions)?;
    engine.set_channels(details)?;
    // The loaded session's controls and mirrors replace the engine's whole, so one it dropped
    // never comes back, also for a musician who is not here and registers later.
    let mut refused: Vec<String> = match engine.replace_kept(&controls_of(next), &mirrors_of(next))
    {
        Ok(refused) => refused
            .into_iter()
            .map(|error| format!("a saved mirror was not applied: {error}"))
            .collect(),
        Err(error) => vec![format!(
            "the saved limits, locks and mirrors were not applied: {error}"
        )],
    };
    // Every channel's strip, after the channel layout so a linked pair takes its first side's: a
    // channel the loaded session keeps none for goes back to no processing.
    let processing_kept =
        match engine.replace_processing(&processing::processing_list(&next.processing)) {
            Ok(_) => None,
            Err(error) => {
                refused.push(format!(
                    "the saved channel processing was not applied: {error}"
                ));
                Some(processing::session_processing(&engine.processing()))
            }
        };
    // The loaded session's buffers replace the engine's whole too, so a phone set to a fixed
    // buffer the session does not keep goes back to Auto.
    if let Err(error) = engine.replace_playback_buffers(&playback_buffer::buffers_of(next)) {
        refused.push(format!(
            "the saved playback buffers were not applied: {error}"
        ));
    }
    // And its redundancy settings, so a musician sent twice whom the session does not keep so goes
    // back to once.
    if let Err(error) = engine.replace_redundancies(&redundancy::redundancies_of(next)) {
        refused.push(format!(
            "the saved redundancy settings were not applied: {error}"
        ));
    }
    // And its limiters: a musician whose mix the session keeps no limiter for goes back to none,
    // and the local mix takes the one `next` keeps: the saved one, or the desk's own when the
    // saved session has no local section (see [limiter::hand_kept_limiters]).
    refused.extend(limiter::hand_kept_limiters(engine, next));
    // And its effects and every mix's returns of them, the same way.
    refused.extend(effects::hand_kept_effects(engine, next));
    let desk = engine.desk();
    let (pushed, skipped) = push_saved_mixes(engine, &desk, next, mixes);
    let local = local.map(|kept| apply_kept_local(engine, kept));
    refused.extend(local.as_ref().and_then(|local| local.refused.clone()));
    Ok(LiveLoad {
        pushed,
        skipped,
        refused,
        local,
        processing_kept,
    })
}

/// The desk's change that puts the `saved` mix on a musician whose mix is `current`, placed for the
/// live groups `groups` (their ids in order): each saved input from CH 1 on, every live group's
/// level by its id (a live group the saved mix does not name keeps its own), the master volume and
/// the master mute. For a musician who `follows` another's mix only the master volume and mute: the
/// rest follows the leader. The phone's own level cap and the engineer's controls are left alone.
fn saved_update(
    saved: &SessionMix,
    current: &MixValues,
    groups: &[String],
    follows: bool,
) -> MixUpdate {
    let master = MixUpdate {
        volume_percent: Some(i32::from(saved.volume_percent)),
        muted: Some(saved.muted),
        ..MixUpdate::default()
    };
    if follows {
        return master;
    }
    let channels = &saved.channels[..saved.channels.len().min(current.channel_gains.len())];
    let values = |value: fn(&SessionPresetChannel) -> i32| channels.iter().map(value).collect();
    let flags = |flag: fn(&SessionPresetChannel) -> bool| channels.iter().map(flag).collect();
    MixUpdate {
        channel_gains: Some(values(|channel| i32::from(channel.gain))),
        pans: Some(values(|channel| i32::from(channel.pan))),
        channel_muted: Some(flags(|channel| channel.muted)),
        channel_solo: Some(flags(|channel| channel.solo)),
        group_levels: Some(
            groups
                .iter()
                .take(current.group_levels.len())
                .enumerate()
                .map(|(position, id)| {
                    i32::from(
                        saved
                            .groups
                            .get(id)
                            .copied()
                            .unwrap_or(current.group_levels[position]),
                    )
                })
                .collect(),
        ),
        ..master
    }
}

/// Pushes each saved mix in `mixes` to the musician `desk` lists under its key (their phone's
/// client id first, then their address), through the desk's path, and answers the mixes pushed and
/// the saved mixes that went to nobody (see [load_session] for who is skipped).
fn push_saved_mixes<E: SessionEngine>(
    engine: &mut E,
    desk: &DeskView,
    next: &Session,
    mixes: &BTreeMap<String, SessionMix>,
) -> (Vec<LoadedMixDto>, Vec<SkippedMixDto>) {
    let mut pushed = Vec::new();
    let mut skipped = Vec::new();
    let mut used = HashSet::new();
    for row in &desk.musicians {
        let address = row.address.to_string();
        let phone = phone_id(row.client_id.as_deref());
        let key = phone
            .map(client_key)
            .into_iter()
            .chain([address_key(&address)])
            .find(|key| mixes.contains_key(key));
        let Some(key) = key else {
            continue;
        };
        used.insert(key.clone());
        if phone.is_some() && !row.control_connected {
            skipped.push(skipped_mix(next, &key, "notConnected", None));
            continue;
        }
        let follows = row.mirror.as_ref().map(|mirror| mirror.name.clone());
        let update = saved_update(&mixes[&key], &row.mix, &desk.groups, follows.is_some());
        match engine.push_mix(&address, &update) {
            Ok(mix) => pushed.push(LoadedMixDto {
                address,
                follows,
                mix: MixDto::from(&mix),
            }),
            Err(error) => skipped.push(skipped_mix(next, &key, "refused", Some(error))),
        }
    }
    for key in mixes.keys().filter(|key| !used.contains(*key)) {
        skipped.push(skipped_mix(next, key, "notHere", None));
    }
    (pushed, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::MusicianDto;
    use crate::events::{EventPump, PumpSink, PumpSource};
    use crate::session::{address_key, client_key};
    use personal_monitoring::LimiterSettings;

    fn status_with_musicians(addresses: &[&str]) -> EngineStatusDto {
        EngineStatusDto {
            local_fx_returns: Default::default(),
            effects: Default::default(),
            local_limiter: LimiterSettings::master(),
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
                    fx_returns: Default::default(),
                    limiter: LimiterSettings::master(),
                    address: (*address).to_owned(),
                    client_name: None,
                    client_id: None,
                    name: None,
                    name_is_local: false,
                    control_connected: false,
                    mix: MixDto::from(&personal_monitoring::MixValues::default()),
                    packets_sent: 0,
                    packets_discarded: 0,
                    last_send_error: None,
                    link_health: "unknown",
                    packets_per_second: None,
                    underruns_per_second: None,
                    loss_percent: None,
                    receive_timeouts_per_second: None,
                    capabilities: Vec::new(),
                    rtt_ms: None,
                    rtt_last_ms: None,
                    link_latest: None,
                    mirror: None,
                    buffer: crate::dto::BufferDto::from(personal_monitoring::PlaybackBuffer::Auto),
                    buffer_supported: false,
                    redundancy: crate::dto::RedundancyDto::from(Redundancy::Off),
                    duplicates_sent: 0,
                    recovered: 0,
                    redundancy_supported: false,
                    redundancy_hold_supported: false,
                })
                .collect(),
            local_mix: MixDto::from(&personal_monitoring::MixValues::default()),
            local_output: crate::dto::LocalOutputDto::from(
                &personal_monitoring::LocalOutputStatus::default(),
            ),
            recording: crate::dto::RecordingDto::from(
                &personal_monitoring::RecordingStatus::default(),
            ),
            last_take: None,
            soundcheck: crate::dto::SoundcheckDto::from(
                &personal_monitoring::SoundcheckStatus::default(),
            ),
            demo: false,
            load: Default::default(),
        }
    }

    /// An empty scratch config directory for one test's session.
    fn session_directory(test: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir()
            .join("bandear-command-tests")
            .join(test);
        let _ = std::fs::remove_dir_all(&directory);
        directory
    }

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    /// The engine's row for the phone `client_id` at `address`, every other value neutral.
    fn engine_row(address: &str, client_id: Option<&str>) -> personal_monitoring::MusicianStatus {
        personal_monitoring::MusicianStatus {
            limiter: Default::default(),
            fx_returns: Default::default(),
            address: address.parse().expect("test address"),
            name: None,
            client_id: client_id.map(str::to_owned),
            control_connected: true,
            mix: Default::default(),
            counters: Default::default(),
            link: Default::default(),
            capabilities: Vec::new(),
            mirror: None,
            playback_buffer: personal_monitoring::PlaybackBuffer::Auto,
            redundancy: personal_monitoring::Redundancy::Off,
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
            .join("bandear-command-tests")
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

        fn local_mix(&self) -> Option<personal_monitoring::MixValues> {
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

    /// The demo source is a driver of its own, started only when the window names it: no driver,
    /// or one the desk does not know, is the system's own, never the demo.
    #[test]
    fn the_demo_source_starts_only_when_the_window_names_it() {
        let named = |host: Option<&str>| {
            to_engine_config(StartRequest {
                host: host.map(str::to_owned),
                ..StartRequest::default()
            })
            .host
        };
        assert_eq!(named(Some("demo")), AudioHost::Demo);
        assert_eq!(named(None), AudioHost::Auto);
        assert_eq!(named(Some("jack")), AudioHost::Auto);
        assert!(
            NO_INPUT_DEVICE.contains("demo source"),
            "a machine with no interface is told about it: {NO_INPUT_DEVICE}"
        );

        let listed = list_input_devices(Some("demo".to_owned())).expect("listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, personal_monitoring::DEMO_DEVICE_NAME);
        assert_eq!(listed[0].channels, personal_monitoring::DEMO_CHANNELS);
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
        /// The musicians it lists, by address, with their phones' client ids.
        rows: Vec<(String, Option<String>)>,
        /// The engineer's controls it took last.
        controls: Option<EngineerControls>,
        /// What it answers for a musician's mix as a preset.
        preset: MixPreset,
        /// The preset it put on musicians last, and whom.
        took_preset: Option<(MixPreset, Vec<String>)>,
        /// The copy it made last: from whom, onto whom.
        copied: Option<(String, Vec<String>)>,
        /// The mirror it took last: the follower and the leader's address and name.
        mirrored: Option<(String, Option<(String, String)>)>,
        /// What a session save or load reads of it: the device's inputs, the live groups' ids
        /// (those of the groups it took last, once it took any) and the musicians here.
        inputs: usize,
        live_groups: Vec<String>,
        here: Mutex<Vec<MusicianStatus>>,
        /// Every step a session load took, in order.
        log: Mutex<Vec<String>>,
        /// The mixes pushed to musicians: to whom, and the change.
        pushed: Vec<(String, MixUpdate)>,
        /// The controls and mirrors it took whole last.
        kept: KeptBooks,
        /// Why it refuses to take controls and mirrors whole, when it does.
        refuses_kept: Option<String>,
        /// The playback buffers it took whole last.
        buffers: Mutex<Option<Vec<(MusicianKey, PlaybackBuffer)>>>,
        /// The redundancy settings it took whole last.
        redundancies: Mutex<Option<Vec<(MusicianKey, Redundancy)>>>,
        /// The musicians' limiters it took whole last, and the local mix's it took last.
        limiters: Mutex<Option<Vec<(MusicianKey, LimiterSettings)>>>,
        local_limiter: Mutex<Option<LimiterSettings>>,
        /// The effects it took, the musicians' returns it took whole last and the local mix's.
        effects: Mutex<personal_monitoring::EffectSettings>,
        fx_returns: Mutex<Option<Vec<(MusicianKey, personal_monitoring::EffectLevels)>>>,
        local_fx_returns: Mutex<Option<personal_monitoring::EffectLevels>>,
        /// The musician whose mix it refuses to change, when it does.
        refuses_push_to: Option<String>,
        /// The engineer's local mix as a session save or load reads it.
        local: MixValues,
        /// The local mix changes it took, in order.
        local_pushed: Vec<MixUpdate>,
        /// The outputs its driver offers, and where the local mix plays.
        outputs: Vec<personal_monitoring::OutputDevice>,
        output: personal_monitoring::LocalOutputStatus,
        /// Every channel's strip, as the engine keeps them (none until it took any).
        strips: Mutex<Vec<StripSettings>>,
        /// Why it refuses to take every strip whole, when it does.
        refuses_processing: Option<String>,
    }

    impl ProcessingEngine for StandInEngine {
        fn set_channel_processing(
            &self,
            channel: usize,
            settings: StripSettings,
        ) -> Result<StripSettings, String> {
            let mut strips = self.strips.lock().expect("strips lock");
            strips.resize(32, StripSettings::default());
            strips[channel] = settings.clamped();
            Ok(strips[channel])
        }

        fn processing(&self) -> Vec<StripSettings> {
            self.strips.lock().expect("strips lock").clone()
        }

        fn replace_processing(
            &self,
            settings: &[StripSettings],
        ) -> Result<Vec<StripSettings>, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            if let Some(why) = &self.refuses_processing {
                return Err(why.clone());
            }
            self.note("replace_processing".to_owned());
            *self.strips.lock().expect("strips lock") = settings.to_vec();
            Ok(settings.to_vec())
        }

        fn set_channel_send(
            &self,
            channel: usize,
            effect: personal_monitoring::Effect,
            level: u8,
        ) -> Result<StripSettings, String> {
            let current = self.processing().get(channel).copied().unwrap_or_default();
            self.set_channel_processing(
                channel,
                StripSettings {
                    sends: current.sends.with(effect, level),
                    ..current
                },
            )
        }
    }

    impl effects::EffectsEngine for StandInEngine {
        fn set_reverb(
            &self,
            settings: personal_monitoring::ReverbSettings,
        ) -> Result<personal_monitoring::EffectSettings, String> {
            let mut effects = self.effects.lock().expect("effects lock");
            effects.reverb = settings.clamped();
            Ok(*effects)
        }

        fn set_delay(
            &self,
            settings: personal_monitoring::DelaySettings,
        ) -> Result<personal_monitoring::EffectSettings, String> {
            let mut effects = self.effects.lock().expect("effects lock");
            effects.delay = settings.clamped();
            Ok(*effects)
        }

        fn replace_effects(
            &self,
            settings: personal_monitoring::EffectSettings,
        ) -> Result<personal_monitoring::EffectSettings, String> {
            self.note("replace_effects".to_owned());
            *self.effects.lock().expect("effects lock") = settings.clamped();
            Ok(settings.clamped())
        }

        fn set_musician_fx_return(
            &self,
            key: &MusicianKey,
            _effect: personal_monitoring::Effect,
            _level: u8,
        ) -> Result<(MusicianKey, String, personal_monitoring::EffectLevels), String> {
            Err(format!("{key:?} is not here"))
        }

        fn replace_musician_fx_returns(
            &self,
            kept: &[(MusicianKey, personal_monitoring::EffectLevels)],
        ) -> Result<(), String> {
            self.note("replace_musician_fx_returns".to_owned());
            *self.fx_returns.lock().expect("returns lock") = Some(kept.to_vec());
            Ok(())
        }

        fn set_local_fx_return(
            &self,
            effect: personal_monitoring::Effect,
            level: u8,
        ) -> Result<personal_monitoring::EffectLevels, String> {
            let mut local = self.local_fx_returns.lock().expect("returns lock");
            let next = local.unwrap_or_default().with(effect, level);
            *local = Some(next);
            Ok(next)
        }

        fn replace_local_fx_returns(
            &self,
            levels: personal_monitoring::EffectLevels,
        ) -> Result<personal_monitoring::EffectLevels, String> {
            self.note("replace_local_fx_returns".to_owned());
            *self.local_fx_returns.lock().expect("returns lock") = Some(levels);
            Ok(levels)
        }
    }

    /// A processing saver with nothing waiting that writes nowhere.
    fn idle_processing_saver() -> ProcessingSaver {
        ProcessingSaver::manual(|_| Ok(()), |_| {})
    }

    /// The engineer's controls and the mirrors an engine keeps, as a load hands them over.
    type Books = (
        Vec<(MusicianKey, EngineerControls)>,
        Vec<(MusicianKey, MusicianKey, String)>,
    );
    type KeptBooks = Mutex<Option<Books>>;

    impl StandInEngine {
        fn note(&self, step: String) {
            self.log.lock().expect("log lock").push(step);
        }

        /// Whether `key` names the musician on `row`, as the engine's book does.
        fn names(key: &MusicianKey, row: &MusicianStatus) -> bool {
            match key {
                MusicianKey::Client(id) => row.client_id.as_deref() == Some(id.as_str()),
                MusicianKey::Address(at) => row.address.ip() == at.ip(),
            }
        }

        /// The key the stand-in keeps what it knows about the musician at `address` under.
        fn key_at(&self, address: &str) -> Result<MusicianKey, String> {
            let (at, client_id) = self
                .rows
                .iter()
                .find(|(at, _)| at == address)
                .cloned()
                .ok_or_else(|| format!("target `{address}` is not configured"))?;
            Ok(match client_id {
                Some(id) => MusicianKey::Client(id),
                None => MusicianKey::Address(at.parse().expect("a test address")),
            })
        }

        fn mixes(&self, musicians: &[String]) -> Vec<(SocketAddr, MixValues)> {
            musicians
                .iter()
                .map(|at| (at.parse().expect("a test address"), MixValues::default()))
                .collect()
        }
    }

    impl MixesEngine for StandInEngine {
        fn mix_preset(&self, address: &str) -> Result<MixPreset, String> {
            self.key_at(address).map(|_| self.preset.clone())
        }

        fn apply_preset(
            &mut self,
            preset: &MixPreset,
            musicians: &[String],
        ) -> Result<Vec<(SocketAddr, MixValues)>, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.took_preset = Some((preset.clone(), musicians.to_vec()));
            Ok(self.mixes(musicians))
        }

        fn copy_mix(
            &mut self,
            from: &str,
            to: &[String],
        ) -> Result<Vec<(SocketAddr, MixValues)>, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.key_at(from)?;
            self.copied = Some((from.to_owned(), to.to_vec()));
            Ok(self.mixes(to))
        }

        fn set_mirror(
            &mut self,
            follower: &str,
            leader: Option<MirrorSource<'_>>,
        ) -> Result<MirrorOutcome, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let follower_key = self.key_at(follower)?;
            let leader_key = leader
                .map(|source| self.key_at(source.address))
                .transpose()?;
            self.mirrored = Some((
                follower.to_owned(),
                leader.map(|source| (source.address.to_owned(), source.name.to_owned())),
            ));
            Ok(MirrorOutcome {
                follower: follower_key,
                leader: leader_key,
                name: leader.map(|source| source.name.to_owned()),
                mix: MixValues {
                    revision: 9,
                    ..MixValues::default()
                },
            })
        }
    }

    impl ControlsEngine for StandInEngine {
        fn set_engineer_controls(
            &mut self,
            address: &str,
            update: &EngineerUpdate,
        ) -> Result<(MusicianKey, MixValues), String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let (at, client_id) = self
                .rows
                .iter()
                .find(|(at, _)| at == address)
                .cloned()
                .ok_or_else(|| format!("target `{address}` is not configured"))?;
            let controls = merge_kept(self.controls.unwrap_or_default(), update)?;
            self.controls = Some(controls);
            let key = match client_id {
                Some(id) => MusicianKey::Client(id),
                None => MusicianKey::Address(at.parse().expect("a test address")),
            };
            Ok((
                key,
                MixValues {
                    engineer: controls,
                    revision: 7,
                    ..MixValues::default()
                },
            ))
        }
    }

    /// Holds one group edit or session load between its apply and its save while another change
    /// runs: it says it is parked, then waits until the session changes (another change saved
    /// while this one was half done) or `wait` passes, and notes which.
    struct Park {
        session: std::sync::Arc<SessionStore>,
        parked: std::sync::mpsc::Sender<()>,
        wait: std::time::Duration,
        saved_meanwhile: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl Park {
        fn hold(self) {
            let before = self.session.current();
            self.parked.send(()).expect("the test waits for the park");
            let deadline = std::time::Instant::now() + self.wait;
            while std::time::Instant::now() < deadline {
                if self.session.current() != before {
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
            self.note(format!("set_groups {}", definitions.len()));
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
            self.note(format!("set_channels {}", channels.len()));
            self.applied_channels = Some(channels);
            Ok(())
        }

        fn channel_table(&self) -> Vec<ChannelMeta> {
            self.applied_channels.clone().unwrap_or_default()
        }
    }

    impl DeskReader for StandInEngine {
        fn desk(&self) -> DeskView {
            DeskView {
                inputs: self.inputs,
                groups: self.applied.as_ref().map_or_else(
                    || self.live_groups.clone(),
                    |groups| groups.iter().map(|group| group.id.clone()).collect(),
                ),
                musicians: self.here.lock().expect("rows lock").clone(),
                local: self.local,
                processing: ProcessingEngine::processing(self),
            }
        }
    }

    impl LocalEngine for StandInEngine {
        fn set_local_mix(&mut self, update: &MixUpdate) -> Result<MixValues, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.note("set_local_mix".to_owned());
            self.local_pushed.push(update.clone());
            Ok(MixValues {
                revision: 12,
                volume_percent: update.volume_percent.map_or(100, |volume| volume as u8),
                ..MixValues::default()
            })
        }

        fn output_devices(&self) -> Result<Vec<personal_monitoring::OutputDevice>, String> {
            Ok(self.outputs.clone())
        }

        fn set_local_output(
            &mut self,
            request: Option<personal_monitoring::LocalOutputRequest>,
        ) -> Result<personal_monitoring::LocalOutputStatus, String> {
            self.note(format!(
                "set_local_output {}",
                request
                    .as_ref()
                    .map_or("none", |request| request.device.as_str())
            ));
            self.output = personal_monitoring::LocalOutputStatus {
                state: if request.is_some() {
                    personal_monitoring::LocalOutputState::Running
                } else {
                    personal_monitoring::LocalOutputState::Off
                },
                output: request,
                ..personal_monitoring::LocalOutputStatus::default()
            };
            Ok(self.output.clone())
        }

        fn local_output(&self) -> personal_monitoring::LocalOutputStatus {
            self.output.clone()
        }
    }

    impl limiter::LimiterEngine for StandInEngine {
        fn set_musician_limiter(
            &self,
            key: &MusicianKey,
            _settings: LimiterSettings,
        ) -> Result<(MusicianKey, String, LimiterSettings), String> {
            Err(format!("{key:?} is not here"))
        }

        fn set_local_limiter(&self, settings: LimiterSettings) -> Result<LimiterSettings, String> {
            self.note("set_local_limiter".to_owned());
            let kept = settings.clamped();
            *self.local_limiter.lock().expect("local limiter lock") = Some(kept);
            Ok(kept)
        }

        fn replace_musician_limiters(
            &self,
            kept: &[(MusicianKey, LimiterSettings)],
        ) -> Result<(), String> {
            self.note("replace_musician_limiters".to_owned());
            *self.limiters.lock().expect("limiters lock") = Some(kept.to_vec());
            Ok(())
        }
    }

    impl SessionEngine for StandInEngine {
        fn replace_playback_buffers(
            &self,
            kept: &[(MusicianKey, PlaybackBuffer)],
        ) -> Result<(), String> {
            self.note("replace_playback_buffers".to_owned());
            *self.buffers.lock().expect("buffers lock") = Some(kept.to_vec());
            Ok(())
        }

        fn replace_redundancies(&self, kept: &[(MusicianKey, Redundancy)]) -> Result<(), String> {
            self.note("replace_redundancies".to_owned());
            *self.redundancies.lock().expect("redundancies lock") = Some(kept.to_vec());
            Ok(())
        }

        /// Takes the controls and mirrors whole, as the engine does: every musician here follows the
        /// mirror kept for them now, or none; a mirror on oneself is refused and named.
        fn replace_kept(
            &self,
            controls: &[(MusicianKey, EngineerControls)],
            mirrors: &[(MusicianKey, MusicianKey, String)],
        ) -> Result<Vec<String>, String> {
            self.note("replace_kept".to_owned());
            if let Some(refusal) = &self.refuses_kept {
                return Err(refusal.clone());
            }
            *self.kept.lock().expect("kept lock") = Some((controls.to_vec(), mirrors.to_vec()));
            let mut refused = Vec::new();
            let mut rows = self.here.lock().expect("rows lock");
            for row in rows.iter_mut() {
                row.mirror = None;
            }
            for (follower, leader, name) in mirrors {
                if follower == leader {
                    refused.push(format!("{follower:?} cannot follow their own mix"));
                    continue;
                }
                for row in rows.iter_mut().filter(|row| Self::names(follower, row)) {
                    row.mirror = Some(personal_monitoring::MirrorStatus {
                        leader: leader.clone(),
                        name: name.clone(),
                        leader_address: None,
                    });
                }
            }
            Ok(refused)
        }

        fn push_mix(&mut self, address: &str, update: &MixUpdate) -> Result<MixValues, String> {
            self.note(format!("push_mix {address}"));
            if self.refuses_push_to.as_deref() == Some(address) {
                return Err(format!("target `{address}` is not configured"));
            }
            self.pushed.push((address.to_owned(), update.clone()));
            Ok(MixValues {
                revision: 11,
                volume_percent: update.volume_percent.map_or(100, |volume| volume as u8),
                ..MixValues::default()
            })
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

    /// The window edits the whole list: with an engine running it reads the engine's, entries for
    /// inputs the device lacks included; with none, the session's, a kind this build does not know
    /// read as written.
    #[test]
    fn the_channel_table_is_the_running_engines_whole_list_else_the_sessions() {
        let session = SessionStore::open(&session_directory("channel-table"));
        session
            .set_channels(vec![
                kept("Kick", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ])
            .expect("kept");
        let idle: Mutex<Option<StandInEngine>> = Mutex::new(None);

        assert_eq!(
            read_channel_table(&idle, &session).expect("readable"),
            vec![
                kept("Kick", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ]
        );

        let engine = running(StandInEngine::default());
        let whole = vec![
            detail("Kick", "drums", false, false),
            detail("Keys L", "keys", false, true),
            detail("Keys R", "keys", false, false),
            detail("Spare", "other", true, false),
        ];
        edit_channels(&engine, &session, whole).expect("the details are taken");
        assert_eq!(
            read_channel_table(&engine, &session).expect("readable"),
            vec![
                kept("Kick", "drums", false, false),
                kept("Keys L", "keys", false, true),
                kept("Keys R", "keys", false, false),
                kept("Spare", "other", true, false),
            ]
        );
    }

    /// A kind a newer build wrote reads back as written, from the session or from the running
    /// engine (where it applies as `other`); sent back with an edit of another entry, or of that
    /// entry's name, it is kept, with or without an engine. Choosing a kind for that entry replaces
    /// it, an explicit `other` included; an unknown kind the session does not keep there is refused.
    #[test]
    fn an_edit_keeps_a_kind_this_build_does_not_know_until_that_entry_gets_another_kind() {
        let session = SessionStore::open(&session_directory("channel-table-unknown-kind"));
        session
            .set_channels(vec![
                kept("Kick", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ])
            .expect("kept");
        let idle: Mutex<Option<StandInEngine>> = Mutex::new(None);
        let round_trip = |engine: &Mutex<Option<StandInEngine>>,
                          change: &dyn Fn(&mut Vec<ChannelRequest>)| {
            let read =
                serde_json::to_string(&read_channel_table(engine, &session).expect("readable"))
                    .expect("serializes");
            let mut requests: Vec<ChannelRequest> =
                serde_json::from_str(&read).expect("the window can send back what it reads");
            change(&mut requests);
            edit_channels(engine, &session, requests).expect("the details are taken");
        };

        round_trip(&idle, &|requests| requests[0].name = "Kick in".to_owned());
        assert_eq!(
            session.channels(),
            vec![
                kept("Kick in", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ],
            "an idle edit of another entry keeps the kind as written"
        );

        round_trip(&idle, &|requests| {
            requests[1].name = "Theremin (spare)".to_owned()
        });
        assert_eq!(
            session.channels()[1],
            kept("Theremin (spare)", "theremin", true, false),
            "a new name for that entry keeps its kind too"
        );

        // A running engine took the details at start as they apply: the unknown kind is `other`.
        let engine = running(StandInEngine {
            applied_channels: Some(session_channels(&session)),
            ..StandInEngine::default()
        });
        round_trip(&engine, &|_| {});
        assert_eq!(
            applied_channels(&engine).expect("taken")[1].kind,
            ChannelKind::Other,
            "it applies as other"
        );
        assert_eq!(
            read_channel_table(&engine, &session).expect("readable")[1].kind,
            "theremin",
            "and reads back as written"
        );
        round_trip(&engine, &|requests| requests[0].hidden = true);
        assert_eq!(
            session.channels()[1].kind,
            "theremin",
            "an edit read from the running engine keeps it as well"
        );

        let refused = edit_channels(
            &idle,
            &session,
            vec![
                detail("Kick in", "theremin", true, false),
                detail("Theremin (spare)", "theremin", true, false),
            ],
        )
        .expect_err("an unknown kind the session does not keep for that input");
        assert!(
            refused.contains("CH 1") && refused.contains("theremin"),
            "{refused}"
        );

        round_trip(&idle, &|requests| requests[1].kind = "other".to_owned());
        assert_eq!(
            session.channels()[1],
            kept("Theremin (spare)", "other", true, false),
            "an explicit other replaces it"
        );
        session
            .set_channels(vec![
                kept("Kick in", "drums", true, false),
                kept("Theremin (spare)", "theremin", true, false),
            ])
            .expect("kept");
        round_trip(&idle, &|requests| requests[1].kind = "keys".to_owned());
        assert_eq!(
            session.channels()[1],
            kept("Theremin (spare)", "keys", true, false),
            "choosing another kind for that entry replaces it"
        );
    }

    /// An explicit `other` the running engine took while the session could not be saved is what
    /// the table reads back: the stale kind the session still holds never returns with the next
    /// edit of another input.
    #[test]
    fn after_a_failed_save_the_engines_explicit_other_is_read_back_not_the_stale_kind() {
        let directory = session_directory("channel-table-stale-kind");
        let session = SessionStore::open(&directory);
        session
            .set_channels(vec![
                kept("Kick", "drums", false, false),
                kept("Theremin", "theremin", true, false),
            ])
            .expect("kept");
        let engine = running(StandInEngine {
            applied_channels: Some(session_channels(&session)),
            ..StandInEngine::default()
        });
        let edit = |change: &dyn Fn(&mut Vec<ChannelRequest>)| {
            let read =
                serde_json::to_string(&read_channel_table(&engine, &session).expect("readable"))
                    .expect("serializes");
            let mut requests: Vec<ChannelRequest> =
                serde_json::from_str(&read).expect("the window can send back what it reads");
            change(&mut requests);
            edit_channels(&engine, &session, requests).expect("the engine takes them")
        };
        // A folder where the temporary file goes makes every save fail.
        let blocker = directory.join(format!("{}.tmp", crate::session::SESSION_FILE));
        std::fs::create_dir(&blocker).expect("blocker");

        let other = edit(&|requests| requests[1].kind = "other".to_owned());

        assert!(other.applied && !other.saved, "{other:?}");
        assert_eq!(
            session.channels()[1].kind,
            "theremin",
            "the session is behind"
        );
        assert_eq!(
            read_channel_table(&engine, &session).expect("readable")[1].kind,
            "other",
            "the engine's newer other, not the stale kind"
        );
        std::fs::remove_dir(&blocker).expect("the disk saves again");
        let renamed = edit(&|requests| requests[0].name = "Kick in".to_owned());
        assert!(renamed.saved, "{renamed:?}");
        assert_eq!(
            session.channels(),
            vec![
                kept("Kick in", "drums", false, false),
                kept("Theremin", "other", true, false),
            ],
            "the replaced kind does not come back with an edit of another input"
        );
        assert!(!session.channels_behind());
    }

    /// Pinned, as accepted: while the session is behind, an unknown kind on an input no one edited
    /// reads as the engine applies it, `other`, and the next save keeps that.
    #[test]
    fn while_the_session_is_behind_every_unknown_kind_reads_as_the_engine_applies_it() {
        let directory = session_directory("channel-table-behind-untouched");
        let session = SessionStore::open(&directory);
        session
            .set_channels(vec![
                kept("Kick", "drums", false, false),
                kept("Pad", "synthpad", false, false),
            ])
            .expect("kept");
        let engine = running(StandInEngine {
            applied_channels: Some(session_channels(&session)),
            ..StandInEngine::default()
        });
        let blocker = directory.join(format!("{}.tmp", crate::session::SESSION_FILE));
        std::fs::create_dir(&blocker).expect("blocker");
        edit_channels(
            &engine,
            &session,
            vec![
                detail("Kick", "voice", false, false),
                detail("Pad", "synthpad", false, false),
            ],
        )
        .expect("the engine takes them, the save fails");
        assert!(session.channels_behind());

        assert_eq!(
            read_channel_table(&engine, &session).expect("readable")[1].kind,
            "other",
            "the input no one edited reads as the engine applies it"
        );
        std::fs::remove_dir(&blocker).expect("the disk saves again");
        let read = read_channel_table(&engine, &session).expect("readable");
        let requests: Vec<ChannelRequest> =
            serde_json::from_str(&serde_json::to_string(&read).expect("serializes"))
                .expect("the window sends back what it reads");
        edit_channels(&engine, &session, requests).expect("saved");
        assert_eq!(session.channels()[1].kind, "other");
    }

    /// Every start hands the new engine the session's groups and details, so the session is no
    /// longer behind it.
    #[test]
    fn a_start_hands_the_engine_the_sessions_layout_and_the_session_is_no_longer_behind() {
        let session = SessionStore::open(&session_directory("start-not-behind"));
        session.set_channels_behind(true);
        let mut engine = StandInEngine::default();

        hand_session_layout(
            &mut engine,
            &session,
            vec![GroupDefinition {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1],
            }],
            vec![ChannelMeta {
                name: "Kick".to_owned(),
                kind: ChannelKind::Drums,
                hidden: false,
                linked_to_next: false,
            }],
        );

        assert!(!session.channels_behind());
        assert_eq!(engine.applied.map(|groups| groups.len()), Some(1));
        assert_eq!(
            engine
                .applied_channels
                .map(|channels| channels[0].name.clone()),
            Some("Kick".to_owned())
        );
    }

    /// A loaded session the running engine took but the desk could not save leaves the session
    /// behind the engine; the next save that works clears it.
    #[test]
    fn a_loaded_session_the_desk_could_not_save_leaves_the_session_behind() {
        let test = "session-load-behind";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                here: Mutex::new(vec![here(RACK, None, false, MixValues::default())]),
                ..StandInEngine::default()
            }),
        );
        saved_session(test, "show", &snapshot(layout_session(&[], 2), &[]));
        let blocker =
            session_directory_kept(test).join(format!("{}.tmp", crate::session::SESSION_FILE));
        std::fs::create_dir_all(&blocker).expect("blocker");

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert!(outcome.applied && !outcome.saved, "{outcome:?}");
        assert!(session.channels_behind());
        std::fs::remove_dir(&blocker).expect("the disk saves again");
        session
            .set_channels(vec![kept("In 1", "other", false, false)])
            .expect("saved");
        assert!(!session.channels_behind());
    }

    /// What the window reads is what it sends back: with one entry changed on the table it read,
    /// every other entry (the details of inputs a smaller device lacks among them) is kept by the
    /// engine and the session.
    #[test]
    fn an_edit_of_the_table_read_back_keeps_every_other_entry() {
        let session = SessionStore::open(&session_directory("channel-table-edit"));
        let engine = running(StandInEngine::default());
        edit_channels(
            &engine,
            &session,
            vec![
                detail("Kick", "drums", false, false),
                detail("Snare", "drums", false, false),
                detail("Spare", "other", true, false),
            ],
        )
        .expect("the details are taken");

        let read = serde_json::to_string(&read_channel_table(&engine, &session).expect("readable"))
            .expect("serializes");
        let mut requests: Vec<ChannelRequest> =
            serde_json::from_str(&read).expect("the window can send back what it reads");
        requests[0].name = "Kick in".to_owned();
        edit_channels(&engine, &session, requests).expect("the details are taken");

        assert_eq!(
            session.channels(),
            vec![
                kept("Kick in", "drums", false, false),
                kept("Snare", "drums", false, false),
                kept("Spare", "other", true, false),
            ]
        );
        assert_eq!(
            applied_channels(&engine).map(|channels| channels.len()),
            Some(3)
        );
    }

    /// The groups a `set_groups` answer carries are what the window sends back on its next edit:
    /// the same ids (a new group's included) and the same 1-based inputs, so a chain of edits never
    /// moves a member or loses a group's identity.
    #[test]
    fn a_group_answer_sent_back_as_the_next_request_keeps_its_ids_and_inputs() {
        let session = SessionStore::open(&session_directory("groups-roundtrip"));
        let engine = running(StandInEngine::default());
        let first = edit_groups(
            &engine,
            &session,
            vec![
                request(Some("kit"), "Drums", &[1, 2, 3]),
                request(None, "Vocals", &[5, 6]),
            ],
        )
        .expect("the groups are taken");
        let new_id = first.groups[1].id.clone();
        assert!(!new_id.is_empty(), "a new group gets an id");

        let answer = serde_json::to_value(&first).expect("serializes");
        assert_eq!(
            answer["groups"],
            serde_json::json!([
                { "id": "kit", "name": "Drums", "channels": [1, 2, 3] },
                { "id": new_id, "name": "Vocals", "channels": [5, 6] },
            ]),
            "the inputs are numbered from 1, as the request numbered them"
        );
        let mut requests: Vec<GroupRequest> =
            serde_json::from_value(answer["groups"].clone()).expect("an answer is a request");
        requests[1].name = "Voices".to_owned();
        let second = edit_groups(&engine, &session, requests).expect("the groups are taken");

        assert_eq!(
            second
                .groups
                .iter()
                .map(|group| (group.id.clone(), group.name.clone(), group.channels.clone()))
                .collect::<Vec<_>>(),
            vec![
                ("kit".to_owned(), "Drums".to_owned(), vec![1, 2, 3]),
                (new_id, "Voices".to_owned(), vec![5, 6]),
            ]
        );
        assert_eq!(
            applied_names(&engine),
            Some(vec!["Drums".to_owned(), "Voices".to_owned()])
        );
        assert_eq!(session.groups(), second.groups);
    }

    /// The rows a stand-in engine lists for the engineer's controls: its addresses with the client
    /// ids their phones announced.
    fn listing(rows: &[(&str, Option<&str>)]) -> StandInEngine {
        StandInEngine {
            rows: rows
                .iter()
                .map(|(address, id)| ((*address).to_owned(), id.map(str::to_owned)))
                .collect(),
            ..StandInEngine::default()
        }
    }

    fn controls_request(limit: Option<i32>, locked: Option<&[bool]>) -> EngineerControlsRequest {
        EngineerControlsRequest {
            limit_percent: limit,
            locked_channels: locked.map(<[bool]>::to_vec),
        }
    }

    fn kept_controls(limit_percent: u8, locked_channels: &[usize]) -> SessionEngineer {
        SessionEngineer {
            limit_percent,
            locked_channels: locked_channels.to_vec(),
        }
    }

    #[test]
    fn a_running_engine_takes_the_controls_and_they_are_saved_for_the_phone() {
        let test = "controls-phone";
        let session = SessionStore::open(&session_directory(test));
        session
            .set_engineer(None, "192.168.1.10:50000", kept_controls(40, &[]))
            .expect("kept for the address");
        let engine = running(listing(&[("192.168.1.10:50000", Some(PHONE))]));

        let outcome = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.10:50000",
            Some(PHONE),
            controls_request(Some(60), Some(&[true, true])),
        )
        .expect("the controls are taken");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(outcome.kept_for, "phone");
        assert_eq!(outcome.limit_percent, 60);
        assert_eq!(outcome.locked_channels.len(), 32);
        assert_eq!(&outcome.locked_channels[..3], &[true, true, false]);
        let mix = outcome.mix.expect("the mix the engine stored");
        assert_eq!(mix.engineer_limit_percent, 60);
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| engine.controls),
            Some(EngineerControls {
                limit_percent: 60,
                locked: 0b11,
            }),
            "the engine took them"
        );
        assert_eq!(
            saved_musicians(test),
            serde_json::json!({
                format!("client:{PHONE}"): {
                    "engineer": { "limitPercent": 60, "lockedChannels": [1, 2] }
                }
            }),
            "kept for the phone, and those kept for its address are gone"
        );
    }

    #[test]
    fn a_configured_targets_controls_are_saved_for_its_address() {
        let test = "controls-address";
        let session = SessionStore::open(&session_directory(test));
        let engine = running(listing(&[("192.168.1.40:50000", None)]));

        let outcome = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.40:50000",
            None,
            controls_request(None, Some(&[false, false, true])),
        )
        .expect("the controls are taken");

        assert_eq!(outcome.kept_for, "address");
        assert_eq!(
            saved_musicians(test),
            serde_json::json!({
                "address:192.168.1.40:50000": {
                    "engineer": { "limitPercent": 100, "lockedChannels": [3] }
                }
            })
        );
    }

    #[test]
    fn an_engine_that_is_stopping_refuses_the_controls_and_nothing_is_saved() {
        let session = SessionStore::open(&session_directory("controls-stopping"));
        let engine = running(StandInEngine {
            stopping: true,
            ..listing(&[("192.168.1.10:50000", Some(PHONE))])
        });

        let error = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.10:50000",
            Some(PHONE),
            controls_request(Some(60), None),
        )
        .expect_err("a stopping engine refuses");

        assert!(error.contains("stopping"), "{error}");
        assert_eq!(session.engineer_controls(), Vec::new());
    }

    #[test]
    fn controls_a_running_engine_took_but_the_session_could_not_keep_say_so() {
        let directory = session_directory("controls-unsaved");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            r#"{"version":99}"#,
        )
        .expect("a newer session");
        let session = SessionStore::open(&directory);
        let engine = running(listing(&[("192.168.1.10:50000", Some(PHONE))]));

        let outcome = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.10:50000",
            Some(PHONE),
            controls_request(Some(60), None),
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
    }

    #[test]
    fn controls_set_without_an_engine_join_those_kept_for_the_address() {
        let session = SessionStore::open(&session_directory("controls-idle"));
        session
            .set_engineer(None, "192.168.1.40:50000", kept_controls(50, &[2]))
            .expect("kept");
        let engine: Mutex<Option<StandInEngine>> = Mutex::new(None);

        let outcome = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.40:50000",
            None,
            controls_request(None, Some(&[true])),
        )
        .expect("kept for the next start");

        assert!(!outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(outcome.limit_percent, 50, "the kept limit stays");
        assert_eq!(&outcome.locked_channels[..3], &[true, true, false]);
        assert!(outcome.mix.is_none());
        assert_eq!(
            session.engineer_for(None, "192.168.1.40:50000"),
            Some(kept_controls(50, &[1, 2]))
        );
        let error = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.40:50000",
            None,
            controls_request(Some(10), Some(&[true; 33])),
        )
        .expect_err("a list longer than the mix is refused");
        assert!(error.contains("locked"), "{error}");
        assert_eq!(
            session.engineer_for(None, "192.168.1.40:50000"),
            Some(kept_controls(50, &[1, 2])),
            "nothing changed"
        );
    }

    #[test]
    fn a_start_gives_the_engine_the_controls_kept_for_each_phone_and_address() {
        let directory = session_directory("controls-start");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{
                    "client:{PHONE}":{{"engineer":{{"limitPercent":250,"lockedChannels":[1,33,0,32]}}}},
                    "address:192.168.1.40:50000":{{"label":"Rack","engineer":{{"limitPercent":70}}}},
                    "address:somewhere":{{"engineer":{{"limitPercent":10}}}},
                    "address:192.168.1.41:50000":{{"label":"Monitor"}}
                }}}}"#
            ),
        )
        .expect("a session");
        let session = SessionStore::open(&directory);

        assert_eq!(
            session_engineer_controls(&session),
            vec![
                (
                    MusicianKey::Address("192.168.1.40:50000".parse().unwrap()),
                    EngineerControls {
                        limit_percent: 70,
                        locked: 0,
                    }
                ),
                (
                    MusicianKey::Client(PHONE.to_owned()),
                    EngineerControls {
                        limit_percent: 100,
                        locked: 1 | 1 << 31,
                    }
                ),
            ],
            "a limit above 100 reads as 100, numbers outside 1-32 and unreadable keys are skipped"
        );
    }

    /// With no engine, an edit for a phone the window knows by its client id builds on the
    /// controls kept for that phone and is kept under its key, the key the next engine resolves
    /// first; those kept for its address go, so nothing at start stands in for the edit.
    #[test]
    fn idle_controls_for_a_known_phone_are_kept_under_its_client_id_for_the_next_start() {
        let session = SessionStore::open(&session_directory("controls-idle-phone"));
        session
            .set_engineer(Some(PHONE), "192.168.1.10:50000", kept_controls(40, &[3]))
            .expect("kept for the phone");
        session
            .set_engineer(None, "192.168.1.10:50000", kept_controls(70, &[]))
            .expect("kept for the address");
        let engine: Mutex<Option<StandInEngine>> = Mutex::new(None);

        let outcome = edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.10:50000",
            Some(PHONE),
            controls_request(None, Some(&[true])),
        )
        .expect("kept for the next start");

        assert!(!outcome.applied && outcome.saved, "{outcome:?}");
        assert_eq!(outcome.kept_for, "phone");
        assert_eq!(outcome.limit_percent, 40, "the phone's own limit stays");
        assert_eq!(&outcome.locked_channels[..3], &[true, false, true]);
        assert_eq!(
            session_engineer_controls(&session),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                EngineerControls {
                    limit_percent: 40,
                    locked: 0b101,
                }
            )],
            "the next engine gets exactly the edit, and nothing kept for the address"
        );
    }

    #[test]
    fn an_empty_client_id_names_no_phone_and_idle_controls_stay_with_the_address() {
        let engine: Mutex<Option<StandInEngine>> = Mutex::new(None);

        for (test, client_id) in [("controls-idle-empty", ""), ("controls-idle-blank", "   ")] {
            let session = SessionStore::open(&session_directory(test));
            let outcome = edit_engineer_controls(
                &engine,
                &session,
                "192.168.1.10:50000",
                Some(client_id),
                controls_request(Some(55), None),
            )
            .expect("kept for the next start");

            assert_eq!(outcome.kept_for, "address", "{client_id:?}");
            assert_eq!(
                session_engineer_controls(&session),
                vec![(
                    MusicianKey::Address("192.168.1.10:50000".parse().expect("an address")),
                    EngineerControls {
                        limit_percent: 55,
                        locked: 0,
                    }
                )],
                "{client_id:?} is kept under no phone"
            );
        }
    }

    #[test]
    fn an_idle_edit_must_name_a_musician_address() {
        let session = SessionStore::open(&session_directory("controls-idle-bad-address"));
        let engine: Mutex<Option<StandInEngine>> = Mutex::new(None);

        let error = edit_engineer_controls(
            &engine,
            &session,
            "somewhere",
            None,
            controls_request(Some(50), None),
        )
        .expect_err("an address that does not parse is refused");

        assert!(error.contains("somewhere"), "{error}");
        assert_eq!(session.engineer_controls(), Vec::new(), "nothing is kept");
    }

    /// The desk's half of a phone keeping its controls when DHCP gives it another IP: they are
    /// saved for its client id, so the next start hands the engine that key, and the engine puts
    /// them in force when the phone registers from any address.
    #[test]
    fn controls_kept_for_a_phone_are_handed_to_the_next_engine_by_its_client_id() {
        let session = SessionStore::open(&session_directory("controls-ip-change"));
        let engine = running(listing(&[("192.168.1.10:50000", Some(PHONE))]));
        edit_engineer_controls(
            &engine,
            &session,
            "192.168.1.10:50000",
            Some(PHONE),
            controls_request(Some(55), Some(&[false, true])),
        )
        .expect("the controls are taken");

        assert_eq!(
            session_engineer_controls(&session),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                EngineerControls {
                    limit_percent: 55,
                    locked: 0b10,
                }
            )]
        );
    }

    fn audio_request(
        host: &str,
        device: Option<&str>,
        asio_driver: Option<&str>,
    ) -> AudioSettingsRequest {
        AudioSettingsRequest {
            host: host.to_owned(),
            device: device.map(str::to_owned),
            buffer_frames: Some(256),
            asio_driver: asio_driver.map(str::to_owned),
        }
    }

    #[test]
    fn the_audio_input_an_engine_runs_with_is_kept_for_the_next_start() {
        let directory = session_directory("audio-running");
        let session = SessionStore::open(&directory);
        let engine = running(StandInEngine::default());

        let kept = keep_audio_settings(
            &engine,
            &session,
            audio_request("WASAPI", Some(" INPUT 1/2 (Volt 4) "), None),
        )
        .expect("kept");

        let volt = SessionAudio {
            host: "wasapi".to_owned(),
            device: Some("INPUT 1/2 (Volt 4)".to_owned()),
            buffer_frames: Some(256),
            asio_driver: None,
        };
        assert_eq!(kept, volt, "the driver's own name, the device trimmed");
        assert_eq!(session.audio(), Some(volt.clone()));
        assert_eq!(
            SessionStore::open(&directory).audio(),
            Some(volt),
            "and after a restart"
        );
    }

    /// The demo source is kept as any driver is, with its one device, for every later start.
    #[test]
    fn the_demo_source_is_kept_for_the_next_start() {
        let directory = session_directory("audio-demo");
        let session = SessionStore::open(&directory);
        let engine = running(StandInEngine::default());

        let kept = keep_audio_settings(
            &engine,
            &session,
            audio_request("demo", Some(personal_monitoring::DEMO_DEVICE_NAME), None),
        )
        .expect("kept");

        let demo = SessionAudio {
            host: "demo".to_owned(),
            device: Some("Demo (no interface)".to_owned()),
            buffer_frames: Some(256),
            asio_driver: None,
        };
        assert_eq!(kept, demo);
        assert_eq!(SessionStore::open(&directory).audio(), Some(demo));
    }

    #[test]
    fn with_no_engine_running_no_audio_input_is_kept() {
        let session = SessionStore::open(&session_directory("audio-idle"));
        let engine: Mutex<Option<StandInEngine>> = Mutex::new(None);

        let error = keep_audio_settings(
            &engine,
            &session,
            audio_request("wasapi", Some("INPUT 1/2 (Volt 4)"), None),
        )
        .expect_err("only settings an engine runs with are kept");

        assert!(error.contains("not running"), "{error}");
        assert_eq!(session.audio(), None, "nothing is kept");
    }

    #[test]
    fn asio_is_kept_only_with_the_driver_the_engineer_picked_and_an_unknown_driver_never() {
        let session = SessionStore::open(&session_directory("audio-asio"));
        let engine = running(StandInEngine::default());

        for refused in [
            audio_request("asio", None, None),
            audio_request("asio", Some("INPUT 1/2 (Volt 4)"), Some("  ")),
            audio_request("kernel", None, None),
        ] {
            let error =
                keep_audio_settings(&engine, &session, refused.clone()).expect_err("refused");
            assert!(
                error.contains("ASIO") || error.contains("kernel"),
                "{refused:?}: {error}"
            );
        }
        assert_eq!(session.audio(), None, "nothing is kept");

        let kept = keep_audio_settings(
            &engine,
            &session,
            audio_request("asio", Some(""), Some("Volt ASIO")),
        )
        .expect("an ASIO driver the engineer picked is kept");
        assert_eq!(kept.host, "asio");
        assert_eq!(kept.asio_driver.as_deref(), Some("Volt ASIO"));
        assert_eq!(kept.device, None, "an empty name is none");
    }

    #[test]
    fn the_window_sends_and_reads_the_audio_input_in_its_own_words() {
        let request: AudioSettingsRequest = serde_json::from_value(serde_json::json!({
            "host": "wasapi",
            "device": "INPUT 1/2 (Volt 4)",
            "bufferFrames": 512,
            "asioDriver": null
        }))
        .expect("the window's request reads");
        assert_eq!(request.buffer_frames, Some(512));
        assert!(
            serde_json::from_value::<AudioSettingsRequest>(serde_json::json!({ "driver": "asio" }))
                .is_err(),
            "a field the desk does not know is refused"
        );

        let state = DesktopState::new(SessionStore::open(&session_directory("audio-read")));
        assert_eq!(audio_settings(&state), None, "nothing kept yet");
        state
            .session
            .set_audio(SessionAudio {
                host: "asio".to_owned(),
                device: None,
                buffer_frames: Some(128),
                asio_driver: Some("Volt ASIO".to_owned()),
            })
            .expect("saved");
        assert_eq!(
            serde_json::to_value(audio_settings(&state)).expect("serializes"),
            serde_json::json!({
                "host": "asio",
                "device": null,
                "bufferFrames": 128,
                "asioDriver": "Volt ASIO"
            })
        );
    }

    const RACK: &str = "192.168.1.40:50000";
    const ANA: &str = "192.168.1.10:50000";

    /// A running stand-in listing Ana's phone and a rack, whose mixes read as `preset`.
    fn band(preset: MixPreset) -> Mutex<Option<StandInEngine>> {
        running(StandInEngine {
            rows: vec![
                (ANA.to_owned(), Some(PHONE.to_owned())),
                (RACK.to_owned(), None),
            ],
            preset,
            ..StandInEngine::default()
        })
    }

    fn two_inputs() -> MixPreset {
        MixPreset {
            channels: vec![
                PresetChannel {
                    gain: 80,
                    pan: 20,
                    muted: false,
                    solo: true,
                },
                PresetChannel {
                    gain: 60,
                    pan: 70,
                    muted: true,
                    solo: false,
                },
            ],
            groups: vec![("kit".to_owned(), 70), ("vox".to_owned(), 40)],
        }
    }

    #[test]
    fn a_preset_is_saved_from_a_musicians_mix_under_a_new_id_and_a_name_of_its_own() {
        let session = SessionStore::open(&session_directory("preset-save"));
        let engine = band(two_inputs());

        let saved = save_preset_with(&engine, &session, ANA, "  Vocal heavy ")
            .expect("a running engine has the mix");

        let preset = saved.preset.expect("the new preset");
        assert!(
            preset.id.starts_with('p') && preset.id.len() > 1,
            "{}",
            preset.id
        );
        assert_eq!(preset.name, "Vocal heavy");
        assert_eq!(
            serde_json::to_value(&preset.mix).expect("serializes"),
            serde_json::json!({
                "channels": [
                    { "gain": 80, "pan": 20, "muted": false, "solo": true },
                    { "gain": 60, "pan": 70, "muted": true, "solo": false }
                ],
                "groups": { "kit": 70, "vox": 40 }
            })
        );
        assert_eq!(session.presets(), saved.presets);
        for (name, refusal) in [
            ("vocal HEAVY", "already"),
            ("   ", "name"),
            (&"x".repeat(MAX_PRESET_NAME_LENGTH + 1), "characters"),
        ] {
            let error = save_preset_with(&engine, &session, ANA, name).expect_err("refused");
            assert!(error.contains(refusal), "{name}: {error}");
        }
        let idle = save_preset_with(&Mutex::new(None::<StandInEngine>), &session, ANA, "Other")
            .expect_err("a preset is saved from a mix that plays");
        assert!(idle.contains("not running"), "{idle}");
        assert_eq!(session.presets().len(), 1, "nothing else was kept");
    }

    #[test]
    fn presets_are_renamed_and_deleted_by_id_without_an_engine() {
        let session = SessionStore::open(&session_directory("preset-edit"));
        let engine = band(two_inputs());
        let verse = save_preset_with(&engine, &session, ANA, "Verse")
            .expect("saved")
            .preset
            .expect("kept");
        let chorus = save_preset_with(&engine, &session, ANA, "Chorus")
            .expect("saved")
            .preset
            .expect("kept");
        assert_ne!(verse.id, chorus.id);

        let renamed = rename_kept_preset(&session, &verse.id, "Bridge").expect("renamed");

        assert_eq!(
            renamed.preset.as_ref().map(|preset| preset.name.as_str()),
            Some("Bridge")
        );
        assert_eq!(renamed.presets[0].mix, verse.mix, "only the name changes");
        assert!(rename_kept_preset(&session, &verse.id, "chorus")
            .expect_err("taken")
            .contains("already"));
        assert!(rename_kept_preset(&session, "p-gone", "Intro")
            .expect_err("unknown")
            .contains("no preset"));
        let deleted = delete_kept_preset(&session, &verse.id).expect("deleted");
        assert_eq!(deleted.preset, None);
        assert_eq!(
            deleted
                .presets
                .iter()
                .map(|preset| &preset.name)
                .collect::<Vec<_>>(),
            ["Chorus"]
        );
        assert_eq!(session.presets(), deleted.presets);
        assert!(delete_kept_preset(&session, &verse.id).is_err());
    }

    #[test]
    fn a_kept_preset_is_put_on_musicians_through_the_engine_as_it_was_saved() {
        let session = SessionStore::open(&session_directory("preset-apply"));
        let engine = band(two_inputs());
        let kept = save_preset_with(&engine, &session, ANA, "Verse")
            .expect("saved")
            .preset
            .expect("kept");
        let both = vec![RACK.to_owned(), ANA.to_owned()];

        let applied = apply_preset_with(&engine, &session, &kept.id, &both).expect("applied");

        let took = engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .and_then(|engine| engine.took_preset.clone());
        assert_eq!(took, Some((two_inputs(), both.clone())), "as it was saved");
        assert_eq!(
            applied
                .mixes
                .iter()
                .map(|mixed| mixed.address.as_str())
                .collect::<Vec<_>>(),
            [RACK, ANA]
        );
        assert!(apply_preset_with(&engine, &session, "p-gone", &both)
            .expect_err("unknown")
            .contains("no preset"));
        let idle = apply_preset_with(
            &Mutex::new(None::<StandInEngine>),
            &session,
            &kept.id,
            &both,
        )
        .expect_err("a preset is put on mixes that play");
        assert!(idle.contains("not running"), "{idle}");
    }

    #[test]
    fn a_copy_goes_to_the_running_engine() {
        let engine = band(MixPreset::default());

        let copied = copy_mix_with(&engine, ANA, &[RACK.to_owned()]).expect("copied");

        assert_eq!(copied.mixes[0].address, RACK);
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| engine.copied.clone()),
            Some((ANA.to_owned(), vec![RACK.to_owned()]))
        );
        assert!(
            copy_mix_with(&Mutex::new(None::<StandInEngine>), ANA, &[RACK.to_owned()])
                .expect_err("idle")
                .contains("not running")
        );
    }

    fn leader(address: &str, client_id: Option<&str>, name: &str) -> MirrorRequest {
        MirrorRequest {
            address: address.to_owned(),
            client_id: client_id.map(str::to_owned),
            name: name.to_owned(),
        }
    }

    /// A mirror as the session keeps it, with the addresses kept beside the phones' keys.
    fn kept_mirror(
        leader: String,
        name: &str,
        leader_address: Option<&str>,
        follower_address: Option<&str>,
    ) -> SessionMirror {
        SessionMirror {
            leader,
            name: name.to_owned(),
            leader_address: leader_address.map(str::to_owned),
            follower_address: follower_address.map(str::to_owned),
        }
    }

    #[test]
    fn a_mirror_the_engine_took_is_saved_for_the_followers_phone_and_stopping_drops_it() {
        let session = SessionStore::open(&session_directory("mirror-running"));
        let engine = band(MixPreset::default());

        let started = edit_mirror(
            &engine,
            &session,
            ANA,
            Some(PHONE),
            Some(leader(RACK, None, "Rack")),
        )
        .expect("the engine took it");

        assert!(started.applied && started.saved, "{started:?}");
        assert_eq!(started.kept_for, "phone");
        assert_eq!(started.leader.as_deref(), Some(RACK));
        assert_eq!(started.name.as_deref(), Some("Rack"));
        assert_eq!(started.mix.as_ref().map(|mix| mix.revision), Some(9));
        assert_eq!(
            session.mirror_for(Some(PHONE), ANA),
            Some(kept_mirror(
                format!("address:{RACK}"),
                "Rack",
                None,
                Some(ANA)
            ))
        );
        let stopped = edit_mirror(&engine, &session, ANA, Some(PHONE), None).expect("stopped");
        assert!(stopped.applied && stopped.leader.is_none());
        assert_eq!(session.mirrors(), Vec::new());

        engine
            .lock()
            .expect("engine lock")
            .as_mut()
            .expect("running")
            .stopping = true;
        let refused = edit_mirror(
            &engine,
            &session,
            ANA,
            Some(PHONE),
            Some(leader(RACK, None, "Rack")),
        )
        .expect_err("a stopping engine refuses");
        assert!(refused.contains("stopping"), "{refused}");
        assert_eq!(session.mirrors(), Vec::new(), "nothing was saved");
    }

    #[test]
    fn with_no_engine_a_mirror_is_kept_for_the_next_start_and_never_in_a_circle() {
        let session = SessionStore::open(&session_directory("mirror-idle"));
        let idle = Mutex::new(None::<StandInEngine>);

        let kept = edit_mirror(
            &idle,
            &session,
            ANA,
            Some(PHONE),
            Some(leader(RACK, None, "Rack")),
        )
        .expect("kept for the next start");

        assert!(!kept.applied && kept.saved);
        assert_eq!(kept.kept_for, "phone");
        assert_eq!(
            session.mirrors(),
            vec![(
                format!("client:{PHONE}"),
                kept_mirror(format!("address:{RACK}"), "Rack", None, Some(ANA))
            )]
        );
        let circle = edit_mirror(
            &idle,
            &session,
            RACK,
            None,
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect_err("the rack cannot follow Ana while Ana follows the rack");
        assert!(circle.contains("circle"), "{circle}");
        let own = edit_mirror(
            &idle,
            &session,
            RACK,
            None,
            Some(leader(RACK, None, "Rack")),
        )
        .expect_err("nobody follows their own mix");
        assert!(own.contains("own"), "{own}");
        assert!(edit_mirror(&idle, &session, "rack.local:50000", None, None).is_err());
        assert_eq!(session.mirrors().len(), 1, "the refusals kept nothing");
    }

    const OTHER_PHONE: &str = "7d1e2f3a-4b5c-4d6e-8f9a-0b1c2d3e4f5a";
    const BASS_AT: &str = "192.168.1.12:50000";

    #[test]
    fn with_no_engine_a_circle_through_a_phone_and_its_address_is_refused() {
        let idle = Mutex::new(None::<StandInEngine>);
        // Ana, kept by her address, follows the bass player's phone; the bass player now asks to
        // follow Ana, named by her phone and her address.
        let by_address = SessionStore::open(&session_directory("mirror-mixed-address"));
        by_address
            .set_mirror(
                None,
                ANA,
                Some(kept_mirror(client_key(OTHER_PHONE), "Bass", None, None)),
            )
            .expect("saved");
        let refused = edit_mirror(
            &idle,
            &by_address,
            BASS_AT,
            Some(OTHER_PHONE),
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect_err("Ana follows the bass player already");
        assert!(refused.contains("circle"), "{refused}");

        // Ana, kept by her phone, follows the bass rack at one port; the rack is asked for at
        // another port of the same host, which the engine keeps as the same musician.
        let by_phone = SessionStore::open(&session_directory("mirror-mixed-phone"));
        by_phone
            .set_mirror(
                Some(PHONE),
                ANA,
                Some(kept_mirror(address_key(BASS_AT), "Bass", None, None)),
            )
            .expect("saved");
        let refused = edit_mirror(
            &idle,
            &by_phone,
            "192.168.1.12:50001",
            None,
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect_err("the rack is the one Ana follows");
        assert!(refused.contains("circle"), "{refused}");
        assert_eq!(
            by_address.mirrors().len() + by_phone.mirrors().len(),
            2,
            "nothing more kept"
        );
    }

    const KEYS_PHONE: &str = "3c4d5e6f-7a8b-4c9d-8e0f-1a2b3c4d5e6f";
    const KEYS_AT: &str = "192.168.1.17:50000";

    #[test]
    fn with_no_engine_a_circle_of_three_is_refused_whichever_key_each_mirror_was_kept_under() {
        let idle = Mutex::new(None::<StandInEngine>);

        // The bass player, kept by address, follows the keys player; Ana follows the bass player
        // named by their phone. The keys player then asks to follow Ana.
        let by_address = SessionStore::open(&session_directory("mirror-three-by-address"));
        edit_mirror(
            &idle,
            &by_address,
            BASS_AT,
            None,
            Some(leader(KEYS_AT, Some(KEYS_PHONE), "Keys")),
        )
        .expect("kept");
        edit_mirror(
            &idle,
            &by_address,
            ANA,
            Some(PHONE),
            Some(leader(BASS_AT, Some(OTHER_PHONE), "Bass")),
        )
        .expect("kept");
        let refused = edit_mirror(
            &idle,
            &by_address,
            KEYS_AT,
            Some(KEYS_PHONE),
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect_err("Ana follows the bass player, who follows the keys player");
        assert!(refused.contains("circle"), "{refused}");
        assert_eq!(by_address.mirrors().len(), 2, "nothing more kept");

        // The other way round: the bass player kept by phone, Ana following them by address.
        let by_phone = SessionStore::open(&session_directory("mirror-three-by-phone"));
        edit_mirror(
            &idle,
            &by_phone,
            BASS_AT,
            Some(OTHER_PHONE),
            Some(leader(KEYS_AT, Some(KEYS_PHONE), "Keys")),
        )
        .expect("kept");
        edit_mirror(
            &idle,
            &by_phone,
            ANA,
            Some(PHONE),
            Some(leader(BASS_AT, None, "Bass")),
        )
        .expect("kept");
        let refused = edit_mirror(
            &idle,
            &by_phone,
            KEYS_AT,
            Some(KEYS_PHONE),
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect_err("Ana follows the bass player, who follows the keys player");
        assert!(refused.contains("circle"), "{refused}");
        assert_eq!(by_phone.mirrors().len(), 2, "nothing more kept");
    }

    #[test]
    fn with_no_engine_a_chain_with_mixed_keys_that_does_not_close_is_kept() {
        let idle = Mutex::new(None::<StandInEngine>);
        let session = SessionStore::open(&session_directory("mirror-three-chain"));
        edit_mirror(
            &idle,
            &session,
            BASS_AT,
            None,
            Some(leader(KEYS_AT, Some(KEYS_PHONE), "Keys")),
        )
        .expect("kept");
        edit_mirror(
            &idle,
            &session,
            ANA,
            Some(PHONE),
            Some(leader(BASS_AT, Some(OTHER_PHONE), "Bass")),
        )
        .expect("kept");

        // The rack at another host follows the end of the chain, then its start: no circle.
        for leading in [
            leader(KEYS_AT, Some(KEYS_PHONE), "Keys"),
            leader(ANA, Some(PHONE), "Ana"),
        ] {
            let kept = edit_mirror(&idle, &session, RACK, None, Some(leading))
                .expect("a chain that does not come back is no circle");
            assert_eq!(kept.kept_for, "address");
        }
        // The keys player follows a musician outside the chain.
        let kept = edit_mirror(
            &idle,
            &session,
            KEYS_AT,
            Some(KEYS_PHONE),
            Some(leader("192.168.1.50:50000", None, "Drums")),
        )
        .expect("the drums follow nobody in the chain");
        assert_eq!(kept.kept_for, "phone");
        assert_eq!(session.mirrors().len(), 4);
    }

    #[test]
    fn with_no_engine_a_mirror_on_oneself_is_refused_as_the_engine_refuses_it() {
        let idle = Mutex::new(None::<StandInEngine>);
        let session = SessionStore::open(&session_directory("mirror-idle-self"));

        // The engine keeps an address by its IP and lists one musician per IP, so two ports of
        // one host are one musician to it.
        for (follower, client_id, leader_at, leader_id) in [
            ("192.168.1.9:50000", None, "192.168.1.9:50001", None),
            (ANA, Some(PHONE), "192.168.1.99:50000", Some(PHONE)),
        ] {
            let own = edit_mirror(
                &idle,
                &session,
                follower,
                client_id,
                Some(leader(leader_at, leader_id, "Me")),
            )
            .expect_err("nobody follows their own mix");
            assert!(own.contains("own mix"), "{follower} on {leader_at}: {own}");
        }
        let kept = edit_mirror(
            &idle,
            &session,
            "192.168.1.9:50000",
            None,
            Some(leader("192.168.1.10:50000", None, "Ten")),
        )
        .expect("another host is another musician");
        assert_eq!(kept.kept_for, "address");
    }

    #[test]
    fn a_mirror_the_engine_took_clears_the_address_entry_however_the_address_was_written() {
        let session = SessionStore::open(&session_directory("mirror-canonical"));
        session
            .set_mirror(
                None,
                "[::1]:50000",
                Some(kept_mirror(address_key(RACK), "Old", None, None)),
            )
            .expect("kept for the address");
        let engine = running(StandInEngine {
            rows: vec![
                ("[0:0:0:0:0:0:0:1]:50000".to_owned(), Some(PHONE.to_owned())),
                (RACK.to_owned(), None),
            ],
            ..StandInEngine::default()
        });

        edit_mirror(
            &engine,
            &session,
            "[0:0:0:0:0:0:0:1]:50000",
            Some(PHONE),
            Some(leader(RACK, None, "Rack")),
        )
        .expect("the engine took it");

        assert_eq!(
            session
                .mirrors()
                .into_iter()
                .map(|(key, _)| key)
                .collect::<Vec<_>>(),
            [client_key(PHONE)],
            "the address's entry gave way to the phone's"
        );
    }

    #[test]
    fn a_mirror_the_engine_took_but_the_session_could_not_keep_says_so() {
        let directory = session_directory("mirror-unsaved");
        std::fs::create_dir_all(&directory).expect("directory");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            r#"{"version":99}"#,
        )
        .expect("a newer session");
        let session = SessionStore::open(&directory);
        let engine = band(MixPreset::default());

        let outcome = edit_mirror(
            &engine,
            &session,
            ANA,
            Some(PHONE),
            Some(leader(RACK, None, "Rack")),
        )
        .expect("the engine took it");

        assert!(outcome.applied && !outcome.saved, "{outcome:?}");
        assert!(
            outcome
                .save_error
                .as_deref()
                .is_some_and(|error| error.contains("newer")),
            "{outcome:?}"
        );
        assert!(outcome.mix.is_some());
    }

    #[test]
    fn a_follower_without_a_phone_keeps_its_mirror_by_address_while_the_engine_runs() {
        let session = SessionStore::open(&session_directory("mirror-running-address"));
        let engine = band(MixPreset::default());

        let outcome = edit_mirror(
            &engine,
            &session,
            RACK,
            None,
            Some(leader(ANA, Some(PHONE), "Ana")),
        )
        .expect("the engine took it");

        assert_eq!(outcome.kept_for, "address");
        assert_eq!(
            session.mirrors(),
            vec![(
                address_key(RACK),
                kept_mirror(client_key(PHONE), "Ana", Some(ANA), None)
            )],
            "kept with the leader's key the engine gave, and the address beside it"
        );
    }

    #[test]
    fn a_stopping_engine_refuses_presets_and_copies_and_takes_nothing() {
        let session = SessionStore::open(&session_directory("mixes-stopping"));
        let engine = band(two_inputs());
        let kept = save_preset_with(&engine, &session, ANA, "Verse")
            .expect("saved")
            .preset
            .expect("kept");
        engine
            .lock()
            .expect("engine lock")
            .as_mut()
            .expect("running")
            .stopping = true;

        let preset = apply_preset_with(&engine, &session, &kept.id, &[RACK.to_owned()])
            .expect_err("a stopping engine refuses");
        let copy = copy_mix_with(&engine, ANA, &[RACK.to_owned()]).expect_err("and refuses this");

        for refused in [preset, copy] {
            assert!(refused.contains("stopping"), "{refused}");
        }
        let guard = engine.lock().expect("engine lock");
        let stand_in = guard.as_ref().expect("running");
        assert_eq!(stand_in.took_preset, None);
        assert_eq!(stand_in.copied, None);
    }

    /// Stands in for a new engine taking the session's mirrors: it records each and refuses the
    /// one whose follower it is told to.
    struct MirrorTaker {
        refuses: MusicianKey,
        taken: std::cell::RefCell<Vec<(MusicianKey, MusicianKey, String)>>,
    }

    impl MirrorKeeper for MirrorTaker {
        fn keep_mirror(
            &self,
            follower: &MusicianKey,
            leader: &MusicianKey,
            name: &str,
        ) -> Result<(), String> {
            if *follower == self.refuses {
                return Err("a mirror cannot go round in a circle".to_owned());
            }
            self.taken
                .borrow_mut()
                .push((follower.clone(), leader.clone(), name.to_owned()));
            Ok(())
        }
    }

    #[test]
    fn a_start_hands_every_kept_mirror_to_the_engine_and_names_those_it_refused() {
        let rack = MusicianKey::Address(RACK.parse().expect("an address"));
        let ana = MusicianKey::Client(PHONE.to_owned());
        let bass = MusicianKey::Client(OTHER_PHONE.to_owned());
        let engine = MirrorTaker {
            refuses: bass.clone(),
            taken: std::cell::RefCell::new(Vec::new()),
        };

        let refused = hand_kept_mirrors(
            &engine,
            vec![
                (ana.clone(), rack.clone(), "Rack".to_owned()),
                (bass.clone(), ana.clone(), "Ana".to_owned()),
                (rack.clone(), bass.clone(), "Bass".to_owned()),
            ],
        );

        assert_eq!(
            engine.taken.into_inner(),
            vec![
                (ana, rack.clone(), "Rack".to_owned()),
                (rack, bass, "Bass".to_owned())
            ],
            "every other mirror is in force"
        );
        assert_eq!(refused.len(), 1);
        assert!(
            refused[0].contains(OTHER_PHONE) && refused[0].contains("circle"),
            "{refused:?}"
        );
    }

    #[test]
    fn every_start_is_handed_the_sessions_mirrors_by_key() {
        let session = SessionStore::open(&session_directory("mirror-start"));
        session
            .set_mirror(
                Some(PHONE),
                ANA,
                Some(kept_mirror(format!("address:{RACK}"), "Rack", None, None)),
            )
            .expect("saved");
        session
            .set_mirror(
                None,
                RACK,
                Some(kept_mirror("somewhere".to_owned(), "?", None, None)),
            )
            .expect("saved");

        assert_eq!(
            session_mirrors(&session),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                MusicianKey::Address(RACK.parse().expect("an address")),
                "Rack".to_owned()
            )],
            "a leader key this build cannot read is skipped"
        );
    }

    const GONE_PHONE: &str = "9a8b7c6d-5e4f-4a3b-8c2d-1e0f9a8b7c6d";
    const AWAY_PHONE: &str = "1f2e3d4c-5b6a-4978-8695-a4b3c2d1e0f9";

    /// A musician the engine lists: at `address`, with `client_id`, their control channel up or
    /// down, and `mix`.
    fn here(
        address: &str,
        client_id: Option<&str>,
        connected: bool,
        mix: MixValues,
    ) -> MusicianStatus {
        MusicianStatus {
            limiter: Default::default(),
            fx_returns: Default::default(),
            control_connected: connected,
            mix,
            ..engine_row(address, client_id)
        }
    }

    /// A mix with the first channels at `gains` (pans centred), the first groups at `levels`, and
    /// this master volume, mute and phone cap.
    fn mix_of(gains: &[u8], levels: &[u8], volume: u8, muted: bool, cap: u8) -> MixValues {
        let mut mix = MixValues {
            volume_percent: volume,
            muted,
            max_level_percent: cap,
            ..MixValues::default()
        };
        mix.channel_gains[..gains.len()].copy_from_slice(gains);
        mix.pans[..gains.len()].fill(50);
        mix.group_levels[..levels.len()].copy_from_slice(levels);
        mix
    }

    fn saved_mix(gains: &[u8], groups: &[(&str, u8)], volume: u8, muted: bool) -> SessionMix {
        SessionMix {
            channels: gains
                .iter()
                .map(|&gain| SessionPresetChannel {
                    gain,
                    pan: 50,
                    muted: false,
                    solo: false,
                })
                .collect(),
            groups: groups
                .iter()
                .map(|(id, level)| ((*id).to_owned(), *level))
                .collect(),
            volume_percent: volume,
            muted,
        }
    }

    /// A desk's session, a folder of named sessions beside it, and an engine slot.
    fn desk(
        test: &str,
        engine: Option<StandInEngine>,
    ) -> (SessionStore, SnapshotStore, Mutex<Option<StandInEngine>>) {
        let directory = session_directory(test);
        let session = SessionStore::open(&directory);
        let snapshots = SnapshotStore::new(directory.join(SESSIONS_DIRECTORY));
        (session, snapshots, Mutex::new(engine))
    }

    /// The desk's session with a label, groups, channels, controls, a mirror, a preset and an
    /// audio input.
    fn working_session(session: &SessionStore) {
        session.set_label(Some(PHONE), ANA, "Ana").expect("saved");
        session
            .set_groups(vec![SessionGroup {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1],
            }])
            .expect("saved");
        session
            .set_channels(vec![kept("Kick", "drums", false, false)])
            .expect("saved");
        session
            .set_engineer(Some(PHONE), ANA, kept_controls(60, &[1]))
            .expect("saved");
        session
            .set_mirror(
                None,
                RACK,
                Some(kept_mirror(client_key(PHONE), "Ana", Some(ANA), None)),
            )
            .expect("saved");
        session
            .set_audio(SessionAudio {
                host: "wasapi".to_owned(),
                device: Some("INPUT 1/2 (Volt 4)".to_owned()),
                buffer_frames: Some(256),
                asio_driver: None,
            })
            .expect("saved");
    }

    /// Writes `snapshot` straight into the named sessions' folder of `test` as `id`, as a saved
    /// session from an earlier save would be.
    fn saved_session(test: &str, id: &str, snapshot: &crate::sessions::Snapshot) {
        let folder = session_directory_kept(test).join(SESSIONS_DIRECTORY);
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(
            folder.join(format!("{id}.json")),
            serde_json::to_string(snapshot).expect("serializes"),
        )
        .expect("written");
    }

    /// The scratch directory of `test`, as it is (not emptied).
    fn session_directory_kept(test: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join("bandear-command-tests")
            .join(test)
    }

    fn snapshot(session: Session, mixes: &[(&str, SessionMix)]) -> crate::sessions::Snapshot {
        crate::sessions::Snapshot {
            session,
            name: "Show".to_owned(),
            saved_at: Some(1_000),
            mixes: mixes
                .iter()
                .map(|(key, mix)| ((*key).to_owned(), mix.clone()))
                .collect(),
        }
    }

    /// A session with only these groups and channels.
    fn layout_session(groups: &[(&str, &[usize])], channels: usize) -> Session {
        Session {
            groups: groups
                .iter()
                .map(|(id, members)| SessionGroup {
                    id: (*id).to_owned(),
                    name: id.to_uppercase(),
                    channels: members.to_vec(),
                })
                .collect(),
            channels: (0..channels)
                .map(|index| kept(&format!("In {}", index + 1), "other", false, false))
                .collect(),
            ..Session::default()
        }
    }

    fn log_of(engine: &Mutex<Option<StandInEngine>>) -> Vec<String> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .map(|engine| engine.log.lock().expect("log lock").clone())
            .unwrap_or_default()
    }

    /// The controls and mirrors the stand-in took whole last; none when it took none.
    fn kept_books(engine: &Mutex<Option<StandInEngine>>) -> Books {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .and_then(|engine| engine.kept.lock().expect("kept lock").clone())
            .unwrap_or_default()
    }

    fn pushed_to(engine: &Mutex<Option<StandInEngine>>) -> Vec<(String, MixUpdate)> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .map(|engine| engine.pushed.clone())
            .unwrap_or_default()
    }

    #[test]
    fn a_saved_session_keeps_the_whole_desk_session_and_every_musicians_mix_by_key() {
        let (session, snapshots, engine) = desk(
            "session-save",
            Some(StandInEngine {
                inputs: 2,
                live_groups: vec!["kit".to_owned(), "vox".to_owned()],
                here: Mutex::new(vec![
                    here(
                        ANA,
                        Some(PHONE),
                        true,
                        mix_of(&[80, 40, 7], &[70, 30], 85, false, 90),
                    ),
                    here(
                        RACK,
                        None,
                        false,
                        mix_of(&[100, 90], &[100, 55], 60, true, 100),
                    ),
                ]),
                local: mix_of(&[30, 20, 10], &[40, 50], 75, false, 100),
                ..StandInEngine::default()
            }),
        );
        working_session(&session);
        let speakers = crate::session::SessionOutput {
            device: "Speakers (Realtek Audio)".to_owned(),
            first_channel: 0,
        };
        session
            .set_local_output(Some(speakers.clone()))
            .expect("kept");

        let saved = save_session_with(&engine, &session, &snapshots, " Show ").expect("saved");

        assert_eq!(saved.session.name, "Show");
        assert_eq!(saved.session.mixes, 2);
        assert!(saved.engine_running);
        let read = snapshots.read(&saved.session.id).expect("readable");
        assert_eq!(
            read.snapshot.session,
            Session {
                local: Some(SessionLocal {
                    fx_returns: None,
                    limiter: None,
                    mix: Some(saved_mix(&[30, 20], &[("kit", 40), ("vox", 50)], 75, false)),
                    output: Some(speakers),
                }),
                ..session.current()
            },
            "every section the desk keeps, with the local mix the engine plays"
        );
        assert_eq!(
            read.snapshot.mixes,
            BTreeMap::from([
                (
                    client_key(PHONE),
                    saved_mix(&[80, 40], &[("kit", 70), ("vox", 30)], 85, false)
                ),
                (
                    address_key(RACK),
                    saved_mix(&[100, 90], &[("kit", 100), ("vox", 55)], 60, true)
                ),
            ]),
            "the device's inputs, group levels by id, master volume and mute, by the musician's key"
        );
    }

    #[test]
    fn with_no_engine_the_session_is_saved_without_mixes() {
        let (session, snapshots, engine) = desk("session-save-idle", None);
        working_session(&session);

        let saved = save_session_with(&engine, &session, &snapshots, "Rehearsal").expect("saved");

        assert!(!saved.engine_running);
        assert_eq!(saved.session.mixes, 0);
        let read = snapshots.read(&saved.session.id).expect("readable");
        assert_eq!(read.snapshot.session, session.current());
        assert!(read.snapshot.mixes.is_empty());
    }

    #[test]
    fn a_load_applies_groups_and_channels_then_controls_mirrors_buffers_and_redundancy_then_the_mixes(
    ) {
        let test = "session-load-order";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 2,
                here: Mutex::new(vec![
                    here(ANA, Some(PHONE), true, MixValues::default()),
                    here(RACK, None, false, MixValues::default()),
                ]),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[("kit", &[1])], 2);
        loaded.musicians.insert(
            client_key(PHONE),
            crate::session::MusicianEntry {
                fx_returns: None,
                limiter: Some(LimiterSettings {
                    enabled: true,
                    ceiling_db: -6.0,
                    release_ms: 80.0,
                }),
                label: Some("Ana".to_owned()),
                engineer: Some(kept_controls(70, &[2])),
                mirror: None,
                buffer: Some(crate::session::SessionBuffer {
                    mode: "fixed".to_owned(),
                    ms: Some(40),
                }),
                redundancy: Some(crate::session::SessionRedundancy {
                    delay_blocks: Some(3),
                }),
            },
        );
        loaded.musicians.insert(
            address_key(RACK),
            crate::session::MusicianEntry {
                fx_returns: None,
                limiter: None,
                label: Some("Rack".to_owned()),
                engineer: None,
                mirror: Some(kept_mirror(client_key(PHONE), "Ana", Some(ANA), None)),
                buffer: None,
                redundancy: None,
            },
        );
        saved_session(
            test,
            "show",
            &snapshot(
                loaded,
                &[
                    (
                        &client_key(PHONE),
                        saved_mix(&[80], &[("kit", 70)], 85, false),
                    ),
                    (
                        &address_key(RACK),
                        saved_mix(&[10], &[("kit", 10)], 50, true),
                    ),
                ],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        let log = log_of(&engine);
        let at = |step: &str| {
            log.iter()
                .position(|line| line.starts_with(step))
                .unwrap_or_else(|| panic!("no {step} in {log:?}"))
        };
        assert!(at("set_groups") < at("set_channels"), "{log:?}");
        assert!(at("set_channels") < at("replace_kept"), "{log:?}");
        assert!(
            at("replace_kept") < at("replace_playback_buffers"),
            "{log:?}"
        );
        assert!(
            at("replace_playback_buffers") < at("replace_redundancies"),
            "{log:?}"
        );
        assert!(at("replace_redundancies") < at("push_mix"), "{log:?}");
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| engine.buffers.lock().expect("buffers lock").clone()),
            Some(vec![(
                MusicianKey::Client(PHONE.to_owned()),
                PlaybackBuffer::Fixed { ms: 40 }
            )]),
            "the loaded buffers replace the engine's"
        );
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| engine
                    .redundancies
                    .lock()
                    .expect("redundancies lock")
                    .clone()),
            Some(vec![(
                MusicianKey::Client(PHONE.to_owned()),
                Redundancy::On { delay_blocks: 3 }
            )]),
            "the loaded redundancy settings replace the engine's"
        );
        assert!(
            at("replace_redundancies") < at("replace_musician_limiters"),
            "{log:?}"
        );
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| engine.limiters.lock().expect("limiters lock").clone()),
            Some(vec![(
                MusicianKey::Client(PHONE.to_owned()),
                LimiterSettings {
                    enabled: true,
                    ceiling_db: -6.0,
                    release_ms: 80.0,
                }
            )]),
            "the loaded limiters replace the engine's"
        );
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| *engine.local_limiter.lock().expect("local limiter lock")),
            Some(LimiterSettings::master()),
            "no local section saved or kept: the local mix's limiter is off, as the session keeps it"
        );
        let (controls, mirrors) = kept_books(&engine);
        assert_eq!(
            controls,
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                EngineerControls {
                    limit_percent: 70,
                    locked: 0b10,
                }
            )]
        );
        assert_eq!(
            mirrors,
            vec![(
                MusicianKey::Address(RACK.parse().expect("an address")),
                MusicianKey::Client(PHONE.to_owned()),
                "Ana".to_owned()
            )]
        );
        assert_eq!(
            outcome.mixes[1].follows.as_deref(),
            Some("Ana"),
            "the rack follows Ana once the mirrors are in force"
        );
        assert_eq!(
            pushed_to(&engine)[1],
            (
                RACK.to_owned(),
                MixUpdate {
                    volume_percent: Some(50),
                    muted: Some(true),
                    ..MixUpdate::default()
                }
            ),
            "a follower gets only their own master: their channels and groups follow the leader"
        );
    }

    #[test]
    fn a_saved_mix_goes_only_to_musicians_here_by_key_and_the_rest_are_named() {
        let test = "session-load-mixes";
        let moved = "192.168.1.22:50000";
        let away_at = "192.168.1.31:50000";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 3,
                // The groups were reordered and one added since the session was saved.
                live_groups: vec!["vox".to_owned(), "kit".to_owned(), "keys".to_owned()],
                here: Mutex::new(vec![
                    // Ana's phone came back from another IP.
                    here(
                        moved,
                        Some(PHONE),
                        true,
                        mix_of(&[1, 1, 1], &[1, 1, 55], 1, true, 90),
                    ),
                    here(away_at, Some(AWAY_PHONE), false, MixValues::default()),
                    here(RACK, None, false, MixValues::default()),
                    here(BASS_AT, Some(OTHER_PHONE), true, MixValues::default()),
                ]),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[("vox", &[3]), ("kit", &[1]), ("keys", &[2])], 3);
        for (key, label) in [
            (client_key(GONE_PHONE), "Gone"),
            (client_key(AWAY_PHONE), "Away"),
        ] {
            loaded.musicians.insert(
                key,
                crate::session::MusicianEntry {
                    label: Some(label.to_owned()),
                    ..crate::session::MusicianEntry::default()
                },
            );
        }
        saved_session(
            test,
            "show",
            &snapshot(
                loaded,
                &[
                    (
                        &client_key(PHONE),
                        saved_mix(&[80, 40], &[("kit", 70), ("vox", 30)], 85, false),
                    ),
                    (&client_key(AWAY_PHONE), saved_mix(&[5], &[], 5, false)),
                    (&address_key(RACK), saved_mix(&[60], &[], 60, false)),
                    (&client_key(GONE_PHONE), saved_mix(&[9], &[], 9, false)),
                ],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        let pushed = pushed_to(&engine);
        assert_eq!(
            pushed.iter().map(|(at, _)| at.as_str()).collect::<Vec<_>>(),
            vec![moved, RACK],
            "Ana by her phone wherever it is, and the rack that plays its mix with no phone"
        );
        assert_eq!(
            pushed[0].1,
            MixUpdate {
                channel_gains: Some(vec![80, 40]),
                pans: Some(vec![50, 50]),
                channel_muted: Some(vec![false, false]),
                channel_solo: Some(vec![false, false]),
                group_levels: Some(vec![30, 70, 55]),
                volume_percent: Some(85),
                max_level_percent: None,
                muted: Some(false),
            },
            "group levels land on their groups by id, a group the mix does not name keeps its \
             level, and the phone's own cap is never pushed"
        );
        assert_eq!(
            outcome
                .mixes
                .iter()
                .map(|mix| mix.address.as_str())
                .collect::<Vec<_>>(),
            vec![moved, RACK]
        );
        assert_eq!(
            outcome.skipped,
            vec![
                SkippedMixDto {
                    key: client_key(AWAY_PHONE),
                    label: Some("Away".to_owned()),
                    reason: "notConnected",
                    error: None,
                },
                SkippedMixDto {
                    key: client_key(GONE_PHONE),
                    label: Some("Gone".to_owned()),
                    reason: "notHere",
                    error: None,
                },
            ],
            "a phone that is away keeps its own mix for when it connects, and nobody is here for \
             the other"
        );
        assert!(
            !log_of(&engine).iter().any(|line| line.contains(BASS_AT)),
            "a musician with no saved mix is left alone"
        );
    }

    #[test]
    fn a_load_replaces_the_desk_session_and_takes_its_audio_only_as_the_engineer_could_choose_it() {
        let asio_without_driver = SessionAudio {
            host: "asio".to_owned(),
            device: None,
            buffer_frames: Some(128),
            asio_driver: None,
        };
        let asio_picked = SessionAudio {
            asio_driver: Some(" Focusrite USB ASIO ".to_owned()),
            ..asio_without_driver.clone()
        };
        for (case, saved, loaded, skipped) in [
            ("none", None, false, false),
            ("asio-no-driver", Some(asio_without_driver), false, true),
            (
                "bad-host",
                Some(SessionAudio {
                    host: "jack".to_owned(),
                    ..asio_picked.clone()
                }),
                false,
                true,
            ),
            ("asio-picked", Some(asio_picked.clone()), true, false),
        ] {
            let test = format!("session-load-audio-{case}");
            let (session, snapshots, engine) = desk(&test, None);
            working_session(&session);
            let before = session.audio();
            let mut kept = layout_session(&[], 0);
            kept.audio = saved;
            kept.presets = vec![SessionPreset {
                id: "p1".to_owned(),
                name: "Verse".to_owned(),
                mix: SessionPresetMix::default(),
            }];
            saved_session(&test, "show", &snapshot(kept.clone(), &[]));

            let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

            assert_eq!(outcome.audio_loaded, loaded, "{case}");
            assert_eq!(
                outcome.audio_skipped.is_some(),
                skipped,
                "{case}: {outcome:?}"
            );
            let expected = if loaded {
                Some(SessionAudio {
                    asio_driver: Some("Focusrite USB ASIO".to_owned()),
                    ..asio_picked.clone()
                })
            } else {
                before.clone()
            };
            assert_eq!(
                session.audio(),
                expected,
                "{case}: ASIO only with a picked driver"
            );
            assert_eq!(outcome.audio, expected, "{case}");
            assert_eq!(
                session.current(),
                Session {
                    audio: expected,
                    ..kept
                },
                "{case}: the loaded session replaces the desk's, labels and mirrors included"
            );
            assert_eq!(outcome.presets.len(), 1, "{case}");
        }
    }

    #[test]
    fn with_no_engine_a_load_replaces_the_session_and_pushes_no_mix() {
        let test = "session-load-idle";
        let (session, snapshots, engine) = desk(test, None);
        saved_session(
            test,
            "show",
            &snapshot(
                layout_session(&[("kit", &[1])], 1),
                &[(&client_key(PHONE), saved_mix(&[80], &[], 85, false))],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert!(!outcome.applied && outcome.saved, "{outcome:?}");
        assert!(outcome.mixes.is_empty());
        assert_eq!(
            outcome.skipped,
            vec![SkippedMixDto {
                key: client_key(PHONE),
                label: None,
                reason: "notRunning",
                error: None,
            }]
        );
        assert_eq!(
            session.groups().len(),
            1,
            "the groups apply at the next start"
        );
        assert_eq!(outcome.session.name, "Show");
    }

    #[test]
    fn the_recording_settings_stay_the_desks_own_through_a_save_and_a_load() {
        let test = "session-recording";
        let (session, snapshots, engine) = desk(test, None);
        working_session(&session);
        let desks = crate::session::SessionRecording {
            folder: Some("D:\\Takes".to_owned()),
            armed: vec![1, 2],
            ..crate::session::SessionRecording::default()
        };
        session.set_recording(desks.clone()).expect("kept");

        let saved = save_session_with(&engine, &session, &snapshots, "Show").expect("saved");
        let read = snapshots.read(&saved.session.id).expect("readable");
        assert_eq!(
            read.snapshot.session.recording, None,
            "a named session never carries this desk's recording settings"
        );

        let mut other = layout_session(&[], 0);
        other.recording = Some(crate::session::SessionRecording {
            folder: Some("E:\\Elsewhere".to_owned()),
            include_local: true,
            ..crate::session::SessionRecording::default()
        });
        saved_session(test, "other", &snapshot(other, &[]));
        load_into(&engine, &session, &snapshots, "other").expect("loaded");

        assert_eq!(
            session.recording(),
            Some(desks),
            "a load keeps the desk's own recording settings"
        );
    }

    /// A recording settings change does not wait for the engine lock, so it can be saved while a
    /// load is between reading the desk's session and writing the loaded one. The round forces
    /// that overlap: the load is parked right after the engine took its groups, and the change is
    /// saved meanwhile. The load writes the desk's recording settings as the session holds them
    /// then, so the change the engineer was told was kept is kept, also after a restart.
    #[test]
    fn recording_settings_saved_while_a_session_loads_are_kept() {
        let test = "session-recording-concurrent";
        let directory = session_directory(test);
        let session = std::sync::Arc::new(SessionStore::open(&directory));
        let snapshots = SnapshotStore::new(directory.join(SESSIONS_DIRECTORY));
        session
            .set_recording(crate::session::SessionRecording {
                folder: Some("D:\\Before".to_owned()),
                ..crate::session::SessionRecording::default()
            })
            .expect("kept");
        saved_session(test, "show", &snapshot(layout_session(&[], 0), &[]));
        let (parked, parked_signal) = std::sync::mpsc::channel();
        let saved_meanwhile = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let engine = running(StandInEngine {
            park: Some(Park {
                session: std::sync::Arc::clone(&session),
                parked,
                wait: std::time::Duration::from_secs(5),
                saved_meanwhile: std::sync::Arc::clone(&saved_meanwhile),
            }),
            ..StandInEngine::default()
        });
        let during = crate::session::SessionRecording {
            folder: Some("D:\\During".to_owned()),
            include_local: true,
            armed: vec![3],
            ..crate::session::SessionRecording::default()
        };

        std::thread::scope(|scope| {
            let (engine, session, snapshots) = (&engine, &session, &snapshots);
            let loading = scope.spawn(move || load_into(engine, session, snapshots, "show"));
            parked_signal
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the load reached the engine");
            session
                .set_recording(during.clone())
                .expect("kept while the load runs");
            let outcome = loading.join().expect("the load ran").expect("loaded");
            assert!(outcome.applied && outcome.saved);
        });

        assert!(
            saved_meanwhile.load(std::sync::atomic::Ordering::SeqCst),
            "the change was saved while the load was half done"
        );
        assert_eq!(
            session.recording(),
            Some(during.clone()),
            "the load keeps the change saved while it ran"
        );
        assert_eq!(
            SessionStore::open(&directory).recording(),
            Some(during),
            "and so does the session file"
        );
    }

    /// [load_session_with] with no local mix waiting to be kept and the notice set aside.
    fn load_into<E: SessionEngine>(
        engine: &Mutex<Option<E>>,
        session: &SessionStore,
        snapshots: &SnapshotStore,
        id: &str,
    ) -> Result<SessionLoadedDto, String> {
        let saver = LocalMixSaver::manual(|_| Ok(()), |_| {});
        load_session_with(
            engine,
            session,
            snapshots,
            &saver,
            &idle_processing_saver(),
            &Mutex::new(None),
            id,
        )
    }

    #[test]
    fn a_backup_keeps_every_musicians_mix_name_and_controls_by_their_key() {
        let (session, _, engine) = desk(
            "backup-of",
            Some(StandInEngine {
                inputs: 2,
                live_groups: vec!["kit".to_owned(), "vox".to_owned()],
                here: Mutex::new(vec![
                    MusicianStatus {
                        limiter: Default::default(),
                        fx_returns: Default::default(),
                        name: Some("Ana's phone".to_owned()),
                        ..here(
                            ANA,
                            Some(PHONE),
                            true,
                            mix_of(&[80, 40, 7], &[70, 30], 85, false, 90),
                        )
                    },
                    here(
                        RACK,
                        None,
                        false,
                        mix_of(&[100, 90], &[100, 55], 60, true, 100),
                    ),
                ]),
                ..StandInEngine::default()
            }),
        );
        working_session(&session);
        session.set_label(None, RACK, "Rack A").expect("saved");
        let desk_now = engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .map(DeskReader::desk)
            .expect("running");

        let backup = backups::backup_of(&desk_now, &session, 5).expect("two musicians");

        assert_eq!(
            backup,
            crate::backups::MixBackup {
                version: crate::backups::BACKUP_VERSION,
                saved_at: 5,
                musicians: vec![
                    crate::backups::BackedUpMusician {
                        key: client_key(PHONE),
                        name: Some("Ana's phone".to_owned()),
                        client_id: Some(PHONE.to_owned()),
                        address: Some(ANA.to_owned()),
                        engineer: Some(kept_controls(60, &[1])),
                        mix: saved_mix(&[80, 40], &[("kit", 70), ("vox", 30)], 85, false),
                    },
                    crate::backups::BackedUpMusician {
                        key: address_key(RACK),
                        name: Some("Rack A".to_owned()),
                        client_id: None,
                        address: Some(RACK.to_owned()),
                        engineer: None,
                        mix: saved_mix(&[100, 90], &[("kit", 100), ("vox", 55)], 60, true),
                    },
                ],
            },
            "the phone's own name first, else the engineer's label; never the phone's level cap"
        );
        let nobody = DeskView {
            musicians: Vec::new(),
            ..desk_now
        };
        assert_eq!(
            backups::backup_of(&nobody, &session, 5),
            None,
            "nobody to back up"
        );
    }

    #[test]
    fn a_restore_puts_each_mix_back_on_the_musicians_connected_and_skips_the_others() {
        let (session, _, engine) = desk(
            "backup-restore",
            Some(StandInEngine {
                inputs: 2,
                live_groups: vec!["kit".to_owned()],
                here: Mutex::new(vec![
                    here(ANA, Some(PHONE), true, MixValues::default()),
                    here(BASS_AT, Some(AWAY_PHONE), false, MixValues::default()),
                    here(RACK, None, false, MixValues::default()),
                ]),
                ..StandInEngine::default()
            }),
        );
        let musician = |key: String, name: &str, volume: u8| crate::backups::BackedUpMusician {
            key,
            name: Some(name.to_owned()),
            client_id: None,
            address: None,
            engineer: None,
            mix: saved_mix(&[11, 22], &[("kit", 33)], volume, false),
        };
        let backup = crate::backups::MixBackup {
            version: crate::backups::BACKUP_VERSION,
            saved_at: 7,
            musicians: vec![
                musician(client_key(PHONE), "Ana", 81),
                musician(client_key(AWAY_PHONE), "Bass", 82),
                musician(address_key(RACK), "Rack", 83),
                musician(client_key(GONE_PHONE), "Gone", 84),
            ],
        };

        let outcome = backups::restore_with(
            &engine,
            &session.current(),
            "19700101T000000007Z",
            backup.clone(),
        )
        .expect("restored");

        let restored: Vec<&str> = outcome
            .restored
            .iter()
            .map(|mix| mix.address.as_str())
            .collect();
        assert_eq!(
            restored,
            vec![ANA, RACK],
            "a configured target with no phone plays it"
        );
        let pushed: Vec<(String, Option<i32>)> = pushed_to(&engine)
            .into_iter()
            .map(|(address, update)| (address, update.volume_percent))
            .collect();
        assert_eq!(
            pushed,
            vec![(ANA.to_owned(), Some(81)), (RACK.to_owned(), Some(83))],
            "through the desk's path, so each phone is sent its mix"
        );
        let skipped: Vec<(&str, Option<&str>, &str)> = outcome
            .skipped
            .iter()
            .map(|mix| (mix.key.as_str(), mix.label.as_deref(), mix.reason))
            .collect();
        let away = client_key(AWAY_PHONE);
        let gone = client_key(GONE_PHONE);
        assert_eq!(
            skipped,
            vec![
                (away.as_str(), Some("Bass"), "notConnected"),
                (gone.as_str(), Some("Gone"), "notHere"),
            ],
            "skipped and named, never queued"
        );
        assert_eq!(outcome.backup.stamp, "19700101T000000007Z");
        assert_eq!(outcome.backup.musicians, 4);

        let idle: Mutex<Option<StandInEngine>> = Mutex::new(None);
        let refused =
            backups::restore_with(&idle, &session.current(), "x", backup).expect_err("no engine");
        assert!(refused.contains("not running"), "{refused}");
    }

    /// A status of the engine with no musicians and no local output.
    fn engine_status_now() -> personal_monitoring::EngineStatus {
        personal_monitoring::EngineStatus {
            local_limiter: Default::default(),
            local_fx_returns: Default::default(),
            effects: Default::default(),
            device_name: "Test device".to_owned(),
            capture_format: personal_monitoring::CaptureFormat {
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
            channel_levels: vec![0, 0],
            stopped: false,
            samples_received: 0,
            packets_sent: 0,
            packets_discarded: 0,
            musicians: Vec::new(),
            local_mix: MixValues::default(),
            local_output: personal_monitoring::LocalOutputStatus::default(),
            recording: personal_monitoring::RecordingStatus::default(),
            soundcheck: Default::default(),
            demo: false,
            load: Default::default(),
            non_finite_samples: 0,
        }
    }

    #[test]
    fn a_load_says_why_its_output_was_not_opened_on_the_status_and_a_later_choice_clears_it() {
        let test = "session-load-notice";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 1,
                outputs: offering(&[("Speakers (Realtek Audio)", 2)]),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[], 1);
        loaded.local = Some(local_section(Some(("Volt 4", 0))));
        saved_session(test, "show", &snapshot(loaded, &[]));
        let saver = LocalMixSaver::manual(|_| Ok(()), |_| {});
        let notice = Mutex::new(Some("said at start".to_owned()));

        let outcome = load_session_with(
            &engine,
            &session,
            &snapshots,
            &saver,
            &idle_processing_saver(),
            &notice,
            "show",
        )
        .expect("loaded");

        let said = outcome.local_output_notice.expect("said");
        let status = status_dto(&engine_status_now(), 3, &session, &notice);
        assert_eq!(
            status.local_output.notice.as_deref(),
            Some(said.as_str()),
            "the status says what the load said, written before the load returned"
        );
        local::choose_output(
            &engine,
            &session,
            &notice,
            Some(crate::dto::LocalOutputRequestDto {
                device: "Speakers (Realtek Audio)".to_owned(),
                first_channel: 0,
            }),
        )
        .expect("taken");
        let status = status_dto(&engine_status_now(), 3, &session, &notice);
        assert_eq!(status.local_output.notice, None, "a later choice clears it");
    }

    #[test]
    fn a_local_mix_waiting_to_be_kept_stays_with_a_load_that_keeps_the_desks_own_and_goes_with_one_that_replaces_it(
    ) {
        let test = "session-load-pending";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 1,
                ..StandInEngine::default()
            }),
        );
        let waiting = saved_mix(&[33], &[], 44, false);
        let written: Arc<Mutex<Vec<SessionMix>>> = Arc::default();
        let saver = {
            let written = Arc::clone(&written);
            LocalMixSaver::manual(
                move |mix: &SessionMix| {
                    written.lock().expect("written").push(mix.clone());
                    Ok(())
                },
                |_| {},
            )
        };
        let notice = Mutex::new(None);
        saved_session(test, "older", &snapshot(layout_session(&[], 1), &[]));
        let mut with_local = layout_session(&[], 1);
        with_local.local = Some(local_section(None));
        saved_session(test, "newer", &snapshot(with_local.clone(), &[]));

        saver.mark(waiting.clone());
        load_session_with(
            &engine,
            &session,
            &snapshots,
            &saver,
            &idle_processing_saver(),
            &notice,
            "older",
        )
        .expect("loaded");
        assert_eq!(
            session.local().and_then(|local| local.mix),
            Some(waiting.clone()),
            "the desk's own local mix, as it plays, is kept with the loaded session"
        );
        saver.flush();
        assert!(
            written.lock().expect("written").is_empty(),
            "nothing older is written after the load"
        );

        saver.mark(waiting);
        load_session_with(
            &engine,
            &session,
            &snapshots,
            &saver,
            &idle_processing_saver(),
            &notice,
            "newer",
        )
        .expect("loaded");
        saver.flush();
        assert!(written.lock().expect("written").is_empty());
        assert_eq!(
            session.local(),
            with_local.local,
            "the loaded local mix replaced the one waiting"
        );
    }

    /// A loaded session's effects and returns replace the engine's, after its limiters; one saved
    /// without them sets the effects back to their defaults and returns nothing.
    #[test]
    fn a_load_replaces_the_effects_and_every_mixs_returns() {
        let test = "session-load-effects";
        let (session, snapshots, engine) = desk(test, Some(StandInEngine::default()));
        let wet = personal_monitoring::EffectLevels {
            reverb: 30,
            delay: 10,
        };
        let mut loaded = layout_session(&[], 1);
        loaded.effects = Some(personal_monitoring::EffectSettings {
            delay: personal_monitoring::DelaySettings {
                time_ms: 480.0,
                ..personal_monitoring::DelaySettings::default()
            },
            ..personal_monitoring::EffectSettings::default()
        });
        loaded.musicians.insert(
            client_key(PHONE),
            crate::session::MusicianEntry {
                fx_returns: Some(wet),
                ..crate::session::MusicianEntry::default()
            },
        );
        loaded.local = Some(SessionLocal {
            fx_returns: Some(wet),
            ..local_section(None)
        });
        saved_session(test, "wet", &snapshot(loaded, &[]));
        saved_session(test, "dry", &snapshot(layout_session(&[], 1), &[]));

        load_into(&engine, &session, &snapshots, "wet").expect("loaded");
        let log = log_of(&engine);
        let at = |step: &str| log.iter().position(|line| line == step);
        assert!(
            at("replace_musician_limiters") < at("replace_effects"),
            "{log:?}"
        );
        let guard = engine.lock().expect("engine lock");
        let stand_in = guard.as_ref().expect("running");
        assert_eq!(
            stand_in.effects.lock().expect("effects lock").delay.time_ms,
            480.0
        );
        assert_eq!(
            *stand_in.fx_returns.lock().expect("returns lock"),
            Some(vec![(MusicianKey::Client(PHONE.to_owned()), wet)])
        );
        assert_eq!(
            *stand_in.local_fx_returns.lock().expect("returns lock"),
            Some(wet)
        );
        drop(guard);
        assert_eq!(
            session.current().effects.map(|kept| kept.delay.time_ms),
            Some(480.0)
        );

        load_into(&engine, &session, &snapshots, "dry").expect("loaded");
        let guard = engine.lock().expect("engine lock");
        let stand_in = guard.as_ref().expect("running");
        assert_eq!(
            *stand_in.effects.lock().expect("effects lock"),
            personal_monitoring::EffectSettings::default()
        );
        assert_eq!(
            *stand_in.fx_returns.lock().expect("returns lock"),
            Some(Vec::new())
        );
        assert_eq!(
            *stand_in.local_fx_returns.lock().expect("returns lock"),
            Some(wet),
            "no local section saved: the desk's own returns"
        );
    }

    /// The local mix's limiter after a load is always the one the session keeps then: a saved
    /// local section's limiter, the default when the section keeps none, and the desk's own when
    /// the saved session has no local section (the desk keeps its whole local section).
    #[test]
    fn a_load_leaves_the_engine_and_the_session_with_the_same_local_limiter() {
        let test = "session-load-local-limiter";
        let (session, snapshots, engine) = desk(test, Some(StandInEngine::default()));
        let on = |ceiling_db: f32| LimiterSettings {
            enabled: true,
            ceiling_db,
            release_ms: 30.0,
        };
        let local_limiter = |engine: &Mutex<Option<StandInEngine>>| {
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| *engine.local_limiter.lock().expect("local limiter lock"))
        };
        let mut limited = layout_session(&[], 1);
        limited.local = Some(SessionLocal {
            limiter: Some(on(-4.0)),
            ..local_section(None)
        });
        saved_session(test, "limited", &snapshot(limited, &[]));
        let mut unlimited = layout_session(&[], 1);
        unlimited.local = Some(local_section(None));
        saved_session(test, "unlimited", &snapshot(unlimited, &[]));
        saved_session(test, "older", &snapshot(layout_session(&[], 1), &[]));

        load_into(&engine, &session, &snapshots, "limited").expect("loaded");
        assert_eq!(local_limiter(&engine), Some(on(-4.0)), "the saved one");
        assert_eq!(limiter::local_limiter_of(&session.current()), on(-4.0));

        load_into(&engine, &session, &snapshots, "unlimited").expect("loaded");
        assert_eq!(
            local_limiter(&engine),
            Some(LimiterSettings::master()),
            "a local section without a limiter turns it off"
        );
        assert_eq!(
            limiter::local_limiter_of(&session.current()),
            LimiterSettings::master()
        );

        session.set_local_limiter(Some(on(-9.0))).expect("kept");
        load_into(&engine, &session, &snapshots, "older").expect("loaded");
        assert_eq!(
            local_limiter(&engine),
            Some(on(-9.0)),
            "no local section saved: the desk's own, as the session still keeps it"
        );
        assert_eq!(limiter::local_limiter_of(&session.current()), on(-9.0));
    }

    /// A strip with its compressor on at `threshold_db`.
    fn compressed_strip(threshold_db: f32) -> StripSettings {
        StripSettings {
            compressor: personal_monitoring::CompressorSettings {
                enabled: true,
                threshold_db,
                ..personal_monitoring::CompressorSettings::default()
            },
            ..StripSettings::default()
        }
    }

    fn strips_of(engine: &Mutex<Option<StandInEngine>>) -> Vec<StripSettings> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .map(ProcessingEngine::processing)
            .unwrap_or_default()
    }

    #[test]
    fn a_load_replaces_every_strip_after_the_channel_layout_and_drops_a_change_waiting() {
        let test = "session-load-processing";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 2,
                ..StandInEngine::default()
            }),
        );
        let strip = compressed_strip(-24.0);
        let mut loaded = layout_session(&[], 2);
        loaded.processing = BTreeMap::from([(2, strip)]);
        saved_session(test, "show", &snapshot(loaded.clone(), &[]));
        let mine = crate::session::SessionStripPreset {
            name: "Mine".to_owned(),
            settings: strip,
        };
        session.set_strip_presets(vec![mine.clone()]).expect("kept");
        let written: Arc<Mutex<Vec<crate::session::SessionProcessing>>> = Arc::default();
        let processing_saver = {
            let written = Arc::clone(&written);
            ProcessingSaver::manual(
                move |kept: &crate::session::SessionProcessing| {
                    written.lock().expect("written").push(kept.clone());
                    Ok(())
                },
                |_| {},
            )
        };
        processing_saver.mark(BTreeMap::from([(1, compressed_strip(-10.0))]));
        let saver = LocalMixSaver::manual(|_| Ok(()), |_| {});

        let outcome = load_session_with(
            &engine,
            &session,
            &snapshots,
            &saver,
            &processing_saver,
            &Mutex::new(None),
            "show",
        )
        .expect("loaded");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        let log = log_of(&engine);
        let at = |step: &str| {
            log.iter()
                .position(|line| line.starts_with(step))
                .unwrap_or_else(|| panic!("no {step} in {log:?}"))
        };
        assert!(at("set_channels") < at("replace_processing"), "{log:?}");
        let strips = strips_of(&engine);
        assert_eq!(strips.len(), 32, "every channel, CH 1 first");
        assert_eq!(strips[1], strip);
        assert_eq!(
            strips[0],
            StripSettings::default(),
            "CH 1, which the show keeps nothing for, goes back to no processing"
        );
        assert_eq!(session.processing(), BTreeMap::from([(2, strip)]));
        assert_eq!(
            session.strip_presets(),
            vec![mine],
            "the desk's own strip presets stay"
        );
        processing_saver.flush();
        assert!(
            written.lock().expect("written").is_empty(),
            "the change waiting is never written over the loaded processing"
        );

        let idle = "session-load-processing-idle";
        let (session, snapshots, nothing) = desk(idle, None);
        saved_session(idle, "show", &snapshot(loaded, &[]));
        session
            .set_processing(BTreeMap::from([(7, compressed_strip(-5.0))]))
            .expect("kept");
        load_into(&nothing, &session, &snapshots, "show").expect("loaded");
        assert_eq!(
            session.processing(),
            BTreeMap::from([(2, strip)]),
            "with no engine the next start takes the loaded processing"
        );
    }

    /// An engine that refuses the loaded strips keeps playing its own, the change waiting
    /// included: the change is not dropped, so the session ends with what the engine plays.
    #[test]
    fn a_load_whose_strips_the_engine_refuses_keeps_the_strips_it_plays() {
        let test = "session-load-processing-refused";
        let waiting = compressed_strip(-10.0);
        let mut playing = vec![StripSettings::default(); 32];
        playing[0] = waiting;
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 2,
                refuses_processing: Some("the engine is stopping".to_owned()),
                strips: Mutex::new(playing.clone()),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[], 2);
        loaded.processing = BTreeMap::from([(2, compressed_strip(-24.0))]);
        saved_session(test, "show", &snapshot(loaded, &[]));
        let written: Arc<Mutex<Vec<crate::session::SessionProcessing>>> = Arc::default();
        let processing_saver = {
            let written = Arc::clone(&written);
            ProcessingSaver::manual(
                move |kept: &crate::session::SessionProcessing| {
                    written.lock().expect("written").push(kept.clone());
                    Ok(())
                },
                |_| {},
            )
        };
        processing_saver.mark(BTreeMap::from([(1, waiting)]));
        let saver = LocalMixSaver::manual(|_| Ok(()), |_| {});

        let outcome = load_session_with(
            &engine,
            &session,
            &snapshots,
            &saver,
            &processing_saver,
            &Mutex::new(None),
            "show",
        )
        .expect("loaded");

        assert!(
            outcome
                .refused
                .iter()
                .any(|line| line.contains("channel processing was not applied")),
            "{outcome:?}"
        );
        assert_eq!(strips_of(&engine), playing, "the engine keeps its strips");
        processing_saver.flush();
        assert_eq!(
            *written.lock().expect("written"),
            vec![BTreeMap::from([(1, waiting)])],
            "the session is to keep what the engine plays, not the refused strips"
        );
    }

    #[test]
    fn a_saved_session_keeps_the_processing_the_engine_plays_and_never_the_strip_presets() {
        let test = "session-save-processing";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 2,
                ..StandInEngine::default()
            }),
        );
        let strip = compressed_strip(-20.0);
        // Behind the engine: its change is still waiting to be written.
        session
            .set_processing(BTreeMap::from([(5, compressed_strip(-3.0))]))
            .expect("kept");
        session
            .set_strip_presets(vec![crate::session::SessionStripPreset {
                name: "Mine".to_owned(),
                settings: strip,
            }])
            .expect("kept");
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .expect("running")
            .set_channel_processing(0, strip)
            .expect("set");

        let saved = save_session_with(&engine, &session, &snapshots, "Show").expect("saved");

        let read = snapshots.read(&saved.session.id).expect("readable");
        assert_eq!(
            read.snapshot.session.processing,
            BTreeMap::from([(1, strip)]),
            "the engine's, by channel number"
        );
        assert!(read.snapshot.session.strip_presets.is_empty());
        let file: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(
                session_directory_kept(test)
                    .join(SESSIONS_DIRECTORY)
                    .join(format!("{}.json", saved.session.id)),
            )
            .expect("written"),
        )
        .expect("JSON");
        assert_eq!(file["processing"]["1"]["compressor"]["thresholdDb"], -20.0);
        assert!(file.get("stripPresets").is_none(), "{file}");

        let idle = "session-save-processing-idle";
        let (session, snapshots, nothing) = desk(idle, None);
        session
            .set_processing(BTreeMap::from([(5, strip)]))
            .expect("kept");
        let saved = save_session_with(&nothing, &session, &snapshots, "Show").expect("saved");
        assert_eq!(
            snapshots
                .read(&saved.session.id)
                .expect("readable")
                .snapshot
                .session
                .processing,
            BTreeMap::from([(5, strip)]),
            "with no engine, the session's"
        );
    }

    fn local_section(output: Option<(&str, u16)>) -> SessionLocal {
        SessionLocal {
            fx_returns: None,
            limiter: None,
            mix: Some(saved_mix(&[40], &[("kit", 25)], 65, false)),
            output: output.map(|(device, first_channel)| crate::session::SessionOutput {
                device: device.to_owned(),
                first_channel,
            }),
        }
    }

    fn offering(outputs: &[(&str, u16)]) -> Vec<personal_monitoring::OutputDevice> {
        outputs
            .iter()
            .map(|(name, channels)| personal_monitoring::OutputDevice {
                name: (*name).to_owned(),
                channels: *channels,
            })
            .collect()
    }

    #[test]
    fn a_load_pushes_the_saved_local_mix_after_the_musicians_and_opens_its_output_when_offered() {
        let test = "session-load-local";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 1,
                live_groups: vec!["kit".to_owned()],
                here: Mutex::new(vec![here(ANA, Some(PHONE), true, MixValues::default())]),
                outputs: offering(&[("Speakers (Realtek Audio)", 2), ("Volt 4", 4)]),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[("kit", &[1])], 1);
        loaded.local = Some(local_section(Some(("Volt 4", 2))));
        saved_session(
            test,
            "show",
            &snapshot(
                loaded.clone(),
                &[(
                    &client_key(PHONE),
                    saved_mix(&[80], &[("kit", 70)], 85, false),
                )],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert!(outcome.applied && outcome.saved, "{outcome:?}");
        assert!(outcome.local_loaded);
        assert_eq!(outcome.local_output_notice, None);
        assert_eq!(
            outcome.local_mix.as_ref().map(|mix| mix.volume_percent),
            Some(65)
        );
        let log = log_of(&engine);
        let at = |step: &str| {
            log.iter()
                .position(|line| line.starts_with(step))
                .unwrap_or_else(|| panic!("no {step} in {log:?}"))
        };
        assert!(at("push_mix") < at("set_local_mix"), "{log:?}");
        assert!(
            at("set_local_mix") < at("set_local_output Volt 4"),
            "{log:?}"
        );
        let pushed = engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .map(|engine| engine.local_pushed.clone())
            .unwrap_or_default();
        assert_eq!(
            pushed,
            vec![MixUpdate {
                channel_gains: Some(vec![40]),
                pans: Some(vec![50]),
                channel_muted: Some(vec![false]),
                channel_solo: Some(vec![false]),
                group_levels: Some(vec![25]),
                volume_percent: Some(65),
                max_level_percent: None,
                muted: Some(false),
            }]
        );
        assert_eq!(
            session.local(),
            loaded.local,
            "the desk keeps the loaded one"
        );
    }

    #[test]
    fn a_loaded_output_the_running_driver_does_not_offer_is_left_off_and_said() {
        let test = "session-load-local-unoffered";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 1,
                // Under ASIO only the capture driver's own outputs are offered.
                outputs: offering(&[("Volt 4 ASIO", 4)]),
                output: personal_monitoring::LocalOutputStatus {
                    output: Some(personal_monitoring::LocalOutputRequest {
                        device: "Volt 4 ASIO".to_owned(),
                        first_channel: 0,
                    }),
                    state: personal_monitoring::LocalOutputState::Running,
                    ..personal_monitoring::LocalOutputStatus::default()
                },
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[], 1);
        loaded.local = Some(local_section(Some(("OUTPUT 1/2 (Volt 4)", 0))));
        saved_session(test, "show", &snapshot(loaded.clone(), &[]));

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        let notice = outcome.local_output_notice.expect("said");
        assert!(
            notice.contains("OUTPUT 1/2 (Volt 4) · outputs 1/2"),
            "{notice}"
        );
        let log = log_of(&engine);
        assert!(
            !log.iter()
                .any(|line| line == "set_local_output OUTPUT 1/2 (Volt 4)"),
            "{log:?}"
        );
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .map(|engine| engine.output.output.clone()),
            Some(None),
            "the output that played stops: never another device in its place"
        );
        assert_eq!(
            session.local(),
            loaded.local,
            "the choice is kept for a driver that offers it"
        );
    }

    #[test]
    fn a_saved_session_from_before_the_local_mix_leaves_the_desks_own() {
        let test = "session-load-no-local";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                inputs: 1,
                ..StandInEngine::default()
            }),
        );
        let own = local_section(Some(("Speakers (Realtek Audio)", 0)));
        session
            .set_local_mix(own.mix.clone().expect("a mix"))
            .expect("kept");
        session.set_local_output(own.output.clone()).expect("kept");
        saved_session(test, "show", &snapshot(layout_session(&[], 1), &[]));

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert!(!outcome.local_loaded);
        assert!(outcome.local_mix.is_none());
        assert_eq!(session.local(), Some(own), "the desk's own stays");
        let log = log_of(&engine);
        assert!(
            !log.iter().any(|line| line.contains("local")
                && line != "set_local_limiter"
                && line != "replace_local_fx_returns"),
            "the engine's local mix and output are left alone: {log:?}"
        );
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .and_then(|engine| *engine.local_limiter.lock().expect("local limiter lock")),
            Some(limiter::local_limiter_of(&session.current())),
            "the local mix's limiter is the one the desk's own section keeps"
        );

        let (idle_session, idle_snapshots, idle) = desk("session-load-local-idle", None);
        let mut loaded = layout_session(&[], 1);
        loaded.local = Some(local_section(None));
        saved_session(
            "session-load-local-idle",
            "show",
            &snapshot(loaded.clone(), &[]),
        );
        let outcome = load_into(&idle, &idle_session, &idle_snapshots, "show").expect("loaded");
        assert!(outcome.local_loaded && !outcome.applied);
        assert!(outcome.local_mix.is_none(), "nothing plays it yet");
        assert_eq!(
            idle_session.local(),
            loaded.local,
            "kept for the next start"
        );
    }

    #[test]
    fn a_saved_session_the_engine_would_refuse_is_not_loaded_and_nothing_changes() {
        let test = "session-load-refused";
        let (session, snapshots, engine) = desk(test, Some(StandInEngine::default()));
        working_session(&session);
        let before = session.current();
        let mut overlapping = layout_session(&[("kit", &[1, 2]), ("vox", &[2])], 2);
        saved_session(test, "groups", &snapshot(overlapping.clone(), &[]));
        overlapping.groups.clear();
        overlapping.channels[1].link = true;
        saved_session(test, "channels", &snapshot(overlapping, &[]));

        for (id, says) in [("groups", "groups"), ("channels", "channel details")] {
            let error = load_into(&engine, &session, &snapshots, id).expect_err("refused");
            assert!(error.contains(says), "{id}: {error}");
        }

        assert_eq!(session.current(), before, "the desk's session is unchanged");
        assert_eq!(
            log_of(&engine),
            Vec::<String>::new(),
            "the engine took nothing"
        );
    }

    #[test]
    fn a_load_over_a_session_the_desk_must_not_replace_is_refused_before_the_engine_takes_anything()
    {
        let test = "session-load-newer";
        let directory = session_directory(test);
        std::fs::create_dir_all(&directory).expect("directory");
        let newer = r#"{"version":99}"#;
        std::fs::write(directory.join(crate::session::SESSION_FILE), newer).expect("written");
        let session = SessionStore::open(&directory);
        let snapshots = SnapshotStore::new(directory.join(SESSIONS_DIRECTORY));
        let engine = running(StandInEngine::default());
        saved_session(
            test,
            "show",
            &snapshot(layout_session(&[("kit", &[1])], 1), &[]),
        );

        let error = load_into(&engine, &session, &snapshots, "show").expect_err("refused");

        assert!(error.contains("newer"), "{error}");
        assert_eq!(
            log_of(&engine),
            Vec::<String>::new(),
            "the engine took nothing"
        );
        assert_eq!(
            std::fs::read_to_string(directory.join(crate::session::SESSION_FILE)).expect("kept"),
            newer
        );
    }

    #[test]
    fn a_stopping_engine_refuses_a_load_and_nothing_is_saved() {
        let test = "session-load-stopping";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                stopping: true,
                ..StandInEngine::default()
            }),
        );
        working_session(&session);
        let before = session.current();
        saved_session(
            test,
            "show",
            &snapshot(layout_session(&[("kit", &[1])], 1), &[]),
        );

        let error = load_into(&engine, &session, &snapshots, "show").expect_err("refused");

        assert!(error.contains("stopping"), "{error}");
        assert_eq!(session.current(), before);
    }

    #[test]
    fn a_load_replaces_the_engines_kept_controls_and_mirrors_with_the_loaded_ones() {
        let test = "session-load-replace-kept";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                here: Mutex::new(vec![MusicianStatus {
                    limiter: Default::default(),
                    fx_returns: Default::default(),
                    capabilities: Vec::new(),
                    mirror: Some(personal_monitoring::MirrorStatus {
                        leader: MusicianKey::Address(RACK.parse().expect("an address")),
                        name: "Rack".to_owned(),
                        leader_address: Some(RACK.parse().expect("an address")),
                    }),
                    ..here(ANA, Some(PHONE), true, MixValues::default())
                }]),
                ..StandInEngine::default()
            }),
        );
        // The desk kept Ana's controls and mirror by her phone, and a lock for a phone not here.
        session
            .set_engineer(Some(PHONE), ANA, kept_controls(60, &[1]))
            .expect("saved");
        session
            .set_engineer(Some(OTHER_PHONE), BASS_AT, kept_controls(100, &[2]))
            .expect("saved");
        session
            .set_mirror(
                Some(PHONE),
                ANA,
                Some(kept_mirror(address_key(RACK), "Rack", None, Some(ANA))),
            )
            .expect("saved");
        // The loaded session keeps controls for Ana's address only, and no mirror.
        let mut loaded = layout_session(&[], 0);
        loaded.musicians.insert(
            address_key(ANA),
            crate::session::MusicianEntry {
                engineer: Some(kept_controls(80, &[])),
                ..crate::session::MusicianEntry::default()
            },
        );
        saved_session(test, "show", &snapshot(loaded, &[]));

        load_into(&engine, &session, &snapshots, "show").expect("loaded");

        let (controls, mirrors) = kept_books(&engine);
        assert_eq!(
            controls,
            vec![(
                MusicianKey::Address(ANA.parse().expect("an address")),
                EngineerControls {
                    limit_percent: 80,
                    locked: 0,
                }
            )],
            "only the loaded controls are kept: the phones' old ones, here or away, are gone"
        );
        assert_eq!(mirrors, Vec::new(), "and no mirror");
        assert_eq!(
            engine
                .lock()
                .expect("engine lock")
                .as_ref()
                .expect("running")
                .desk()
                .musicians[0]
                .mirror,
            None,
            "Ana is freed"
        );
        assert_eq!(
            session.engineer_for(Some(PHONE), ANA),
            Some(kept_controls(80, &[]))
        );
        assert_eq!(session.engineer_for(Some(OTHER_PHONE), BASS_AT), None);
        assert_eq!(session.mirror_for(Some(PHONE), ANA), None);
    }

    #[test]
    fn a_mirror_the_engine_refuses_is_named_and_the_rest_of_the_load_goes_on() {
        let test = "session-load-mirror-refused";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                here: Mutex::new(vec![here(RACK, None, false, MixValues::default())]),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[], 1);
        loaded.musicians.insert(
            address_key(RACK),
            crate::session::MusicianEntry {
                fx_returns: None,
                limiter: None,
                label: Some("Rack".to_owned()),
                engineer: None,
                // A hand-edited session: a mirror on oneself, which the engine refuses.
                mirror: Some(kept_mirror(address_key(RACK), "Rack", None, None)),
                buffer: None,
                redundancy: None,
            },
        );
        saved_session(
            test,
            "show",
            &snapshot(
                loaded,
                &[(&address_key(RACK), saved_mix(&[60], &[], 60, false))],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert_eq!(outcome.refused.len(), 1, "{:?}", outcome.refused);
        assert!(
            outcome.refused[0].starts_with("a saved mirror was not applied: ")
                && outcome.refused[0].contains("own mix"),
            "{:?}",
            outcome.refused
        );
        assert_eq!(
            outcome.mixes.len(),
            1,
            "the rack's mix is pushed all the same"
        );
        assert!(outcome.saved);
    }

    #[test]
    fn controls_and_mirrors_the_engine_refuses_are_named_and_the_mixes_still_go_out() {
        let test = "session-load-kept-refused";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                here: Mutex::new(vec![here(RACK, None, false, MixValues::default())]),
                refuses_kept: Some("the engine is stopping".to_owned()),
                ..StandInEngine::default()
            }),
        );
        saved_session(
            test,
            "show",
            &snapshot(
                layout_session(&[], 1),
                &[(&address_key(RACK), saved_mix(&[60], &[], 60, false))],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert_eq!(
            outcome.refused,
            vec![
                "the saved limits, locks and mirrors were not applied: the engine is stopping"
                    .to_owned()
            ]
        );
        assert_eq!(outcome.mixes.len(), 1);
        assert!(
            outcome.saved,
            "the desk keeps the loaded session; a start applies it"
        );
    }

    #[test]
    fn a_saved_mix_the_engine_refuses_is_named_with_its_reason_and_the_others_go_out() {
        let test = "session-load-push-refused";
        let (session, snapshots, engine) = desk(
            test,
            Some(StandInEngine {
                here: Mutex::new(vec![
                    here(RACK, None, false, MixValues::default()),
                    here(ANA, Some(PHONE), true, MixValues::default()),
                ]),
                refuses_push_to: Some(RACK.to_owned()),
                ..StandInEngine::default()
            }),
        );
        let mut loaded = layout_session(&[], 1);
        loaded.musicians.insert(
            address_key(RACK),
            crate::session::MusicianEntry {
                label: Some("Rack".to_owned()),
                ..crate::session::MusicianEntry::default()
            },
        );
        saved_session(
            test,
            "show",
            &snapshot(
                loaded,
                &[
                    (&address_key(RACK), saved_mix(&[60], &[], 60, false)),
                    (&client_key(PHONE), saved_mix(&[70], &[], 70, false)),
                ],
            ),
        );

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert_eq!(
            outcome.skipped,
            vec![SkippedMixDto {
                key: address_key(RACK),
                label: Some("Rack".to_owned()),
                reason: "refused",
                error: Some(format!("target `{RACK}` is not configured")),
            }]
        );
        assert_eq!(
            outcome
                .mixes
                .iter()
                .map(|mix| mix.address.as_str())
                .collect::<Vec<_>>(),
            vec![ANA],
            "Ana's mix goes out all the same"
        );
        assert!(outcome.saved);
    }

    #[test]
    fn a_damaged_saved_session_loads_what_it_can_and_names_the_rest() {
        let test = "session-load-damaged";
        let (session, snapshots, engine) = desk(test, None);
        let folder = session_directory_kept(test).join(SESSIONS_DIRECTORY);
        std::fs::create_dir_all(&folder).expect("folder");
        std::fs::write(
            folder.join("show.json"),
            format!(
                r#"{{"version":1,"name":"Show","musicians":{{"client:{PHONE}":{{"label":"Ana"}}}},"groups":7,"mixes":{{"client:{PHONE}":{{"channels":"loud"}}}}}}"#
            ),
        )
        .expect("written");

        let outcome = load_into(&engine, &session, &snapshots, "show").expect("loaded");

        assert_eq!(outcome.damaged.len(), 2, "{:?}", outcome.damaged);
        assert_eq!(session.label_for(Some(PHONE), ANA), Some("Ana".to_owned()));
        assert!(session.groups().is_empty());
        assert!(outcome.skipped.is_empty(), "the unreadable mix is no mix");
    }
}
