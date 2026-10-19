//! Recording takes from the desk: the `recording_settings`, `set_recording_settings`,
//! `start_recording`, `stop_recording`, `add_marker`, `record_estimate`, `list_takes` and
//! `open_take_folder` commands over the engine's recorder ([EngineHandle::start_recording] and the
//! rest).
//!
//! How this desk records is kept in the session's `recording` section
//! ([crate::session::SessionRecording]): the folder take folders are made in, the format (24-bit
//! integer by default, or 32-bit float), whether the local mix is recorded too, and the armed
//! inputs by their 1-based number. The settings can be changed at any time, engine or not, and a
//! take that records keeps what it started with. The default folder is the user's Music folder +
//! `MixLink\Recordings`; when the Music folder cannot be found the desk falls back to its own data
//! folder and the settings say so ([RecordingsFolder]). No recordings folder, the default included,
//! is ever inside the app's own config folder. A folder is created when the first take is recorded
//! in it.
//!
//! A start arms the inputs the settings arm that the running device has (an armed input past its
//! inputs is left out and named in the answer); the engine records either side of a linked pair as
//! the pair. Nothing armed and no local mix is refused before anything is made. A stop answers the
//! finished take; when its files are still being finished after the engine's wait, the answer says
//! so (`finishing`) and the window asks again. The engine refuses a device switch while a take
//! records.
//!
//! The takes are read back from the folder: each direct subfolder with a `take.json` is a take. A
//! file that cannot be read is left out and counted; a take whose `take.json` never got its end (the
//! app or the computer stopped while it recorded) is listed as interrupted, with the frames its
//! files hold, and the take being recorded now as recording. `open_take_folder` opens a take folder
//! in the system's file manager, and only a take folder directly inside the recordings folder: the
//! path is checked on the disk first and the file manager is started directly, never through a
//! shell.

use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use personal_monitoring::{
    free_space, DiskEstimate, EngineHandle, RecordRequest, RecordingState, RecordingStatus,
    SampleFormat, TakeMarker, TakeSummary, WavReader,
};
use serde::Deserialize;

use super::DesktopState;
use crate::dto::{
    DiskEstimateDto, RecordingSettingsDto, RecordingSettingsRequest, RecordingStartedDto,
    RecordingStoppedDto, TakeListItemDto, TakeMarkerDto, TakeTrackDto, TakesDto,
};
use crate::session::{RecordFormat, SessionRecording, SessionStore};

/// The file that describes a take inside its folder.
pub const TAKE_FILE: &str = "take.json";

/// The name a take gets when the engineer gives none.
pub const DEFAULT_TAKE_NAME: &str = "Take";

/// The highest input number that can be armed: the engine's inputs are numbered by a byte.
const MAX_INPUT: u32 = 255;

/// The farthest a time zone is from UTC, in minutes (UTC+14).
const MAX_UTC_OFFSET_MINUTES: i32 = 14 * 60;

/// The rate every take is recorded at.
const TAKE_SAMPLE_RATE: u32 = 48_000;

/// The desk's default recordings folder, and why it is not in the Music folder when it is not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordingsFolder {
    pub folder: PathBuf,
    pub notice: Option<String>,
}

/// The default recordings folder: `music` (the user's Music folder) + `MixLink\Recordings`; when it
/// is not known, `fallback` (the app's own data folder) + `Recordings`, and when neither is, the
/// system's temporary folder + `MixLink\Recordings`, each saying so. Like any recordings folder it
/// is never inside `config`, the app's own config folder (the app's data folder is that folder on
/// macOS): a place inside it gives way to the next, and when even the temporary folder is inside
/// it, the folder is `MixLink Recordings` beside it. Never fails.
pub fn default_recordings_folder(
    music: Option<PathBuf>,
    fallback: Option<PathBuf>,
    config: &Path,
) -> RecordingsFolder {
    let outside = |folder: &PathBuf| !inside(folder, config);
    let music_folder = music.map(|music| music.join("MixLink").join("Recordings"));
    if let Some(folder) = music_folder.clone().filter(outside) {
        return RecordingsFolder {
            folder,
            notice: None,
        };
    }
    let data_folder = fallback.map(|data| data.join("Recordings"));
    let folder = data_folder
        .clone()
        .filter(outside)
        .or_else(|| Some(std::env::temp_dir().join("MixLink").join("Recordings")).filter(outside))
        .unwrap_or_else(|| beside(config));
    let mut why = if music_folder.is_some() {
        "Your Music folder is inside MixLink's own settings folder".to_owned()
    } else {
        "Your Music folder could not be found".to_owned()
    };
    if data_folder.is_some_and(|data| !outside(&data)) {
        why.push_str(" and MixLink's data folder is inside its settings folder");
    }
    RecordingsFolder {
        notice: Some(format!(
            "{why}, so takes are recorded in {} unless you choose another folder.",
            folder.display()
        )),
        folder,
    }
}

/// `MixLink Recordings` beside the `config` folder, which is never inside it (but for a config
/// folder that is a whole drive, which has nothing beside it).
fn beside(config: &Path) -> PathBuf {
    config.parent().unwrap_or(config).join("MixLink Recordings")
}

/// Where recording looks for its folders: the default recordings folder and the app's own config
/// folder, which no recordings folder may be in.
#[derive(Clone, Copy, Debug)]
pub(super) struct Places<'a> {
    pub(super) default: &'a RecordingsFolder,
    pub(super) config: &'a Path,
}

/// What recording needs from the running engine: the engine in the app, a stand-in in the tests,
/// which run without audio hardware.
pub(super) trait RecordEngine {
    /// The inputs the running device has.
    fn inputs(&self) -> usize;
    fn start_recording(&mut self, request: RecordRequest) -> Result<RecordingStatus, String>;
    fn stop_recording(&mut self) -> Result<TakeSummary, String>;
    fn add_marker(&self, name: &str) -> Result<TakeMarker, String>;
    fn recording(&self) -> RecordingStatus;
    fn record_estimate(&self, request: &RecordRequest) -> Result<DiskEstimate, String>;
}

impl RecordEngine for EngineHandle {
    fn inputs(&self) -> usize {
        usize::from(self.status().source_channels)
    }

    fn start_recording(&mut self, request: RecordRequest) -> Result<RecordingStatus, String> {
        // A soundcheck playing refuses a take; it is said as the soundcheck commands say it.
        EngineHandle::start_recording(self, request)
            .map_err(|error| super::soundcheck::words(&*error))
    }

    fn stop_recording(&mut self) -> Result<TakeSummary, String> {
        EngineHandle::stop_recording(self).map_err(|error| error.to_string())
    }

    fn add_marker(&self, name: &str) -> Result<TakeMarker, String> {
        EngineHandle::add_marker(self, name).map_err(|error| error.to_string())
    }

    fn recording(&self) -> RecordingStatus {
        EngineHandle::recording(self)
    }

    fn record_estimate(&self, request: &RecordRequest) -> Result<DiskEstimate, String> {
        EngineHandle::record_estimate(self, request).map_err(|error| error.to_string())
    }
}

/// Opens a folder in the system's file manager: the system's in the app, a recorder in the tests,
/// which never start anything.
pub trait FolderOpener: Send + Sync {
    fn open(&self, folder: &Path) -> Result<(), String>;
}

/// The system's file manager: Explorer on Windows, `open` on macOS, `xdg-open` elsewhere, started
/// directly with the folder as its one argument, never through a shell.
pub struct SystemFileManager;

impl FolderOpener for SystemFileManager {
    fn open(&self, folder: &Path) -> Result<(), String> {
        let mut command = file_manager(folder);
        command
            .spawn()
            .map(drop)
            .map_err(|error| format!("the file manager could not be started: {error}"))
    }
}

#[cfg(windows)]
fn file_manager(folder: &Path) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    // Explorer reads its own command line and treats a comma as a separator, so the folder is
    // passed whole between quotes; a Windows path never holds a quote.
    let mut quoted = OsString::from("\"");
    quoted.push(folder.as_os_str());
    quoted.push("\"");
    let mut command = std::process::Command::new("explorer.exe");
    command.raw_arg(quoted);
    command
}

#[cfg(target_os = "macos")]
fn file_manager(folder: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("open");
    command.arg(OsString::from(folder.as_os_str()));
    command
}

#[cfg(not(any(windows, target_os = "macos")))]
fn file_manager(folder: &Path) -> std::process::Command {
    let mut command = std::process::Command::new("xdg-open");
    command.arg(OsString::from(folder.as_os_str()));
    command
}

pub(super) fn places(state: &DesktopState) -> Places<'_> {
    Places {
        default: &state.recordings,
        config: state.session.directory(),
    }
}

/// How this desk records takes: the folder (the kept one or the default, whole), the default
/// folder, the format, whether the local mix is recorded too and the armed inputs (1-based).
pub fn recording_settings(state: &DesktopState) -> Result<RecordingSettingsDto, String> {
    Ok(settings_of(&state.session, places(state)))
}

/// Keeps how this desk records takes and answers them as kept (see [recording_settings]).
///
/// `folder` must be a whole path (`D:\Recordings`), must not go up with `..` and must not be inside
/// the app's own config folder; null or blank is the default folder (and so is the default folder
/// typed out). `format` is `int24` (the default) or `float32`. Armed inputs are numbered from 1 and
/// kept in order, each once. A refused setting, or a save that fails, changes nothing. A take that
/// records keeps what it started with.
pub fn set_recording_settings(
    state: &DesktopState,
    settings: RecordingSettingsRequest,
) -> Result<RecordingSettingsDto, String> {
    let answer = keep_settings(&state.session, places(state), settings)?;
    println!(
        "[mixlink-desktop] set_recording_settings: folder=\"{}\" format={} include_local={} armed={:?}",
        answer.folder, answer.format, answer.include_local, answer.armed
    );
    Ok(answer)
}

/// Starts recording a take called `take_name` (blank: "Take") with the kept settings, the folder's
/// name in local time by `utc_offset_minutes` (the window's offset from UTC; none is UTC). An error,
/// and nothing recorded, with no engine running, a take already recording, nothing armed on the
/// running device and no local mix, a folder that cannot be created, or the engine's refusal.
pub fn start_recording(
    state: &DesktopState,
    take_name: String,
    utc_offset_minutes: Option<i32>,
) -> Result<RecordingStartedDto, String> {
    let started = start_with(
        &state.engine,
        &state.session,
        places(state),
        &take_name,
        utc_offset_minutes,
    )?;
    println!(
        "[mixlink-desktop] start_recording: folder=\"{}\" armed={:?} skipped={:?} include_local={}",
        started.recording.take_folder.as_deref().unwrap_or_default(),
        started.armed,
        started.skipped,
        started.include_local
    );
    Ok(started)
}

/// Stops the take and answers it once its files are finished, or `finishing` when they are still
/// being finished after the engine's wait (ask again). An error with no engine or no take.
///
/// The engine needs its handle whole to stop a take, so the engine lock is held while it finishes
/// the files: a few milliseconds on a healthy disk, at most the engine's wait (10 s) on one that
/// has stalled, during which the meters and the status wait too.
pub fn stop_recording(state: &DesktopState) -> Result<RecordingStoppedDto, String> {
    let stopped = stop_with(&state.engine)?;
    match &stopped.take {
        Some(take) => println!(
            "[mixlink-desktop] stop_recording: \"{}\" {} frames, ended {}",
            take.folder, take.frames, take.ended
        ),
        None => println!("[mixlink-desktop] stop_recording: the files are still being finished"),
    }
    Ok(stopped)
}

/// Marks the take being recorded with `name` (blank: the next free "Marker n") at the block being
/// recorded, and answers the marker. An error with no engine or no take recording.
pub fn add_marker(state: &DesktopState, name: String) -> Result<TakeMarkerDto, String> {
    marker_with(&state.engine, &name)
}

/// What a take with the kept settings would take on the disk: its bytes per second (with an engine
/// running and something armed; else `problem` says why not), the free space on the folder's disk
/// and the minutes it holds.
pub fn record_estimate(state: &DesktopState) -> Result<DiskEstimateDto, String> {
    estimate_with(&state.engine, &state.session, places(state))
}

/// The takes in the recordings folder, the newest first (see the module documentation).
pub fn list_takes(state: &DesktopState) -> Result<TakesDto, String> {
    list_with(&state.engine, &state.session, places(state))
}

/// Opens the take folder `folder` in the system's file manager: only a folder with a `take.json`
/// directly inside the recordings folder, checked on the disk first.
pub fn open_take_folder(state: &DesktopState, folder: String) -> Result<(), String> {
    open_with(
        &state.session,
        places(state),
        state.folder_opener.as_ref(),
        &folder,
    )
}

/// The folder takes are made in: the kept one, or the default.
fn folder_of(kept: &SessionRecording, default: &RecordingsFolder) -> PathBuf {
    kept.folder
        .as_deref()
        .map_or_else(|| default.folder.clone(), PathBuf::from)
}

/// The engine's sample format for a kept one.
fn sample_format(format: RecordFormat) -> SampleFormat {
    match format {
        RecordFormat::Int24 => SampleFormat::Int24,
        RecordFormat::Float32 => SampleFormat::Float32,
    }
}

/// The settings as the window shows them.
fn settings_of(session: &SessionStore, places: Places<'_>) -> RecordingSettingsDto {
    answer_of(&session.recording().unwrap_or_default(), places.default)
}

fn answer_of(kept: &SessionRecording, default: &RecordingsFolder) -> RecordingSettingsDto {
    RecordingSettingsDto {
        folder: folder_of(kept, default).display().to_string(),
        default_folder: default.folder.display().to_string(),
        folder_is_default: kept.folder.is_none(),
        format: kept.format.name(),
        include_local: kept.include_local,
        armed: kept.armed.clone(),
        notice: default.notice.clone(),
    }
}

/// [set_recording_settings] over any session and places.
fn keep_settings(
    session: &SessionStore,
    places: Places<'_>,
    settings: RecordingSettingsRequest,
) -> Result<RecordingSettingsDto, String> {
    let format = match settings.format.as_deref().map(str::trim) {
        None | Some("") => RecordFormat::default(),
        Some(name) => RecordFormat::parse(name)
            .ok_or_else(|| format!("the format `{name}` is none of int24, float32"))?,
    };
    let mut armed = Vec::with_capacity(settings.armed.len());
    for input in settings.armed {
        if !(1..=MAX_INPUT).contains(&input) {
            return Err(format!(
                "CH {input} cannot be armed: inputs are numbered from 1 to {MAX_INPUT}"
            ));
        }
        armed.push(u16::try_from(input).unwrap_or(u16::MAX));
    }
    armed.sort_unstable();
    armed.dedup();
    let kept = SessionRecording {
        folder: kept_folder(settings.folder.as_deref(), places)?,
        format,
        include_local: settings.include_local,
        armed,
    };
    session.set_recording(kept.clone())?;
    Ok(answer_of(&kept, places.default))
}

/// The folder a setting keeps: `None` for the default (blank, or the default typed out), or the
/// folder as typed, trimmed, when it may hold takes.
fn kept_folder(folder: Option<&str>, places: Places<'_>) -> Result<Option<String>, String> {
    let Some(text) = folder.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let path = Path::new(text);
    if !path.is_absolute() {
        return Err(format!(
            "the recordings folder must be a whole path, such as D:\\Recordings, and `{text}` is not"
        ));
    }
    if path
        .components()
        .any(|component| component == Component::ParentDir)
    {
        return Err(format!(
            "the recordings folder must not go up with `..`: `{text}`"
        ));
    }
    // Before the default is matched, so no folder inside the config folder is ever kept, whatever
    // the default is.
    if inside(path, places.config) {
        return Err(format!(
            "the recordings folder cannot be in MixLink's own settings folder ({}): `{text}`",
            places.config.display()
        ));
    }
    if same_path(path, &places.default.folder) {
        return Ok(None);
    }
    Ok(Some(text.to_owned()))
}

/// A path component as compared: in any case on Windows, whose paths ignore it.
fn component_key(component: Component<'_>) -> OsString {
    let text = component.as_os_str();
    if cfg!(windows) {
        OsString::from(text.to_string_lossy().to_lowercase())
    } else {
        text.to_owned()
    }
}

/// Whether `path` is `folder` or inside it, comparing their components as written.
fn inside(path: &Path, folder: &Path) -> bool {
    let mut parts = path.components().map(component_key);
    folder
        .components()
        .map(component_key)
        .all(|part| parts.next() == Some(part))
}

/// Whether `one` and `other` name the same folder, as written.
fn same_path(one: &Path, other: &Path) -> bool {
    inside(one, other) && inside(other, one)
}

/// The armed inputs the running device has, and the ones it does not, 1-based.
fn split_armed(armed: &[u16], inputs: usize) -> (Vec<u16>, Vec<u16>) {
    armed
        .iter()
        .copied()
        .partition(|input| usize::from(*input) <= inputs)
}

/// The engine's request for a take of `kept` in `folder` recording `armed` (1-based), unnamed and
/// in UTC until the caller says otherwise.
fn request_of(kept: &SessionRecording, folder: PathBuf, armed: &[u16]) -> RecordRequest {
    RecordRequest {
        folder,
        take_name: String::new(),
        format: sample_format(kept.format),
        armed: armed
            .iter()
            .map(|input| usize::from(*input).saturating_sub(1))
            .collect(),
        include_local: kept.include_local,
        utc_offset_minutes: 0,
    }
}

/// `inputs` as the window numbers them: "CH 7, CH 8".
fn channel_list(inputs: &[u16]) -> String {
    inputs
        .iter()
        .map(|input| format!("CH {input}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn engine_lock<E>(
    engine: &Mutex<Option<E>>,
) -> Result<std::sync::MutexGuard<'_, Option<E>>, String> {
    engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())
}

/// [start_recording] over any engine slot, session and places.
fn start_with<E: RecordEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    places: Places<'_>,
    take_name: &str,
    utc_offset_minutes: Option<i32>,
) -> Result<RecordingStartedDto, String> {
    let utc_offset_minutes = utc_offset_minutes.unwrap_or(0);
    if utc_offset_minutes.abs() > MAX_UTC_OFFSET_MINUTES {
        return Err(format!(
            "the time zone offset of {utc_offset_minutes} minutes is more than 14 hours"
        ));
    }
    // The lock order every command that holds both follows: the engine's, then the session's.
    let mut guard = engine_lock(engine)?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| "the engine is not running: start it to record".to_owned())?;
    if matches!(
        engine.recording().state,
        RecordingState::Recording | RecordingState::Stopping
    ) {
        return Err("a take is already recording: stop it first".to_owned());
    }
    let kept = session.recording().unwrap_or_default();
    let inputs = engine.inputs();
    let (armed, skipped) = split_armed(&kept.armed, inputs);
    if armed.is_empty() && !kept.include_local {
        return Err(if skipped.is_empty() {
            "nothing is armed: arm at least one input, or record the local mix".to_owned()
        } else {
            format!(
                "nothing is armed on this device: {} are past its {inputs} inputs; arm one of \
                 its inputs, or record the local mix",
                channel_list(&skipped)
            )
        });
    }
    let folder = folder_of(&kept, places.default);
    fs::create_dir_all(&folder).map_err(|error| {
        format!(
            "the recordings folder {} could not be created: {error}",
            folder.display()
        )
    })?;
    let name = match take_name.trim() {
        "" => DEFAULT_TAKE_NAME,
        name => name,
    };
    let request = RecordRequest {
        take_name: name.to_owned(),
        utc_offset_minutes,
        ..request_of(&kept, folder.clone(), &armed)
    };
    let status = engine.start_recording(request)?;
    Ok(RecordingStartedDto {
        recording: (&status).into(),
        folder: folder.display().to_string(),
        armed,
        skipped,
        include_local: kept.include_local,
        format: kept.format.name(),
    })
}

/// [stop_recording] over any engine slot.
fn stop_with<E: RecordEngine>(engine: &Mutex<Option<E>>) -> Result<RecordingStoppedDto, String> {
    let mut guard = engine_lock(engine)?;
    let engine = guard
        .as_mut()
        .ok_or_else(|| "the engine is not running, so nothing is recording".to_owned())?;
    match engine.stop_recording() {
        Ok(take) => Ok(RecordingStoppedDto {
            finishing: false,
            take: Some((&take).into()),
            message: None,
        }),
        // The take is stopping and its files are still being finished: not a failure, the window
        // asks again.
        Err(error) if engine.recording().state == RecordingState::Stopping => {
            Ok(RecordingStoppedDto {
                finishing: true,
                take: None,
                message: Some(error),
            })
        }
        Err(error) => Err(error),
    }
}

/// [add_marker] over any engine slot.
fn marker_with<E: RecordEngine>(
    engine: &Mutex<Option<E>>,
    name: &str,
) -> Result<TakeMarkerDto, String> {
    let guard = engine_lock(engine)?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running, so no take is recording".to_owned())?;
    let marker = engine.add_marker(name)?;
    Ok(TakeMarkerDto::of(&marker, TAKE_SAMPLE_RATE))
}

/// [record_estimate] over any engine slot, session and places.
fn estimate_with<E: RecordEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    places: Places<'_>,
) -> Result<DiskEstimateDto, String> {
    let guard = engine_lock(engine)?;
    let kept = session.recording().unwrap_or_default();
    let folder = folder_of(&kept, places.default);
    let unknown = |problem: String, free_bytes: Option<u64>| DiskEstimateDto {
        folder: folder.display().to_string(),
        bytes_per_second: None,
        free_bytes,
        minutes_left: None,
        problem: Some(problem),
    };
    let Some(engine) = guard.as_ref() else {
        return Ok(unknown(
            "the engine is not running: the tracks depend on the input device".to_owned(),
            free_space(&folder),
        ));
    };
    let (armed, _) = split_armed(&kept.armed, engine.inputs());
    match engine.record_estimate(&request_of(&kept, folder.clone(), &armed)) {
        Ok(estimate) => Ok(DiskEstimateDto {
            folder: folder.display().to_string(),
            bytes_per_second: Some(estimate.bytes_per_second),
            free_bytes: estimate.free_bytes,
            minutes_left: estimate.minutes_left,
            problem: None,
        }),
        Err(problem) => Ok(unknown(problem, free_space(&folder))),
    }
}

/// [list_takes] over any engine slot, session and places.
fn list_with<E: RecordEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    places: Places<'_>,
) -> Result<TakesDto, String> {
    // Only the take being recorded is read of the engine; the folder is read with the lock free.
    let recording = engine_lock(engine)?
        .as_ref()
        .map(RecordEngine::recording)
        .filter(|status| status.state != RecordingState::Idle)
        .and_then(|status| status.take_folder);
    let kept = session.recording().unwrap_or_default();
    read_takes(&folder_of(&kept, places.default), recording.as_deref())
}

/// The takes in `folder`, the newest first; `recording` is the folder of the take being recorded.
fn read_takes(folder: &Path, recording: Option<&Path>) -> Result<TakesDto, String> {
    let mut answer = TakesDto {
        folder: folder.display().to_string(),
        takes: Vec::new(),
        unreadable: 0,
        missing: false,
    };
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            answer.missing = true;
            return Ok(answer);
        }
        Err(error) => {
            return Err(format!(
                "the recordings folder {} could not be read: {error}",
                folder.display()
            ))
        }
    };
    for entry in entries {
        let Ok(entry) = entry else {
            answer.unreadable += 1;
            continue;
        };
        let take = entry.path();
        if !take.is_dir() {
            continue;
        }
        let bytes = match fs::read(take.join(TAKE_FILE)) {
            Ok(bytes) => bytes,
            // A folder without one is not a take.
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(_) => {
                answer.unreadable += 1;
                continue;
            }
        };
        let now = recording.is_some_and(|recording| same_path(recording, &take));
        match read_take(&take, &bytes, now) {
            Some(item) => answer.takes.push(item),
            None => answer.unreadable += 1,
        }
    }
    answer.takes.sort_by(|one, other| {
        other
            .started
            .cmp(&one.started)
            .then_with(|| other.folder.cmp(&one.folder))
    });
    Ok(answer)
}

/// `take.json`, version 1, as the engine writes it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeFile {
    version: u64,
    name: String,
    started: String,
    sample_rate: u32,
    format: String,
    #[serde(default)]
    device: String,
    #[serde(default)]
    tracks: Vec<TrackFile>,
    #[serde(default)]
    frames: u64,
    #[serde(default)]
    markers: Vec<TakeMarkerFile>,
    #[serde(default)]
    dropped_frames: u64,
    /// `null` while the take records, and in a take that never ended.
    #[serde(default)]
    ended: Option<TakeEndFile>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrackFile {
    file: String,
    kind: String,
    #[serde(default)]
    channels: Vec<u16>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    instrument: Option<String>,
    #[serde(default)]
    frames: u64,
}

#[derive(Deserialize)]
struct TakeMarkerFile {
    frame: u64,
    name: String,
}

#[derive(Deserialize)]
#[serde(tag = "how", rename_all = "camelCase")]
enum TakeEndFile {
    Stopped,
    EngineStopped,
    DiskError { reason: String },
}

/// The take in folder `take` whose `take.json` holds `bytes`; `None` when it cannot be read (or a
/// newer MixLink wrote it). `recording` says it is the take being recorded now. A take that never
/// ended is interrupted, and its frames are what its files hold.
fn read_take(take: &Path, bytes: &[u8], recording: bool) -> Option<TakeListItemDto> {
    let file: TakeFile = serde_json::from_slice(bytes).ok()?;
    if file.version != 1 {
        return None;
    }
    let (ended, reason) = match file.ended {
        Some(TakeEndFile::Stopped) => ("stopped", None),
        Some(TakeEndFile::EngineStopped) => ("engineStopped", None),
        Some(TakeEndFile::DiskError { reason }) => ("diskError", Some(reason)),
        None if recording => ("recording", None),
        None => ("interrupted", None),
    };
    let mut tracks: Vec<TakeTrackDto> = file
        .tracks
        .into_iter()
        .map(|track| TakeTrackDto {
            file: track.file,
            kind: track.kind,
            channels: track.channels,
            name: track.name,
            instrument: track.instrument,
            frames: track.frames,
        })
        .collect();
    let mut frames = file.frames;
    if ended == "interrupted" {
        // Its take.json was written at the start: the files hold what their last refresh did.
        for track in &mut tracks {
            track.frames = file_name_in(take, &track.file)
                .and_then(|path| WavReader::open(path).ok())
                .map_or(0, |reader| reader.frames());
        }
        frames = tracks.iter().map(|track| track.frames).min().unwrap_or(0);
    }
    let sample_rate = file.sample_rate;
    let seconds = |frames: u64| {
        if sample_rate == 0 {
            0.0
        } else {
            frames as f64 / f64::from(sample_rate)
        }
    };
    Some(TakeListItemDto {
        folder: take.display().to_string(),
        name: file.name,
        started: file.started,
        sample_rate,
        format: file.format,
        device: file.device,
        tracks,
        frames,
        seconds: seconds(frames),
        markers: file
            .markers
            .into_iter()
            .map(|marker| TakeMarkerDto {
                frame: marker.frame,
                seconds: seconds(marker.frame),
                name: marker.name,
            })
            .collect(),
        dropped_frames: file.dropped_frames,
        ended: ended.to_owned(),
        reason,
    })
}

/// The file `name` inside `take`, when `name` is one plain file name (never a path that leaves the
/// take folder).
fn file_name_in(take: &Path, name: &str) -> Option<PathBuf> {
    let mut parts = Path::new(name).components();
    match (parts.next(), parts.next()) {
        (Some(Component::Normal(file)), None) => Some(take.join(file)),
        _ => None,
    }
}

/// [open_take_folder] over any session, places and file manager.
fn open_with(
    session: &SessionStore,
    places: Places<'_>,
    opener: &dyn FolderOpener,
    folder: &str,
) -> Result<(), String> {
    let take = take_in_recordings(session, places, folder)?;
    println!("[mixlink-desktop] open_take_folder: \"{}\"", take.display());
    opener.open(&take)
}

/// The take folder `folder` names in the kept (or default) recordings folder, checked on the disk
/// as [take_folder_in] does: what `open_take_folder` opens and a soundcheck plays.
pub(super) fn take_in_recordings(
    session: &SessionStore,
    places: Places<'_>,
    folder: &str,
) -> Result<PathBuf, String> {
    let recordings = folder_of(&session.recording().unwrap_or_default(), places.default);
    take_folder_in(&recordings, Path::new(folder.trim()))
}

/// The take folder `folder` names, as `recordings` joined with its name: only a folder holding a
/// `take.json` directly inside `recordings`, both checked on the disk (links and `..` resolved).
fn take_folder_in(recordings: &Path, folder: &Path) -> Result<PathBuf, String> {
    let refused = || {
        format!(
            "{} is not a take folder in the recordings folder {}",
            folder.display(),
            recordings.display()
        )
    };
    if !folder.is_absolute() {
        return Err(refused());
    }
    let root = fs::canonicalize(recordings).map_err(|_| refused())?;
    let found = fs::canonicalize(folder).map_err(|_| refused())?;
    if found.parent() != Some(root.as_path()) || !found.is_dir() || !found.join(TAKE_FILE).is_file()
    {
        return Err(refused());
    }
    let name = found.file_name().ok_or_else(refused)?;
    Ok(recordings.join(name))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use personal_monitoring::{ChannelKind, TakeEnd, TakeTrack, TrackKind, WavSpec, WavWriter};

    use super::*;

    /// An empty scratch directory for one test.
    fn scratch(test: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-recording-tests")
            .join(test);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("scratch directory");
        directory
    }

    /// A desk for one test: its config folder (with the session in it) and its default
    /// recordings folder, apart from each other.
    struct Desk {
        root: PathBuf,
        config: PathBuf,
        session: SessionStore,
        default: RecordingsFolder,
    }

    impl Desk {
        fn new(test: &str) -> Self {
            let root = scratch(test);
            let config = root.join("config");
            let session = SessionStore::open(&config);
            let default = RecordingsFolder {
                folder: root.join("Music").join("MixLink").join("Recordings"),
                notice: None,
            };
            Self {
                root,
                config,
                session,
                default,
            }
        }

        fn places(&self) -> Places<'_> {
            Places {
                default: &self.default,
                config: &self.config,
            }
        }

        fn keep(&self, recording: SessionRecording) {
            self.session.set_recording(recording).expect("kept");
        }
    }

    fn summary(folder: &Path) -> TakeSummary {
        TakeSummary {
            folder: folder.to_path_buf(),
            name: "Show".to_owned(),
            started: "2026-10-16T19:04:05Z".to_owned(),
            sample_rate: 48_000,
            format: SampleFormat::Int24,
            device: "Volt 4".to_owned(),
            tracks: vec![TakeTrack {
                file: "01 Kick.wav".to_owned(),
                kind: TrackKind::Input,
                channels: vec![1],
                name: "Kick".to_owned(),
                instrument: Some(ChannelKind::Drums),
                frames: 48_000,
            }],
            frames: 48_000,
            markers: Vec::new(),
            dropped_frames: 0,
            ended: TakeEnd::Stopped,
        }
    }

    /// Stands in for a running engine's recorder: it takes what it is asked, as the engine does,
    /// and answers what the test set.
    #[derive(Default)]
    struct RecordStandIn {
        inputs: usize,
        status: RecordingStatus,
        /// Every start it took, in order.
        requests: Vec<RecordRequest>,
        /// Why it refuses a start, when it does.
        refuse_start: Option<String>,
        /// What each stop answers, in order; a stop with none left says nothing records.
        stops: VecDeque<Result<TakeSummary, String>>,
        /// Every marker name it was given.
        marked: Mutex<Vec<String>>,
        estimate: Option<Result<DiskEstimate, String>>,
    }

    impl RecordEngine for RecordStandIn {
        fn inputs(&self) -> usize {
            self.inputs
        }

        fn start_recording(&mut self, request: RecordRequest) -> Result<RecordingStatus, String> {
            if let Some(why) = &self.refuse_start {
                return Err(why.clone());
            }
            self.status = RecordingStatus {
                state: RecordingState::Recording,
                take_folder: Some(request.folder.join("2026-10-16_210405_Take")),
                tracks: vec!["Kick".to_owned()],
                ..RecordingStatus::default()
            };
            self.requests.push(request);
            Ok(self.status.clone())
        }

        fn stop_recording(&mut self) -> Result<TakeSummary, String> {
            let answer = self
                .stops
                .pop_front()
                .unwrap_or_else(|| Err("nothing is recording".to_owned()));
            self.status.state = match &answer {
                Ok(_) => RecordingState::Idle,
                Err(error) if error.contains("still being finished") => RecordingState::Stopping,
                Err(_) => self.status.state.clone(),
            };
            answer
        }

        fn add_marker(&self, name: &str) -> Result<TakeMarker, String> {
            if self.status.state != RecordingState::Recording {
                return Err("no take is recording".to_owned());
            }
            self.marked.lock().expect("marks").push(name.to_owned());
            Ok(TakeMarker {
                frame: 96_000,
                name: if name.trim().is_empty() {
                    "Marker 1".to_owned()
                } else {
                    name.trim().to_owned()
                },
            })
        }

        fn recording(&self) -> RecordingStatus {
            self.status.clone()
        }

        fn record_estimate(&self, _request: &RecordRequest) -> Result<DiskEstimate, String> {
            self.estimate
                .clone()
                .unwrap_or_else(|| Err("no estimate".to_owned()))
        }
    }

    fn running(engine: RecordStandIn) -> Mutex<Option<RecordStandIn>> {
        Mutex::new(Some(engine))
    }

    fn idle() -> Mutex<Option<RecordStandIn>> {
        Mutex::new(None)
    }

    fn six_inputs() -> RecordStandIn {
        RecordStandIn {
            inputs: 6,
            ..RecordStandIn::default()
        }
    }

    fn requests_of(engine: &Mutex<Option<RecordStandIn>>) -> Vec<RecordRequest> {
        engine
            .lock()
            .expect("engine")
            .as_ref()
            .map(|engine| engine.requests.clone())
            .unwrap_or_default()
    }

    fn settings(
        folder: Option<&str>,
        format: Option<&str>,
        armed: &[u32],
    ) -> RecordingSettingsRequest {
        RecordingSettingsRequest {
            folder: folder.map(str::to_owned),
            format: format.map(str::to_owned),
            include_local: false,
            armed: armed.to_vec(),
        }
    }

    #[test]
    fn the_default_folder_is_in_the_music_folder_or_says_where_it_fell_back_to() {
        let music = PathBuf::from("C:\\Users\\Ana\\Music");
        let data = PathBuf::from("C:\\Users\\Ana\\AppData\\Local\\MixLink");
        let config = PathBuf::from("C:\\Users\\Ana\\AppData\\Roaming\\MixLink");

        assert_eq!(
            default_recordings_folder(Some(music.clone()), Some(data.clone()), &config),
            RecordingsFolder {
                folder: music.join("MixLink").join("Recordings"),
                notice: None,
            }
        );
        let fallen = default_recordings_folder(None, Some(data.clone()), &config);
        assert_eq!(fallen.folder, data.join("Recordings"));
        let notice = fallen.notice.expect("said");
        assert!(notice.contains("Music folder"), "{notice}");
        assert!(
            notice.contains(&data.join("Recordings").display().to_string()),
            "{notice}"
        );
        let nowhere = default_recordings_folder(None, None, &config);
        assert!(nowhere
            .folder
            .ends_with(Path::new("MixLink").join("Recordings")));
        assert!(nowhere.folder.is_absolute());
        assert!(nowhere.notice.is_some());
    }

    /// The rule a folder the engineer types is held to holds for the default too: it is never in
    /// the app's own config folder. The app's data folder is its config folder on macOS, and a
    /// desk with no default named falls back to its session's folder, so each place inside the
    /// config folder gives way to the next, and the notice says where takes go.
    #[test]
    fn the_default_folder_is_never_inside_the_config_folder() {
        let config = PathBuf::from("C:\\Users\\Ana\\Library\\Application Support\\MixLink");
        let temporary = std::env::temp_dir().join("MixLink").join("Recordings");

        let same = default_recordings_folder(None, Some(config.clone()), &config);
        assert_eq!(
            same.folder, temporary,
            "the data folder is the config folder"
        );
        let notice = same.notice.expect("said");
        assert!(notice.contains("settings folder"), "{notice}");
        assert!(
            notice.contains(&temporary.display().to_string()),
            "{notice}"
        );

        let data = PathBuf::from("C:\\Users\\Ana\\AppData\\Local\\MixLink");
        let music_inside =
            default_recordings_folder(Some(config.join("Music")), Some(data.clone()), &config);
        assert_eq!(music_inside.folder, data.join("Recordings"));
        assert!(music_inside.notice.is_some());

        // Even the temporary folder can be inside it: then a folder beside it.
        let everywhere = std::env::temp_dir();
        let beside = default_recordings_folder(None, Some(everywhere.clone()), &everywhere);
        assert!(
            !inside(&beside.folder, &everywhere),
            "{}",
            beside.folder.display()
        );
        assert!(beside.folder.is_absolute());
        assert!(beside
            .notice
            .expect("said")
            .contains(&beside.folder.display().to_string()));
    }

    #[test]
    fn a_desk_with_no_default_named_records_outside_its_config_folder() {
        let config = scratch("state-default").join("config");
        let state = DesktopState::new(SessionStore::open(&config));

        let answer = recording_settings(&state).expect("answered");

        let default = PathBuf::from(&answer.default_folder);
        assert!(!inside(&default, &config), "{}", default.display());
        assert!(answer.notice.is_some(), "it says where takes go");
    }

    /// Typed out, a default that is inside the config folder is refused like any folder there,
    /// rather than slipping through as the default.
    #[test]
    fn the_config_folder_is_refused_before_the_default_is_matched() {
        let desk = Desk::new("settings-default-inside");
        let default = RecordingsFolder {
            folder: desk.config.join("Recordings"),
            notice: None,
        };
        let places = Places {
            default: &default,
            config: &desk.config,
        };

        let error = keep_settings(
            &desk.session,
            places,
            settings(default.folder.to_str(), None, &[1]),
        )
        .expect_err("refused");

        assert!(error.contains("settings folder"), "{error}");
        assert_eq!(desk.session.recording(), None, "nothing changed");
    }

    #[test]
    fn with_nothing_kept_the_settings_are_the_defaults() {
        let mut desk = Desk::new("settings-defaults");
        desk.default.notice = Some("Your Music folder could not be found.".to_owned());

        let answer = settings_of(&desk.session, desk.places());

        assert_eq!(
            answer,
            RecordingSettingsDto {
                folder: desk.default.folder.display().to_string(),
                default_folder: desk.default.folder.display().to_string(),
                folder_is_default: true,
                format: "int24",
                include_local: false,
                armed: Vec::new(),
                notice: Some("Your Music folder could not be found.".to_owned()),
            }
        );
    }

    #[test]
    fn settings_are_kept_in_order_once_each_and_answered_whole() {
        let desk = Desk::new("settings-kept");
        let takes = desk.root.join("Shows");
        let request = RecordingSettingsRequest {
            folder: Some(format!("  {}  ", takes.display())),
            format: Some("float32".to_owned()),
            include_local: true,
            armed: vec![5, 1, 2, 5],
        };

        let answer = keep_settings(&desk.session, desk.places(), request).expect("kept");

        assert_eq!(answer.folder, takes.display().to_string());
        assert!(!answer.folder_is_default);
        assert_eq!(answer.format, "float32");
        assert!(answer.include_local);
        assert_eq!(answer.armed, vec![1, 2, 5]);
        assert_eq!(
            desk.session.recording(),
            Some(SessionRecording {
                folder: Some(takes.display().to_string()),
                format: RecordFormat::Float32,
                include_local: true,
                armed: vec![1, 2, 5],
            })
        );
        assert_eq!(settings_of(&desk.session, desk.places()), answer);
    }

    #[test]
    fn a_blank_folder_or_the_default_typed_out_is_the_default_folder() {
        let desk = Desk::new("settings-default-folder");
        let typed = desk.default.folder.display().to_string();

        for folder in [None, Some("   "), Some(typed.as_str())] {
            let answer = keep_settings(&desk.session, desk.places(), settings(folder, None, &[1]))
                .expect("kept");
            assert!(answer.folder_is_default, "{folder:?}");
            assert_eq!(answer.folder, typed);
            assert_eq!(answer.format, "int24", "the default format");
            assert_eq!(desk.session.recording().and_then(|kept| kept.folder), None);
        }
    }

    #[test]
    fn a_folder_that_is_not_whole_goes_up_or_is_inside_the_config_folder_is_refused() {
        let desk = Desk::new("settings-refused");
        desk.keep(SessionRecording {
            armed: vec![2],
            ..SessionRecording::default()
        });
        let inside = desk.config.join("Takes");
        let up = desk.root.join("Shows").join("..").join("config");

        for (folder, word) in [
            ("Recordings", "whole path"),
            (".\\Takes", "whole path"),
            (up.to_str().expect("text"), ".."),
            (inside.to_str().expect("text"), "settings folder"),
            (desk.config.to_str().expect("text"), "settings folder"),
        ] {
            let error = keep_settings(
                &desk.session,
                desk.places(),
                settings(Some(folder), None, &[1]),
            )
            .expect_err("refused");
            assert!(error.contains(word), "{folder}: {error}");
        }
        assert_eq!(
            desk.session.recording(),
            Some(SessionRecording {
                armed: vec![2],
                ..SessionRecording::default()
            }),
            "nothing changed"
        );
    }

    #[test]
    fn an_unknown_format_or_an_input_out_of_range_is_refused() {
        let desk = Desk::new("settings-format");

        let error = keep_settings(
            &desk.session,
            desk.places(),
            settings(None, Some("mp3"), &[1]),
        )
        .expect_err("refused");
        assert!(error.contains("mp3") && error.contains("int24"), "{error}");
        for armed in [0, 256] {
            let error = keep_settings(&desk.session, desk.places(), settings(None, None, &[armed]))
                .expect_err("refused");
            assert!(error.contains(&format!("CH {armed}")), "{error}");
        }
        assert_eq!(desk.session.recording(), None);
    }

    #[test]
    fn a_start_records_the_armed_inputs_of_the_device_into_the_kept_folder() {
        let desk = Desk::new("start");
        let folder = desk.root.join("Shows").join("Sunday");
        desk.keep(SessionRecording {
            folder: Some(folder.display().to_string()),
            format: RecordFormat::Float32,
            include_local: true,
            armed: vec![1, 2, 5, 9],
        });
        let engine = running(six_inputs());

        let started = start_with(
            &engine,
            &desk.session,
            desk.places(),
            "  Soundcheck ",
            Some(120),
        )
        .expect("started");

        assert!(folder.is_dir(), "the folder is created when missing");
        assert_eq!(
            requests_of(&engine),
            vec![RecordRequest {
                folder: folder.clone(),
                take_name: "Soundcheck".to_owned(),
                format: SampleFormat::Float32,
                armed: vec![0, 1, 4],
                include_local: true,
                utc_offset_minutes: 120,
            }],
            "0-based inputs the device has, the kept format and the local mix"
        );
        assert_eq!(started.armed, vec![1, 2, 5]);
        assert_eq!(
            started.skipped,
            vec![9],
            "an input past the device is named"
        );
        assert_eq!(started.folder, folder.display().to_string());
        assert_eq!(started.format, "float32");
        assert!(started.include_local);
        assert_eq!(started.recording.state, "recording");
    }

    #[test]
    fn a_take_with_no_name_is_called_take_and_the_default_folder_is_used() {
        let desk = Desk::new("start-defaults");
        desk.keep(SessionRecording {
            armed: vec![3],
            ..SessionRecording::default()
        });
        let engine = running(six_inputs());

        start_with(&engine, &desk.session, desk.places(), "   ", None).expect("started");

        let request = requests_of(&engine).pop().expect("taken");
        assert_eq!(request.take_name, DEFAULT_TAKE_NAME);
        assert_eq!(request.folder, desk.default.folder);
        assert_eq!(request.format, SampleFormat::Int24);
        assert_eq!(request.utc_offset_minutes, 0, "none is UTC");
        assert!(desk.default.folder.is_dir());
    }

    #[test]
    fn a_start_with_nothing_armed_no_engine_or_a_take_recording_is_refused() {
        let desk = Desk::new("start-refused");

        let error =
            start_with(&idle(), &desk.session, desk.places(), "Take", None).expect_err("no engine");
        assert!(error.contains("not running"), "{error}");

        let engine = running(six_inputs());
        let error = start_with(&engine, &desk.session, desk.places(), "Take", None)
            .expect_err("nothing armed");
        assert!(error.contains("nothing is armed"), "{error}");

        desk.keep(SessionRecording {
            armed: vec![7, 8],
            ..SessionRecording::default()
        });
        let error = start_with(&engine, &desk.session, desk.places(), "Take", None)
            .expect_err("nothing armed on this device");
        assert!(
            error.contains("CH 7, CH 8") && error.contains("6 inputs"),
            "{error}"
        );
        assert!(
            requests_of(&engine).is_empty(),
            "the engine was asked nothing"
        );
        assert!(!desk.default.folder.exists(), "no folder made");

        desk.keep(SessionRecording {
            armed: vec![1],
            ..SessionRecording::default()
        });
        let recording = running(RecordStandIn {
            status: RecordingStatus {
                state: RecordingState::Recording,
                ..RecordingStatus::default()
            },
            ..six_inputs()
        });
        let error = start_with(&recording, &desk.session, desk.places(), "Take", None)
            .expect_err("a take records");
        assert!(error.contains("already recording"), "{error}");

        let error = start_with(&engine, &desk.session, desk.places(), "Take", Some(24 * 60))
            .expect_err("no such time zone");
        assert!(error.contains("offset"), "{error}");
    }

    #[test]
    fn the_local_mix_alone_can_be_recorded() {
        let desk = Desk::new("start-local-only");
        desk.keep(SessionRecording {
            include_local: true,
            ..SessionRecording::default()
        });
        let engine = running(six_inputs());

        let started =
            start_with(&engine, &desk.session, desk.places(), "Mix", None).expect("started");

        assert!(started.armed.is_empty());
        assert_eq!(requests_of(&engine)[0].armed, Vec::<usize>::new());
        assert!(requests_of(&engine)[0].include_local);
    }

    #[test]
    fn a_folder_that_cannot_be_created_or_an_engine_refusal_is_said_and_nothing_records() {
        let desk = Desk::new("start-folder");
        let blocked = desk.root.join("blocked");
        fs::write(&blocked, b"a file where the folder would be").expect("blocker");
        desk.keep(SessionRecording {
            folder: Some(blocked.join("Takes").display().to_string()),
            armed: vec![1],
            ..SessionRecording::default()
        });
        let engine = running(six_inputs());

        let error =
            start_with(&engine, &desk.session, desk.places(), "Take", None).expect_err("no folder");
        assert!(error.contains("could not be created"), "{error}");
        assert!(
            error.contains(&blocked.join("Takes").display().to_string()),
            "{error}"
        );
        assert!(requests_of(&engine).is_empty());

        desk.keep(SessionRecording {
            armed: vec![1],
            ..SessionRecording::default()
        });
        let refusing = running(RecordStandIn {
            refuse_start: Some("the engine is stopping".to_owned()),
            ..six_inputs()
        });
        let error =
            start_with(&refusing, &desk.session, desk.places(), "Take", None).expect_err("refused");
        assert!(error.contains("stopping"), "{error}");
    }

    #[test]
    fn a_stop_answers_the_finished_take_or_says_its_files_are_still_being_finished() {
        let take = summary(Path::new("D:\\Takes\\2026-10-16_210405_Show"));
        let engine = running(RecordStandIn {
            status: RecordingStatus {
                state: RecordingState::Recording,
                ..RecordingStatus::default()
            },
            stops: VecDeque::from([
                Err("the take's files are still being finished; try again".to_owned()),
                Ok(take.clone()),
            ]),
            ..six_inputs()
        });

        let first = stop_with(&engine).expect("an answer, not an error");
        assert!(first.finishing);
        assert_eq!(first.take, None);
        assert!(first
            .message
            .expect("said")
            .contains("still being finished"));

        let second = stop_with(&engine).expect("stopped");
        assert!(!second.finishing);
        assert_eq!(second.take.expect("the take").name, "Show");

        let error = stop_with(&engine).expect_err("nothing records");
        assert!(error.contains("nothing is recording"), "{error}");
        let error = stop_with(&idle()).expect_err("no engine");
        assert!(error.contains("not running"), "{error}");
    }

    #[test]
    fn a_marker_goes_to_the_take_being_recorded() {
        let engine = running(RecordStandIn {
            status: RecordingStatus {
                state: RecordingState::Recording,
                ..RecordingStatus::default()
            },
            ..six_inputs()
        });

        let marker = marker_with(&engine, "Chorus").expect("marked");

        assert_eq!(
            marker,
            TakeMarkerDto {
                frame: 96_000,
                seconds: 2.0,
                name: "Chorus".to_owned(),
            }
        );
        let error = marker_with(&idle(), "Chorus").expect_err("no engine");
        assert!(error.contains("not running"), "{error}");
        let error = marker_with(&running(six_inputs()), "Chorus").expect_err("no take");
        assert!(error.contains("no take"), "{error}");
    }

    #[test]
    fn the_estimate_says_why_it_has_no_rate_and_gives_the_engines_otherwise() {
        let desk = Desk::new("estimate");
        desk.keep(SessionRecording {
            armed: vec![1, 2],
            ..SessionRecording::default()
        });

        let idle_estimate = estimate_with(&idle(), &desk.session, desk.places()).expect("answered");
        assert_eq!(
            idle_estimate.folder,
            desk.default.folder.display().to_string()
        );
        assert_eq!(idle_estimate.bytes_per_second, None);
        assert!(idle_estimate.problem.expect("said").contains("not running"));

        let engine = running(RecordStandIn {
            estimate: Some(Ok(DiskEstimate {
                bytes_per_second: 288_000,
                free_bytes: Some(1_728_000_000),
                minutes_left: Some(100),
            })),
            ..six_inputs()
        });
        let estimate = estimate_with(&engine, &desk.session, desk.places()).expect("answered");
        assert_eq!(estimate.bytes_per_second, Some(288_000));
        assert_eq!(estimate.free_bytes, Some(1_728_000_000));
        assert_eq!(estimate.minutes_left, Some(100));
        assert_eq!(estimate.problem, None);

        let refusing = running(RecordStandIn {
            estimate: Some(Err(
                "arm at least one input, or record the local mix".to_owned()
            )),
            ..six_inputs()
        });
        let refused = estimate_with(&refusing, &desk.session, desk.places()).expect("answered");
        assert_eq!(refused.bytes_per_second, None);
        assert!(refused
            .problem
            .expect("said")
            .contains("arm at least one input"));
    }

    /// Writes `take.json` into a new take folder `name` in `folder`.
    fn take_folder(folder: &Path, name: &str, json: &str) -> PathBuf {
        let take = folder.join(name);
        fs::create_dir_all(&take).expect("take folder");
        fs::write(take.join(TAKE_FILE), json).expect("take.json");
        take
    }

    fn take_json(name: &str, started: &str, ended: &str) -> String {
        format!(
            r#"{{
  "version": 1,
  "name": "{name}",
  "started": "{started}",
  "sampleRate": 48000,
  "format": "int24",
  "device": "Volt 4",
  "tracks": [
    {{ "file": "01 Kick.wav", "kind": "input", "channels": [1], "name": "Kick", "instrument": "drums", "frames": 96000 }},
    {{ "file": "03-04 Keys.wav", "kind": "pair", "channels": [3, 4], "name": "Keys", "instrument": "keys", "frames": 96000 }}
  ],
  "frames": 96000,
  "markers": [{{ "frame": 48000, "name": "Chorus" }}],
  "droppedFrames": 0,
  "ended": {ended}
}}
"#
        )
    }

    #[test]
    fn takes_are_read_from_the_folder_newest_first_skipping_what_cannot_be_read() {
        let desk = Desk::new("list");
        let folder = desk.default.folder.clone();
        fs::create_dir_all(&folder).expect("folder");
        take_folder(
            &folder,
            "2026-10-15_200000_Rehearsal",
            &take_json(
                "Rehearsal",
                "2026-10-15T18:00:00Z",
                r#"{ "how": "stopped" }"#,
            ),
        );
        take_folder(
            &folder,
            "2026-10-16_190000_Show",
            &take_json(
                "Show",
                "2026-10-16T17:00:00Z",
                r#"{ "how": "diskError", "reason": "the disk is full" }"#,
            ),
        );
        // A take that never ended: its files hold what their last refresh did.
        let crashed = take_folder(
            &folder,
            "2026-10-16_100000_Crash",
            &take_json("Crash", "2026-10-16T08:00:00Z", "null").replace("96000", "0"),
        );
        let spec = |channels| WavSpec {
            channels,
            sample_rate: 48_000,
            format: SampleFormat::Int24,
        };
        let mut kick = WavWriter::create(crashed.join("01 Kick.wav"), spec(1)).expect("kick");
        kick.write_frames(&vec![0.25; 4_800]).expect("written");
        drop(kick);
        let mut keys = WavWriter::create(crashed.join("03-04 Keys.wav"), spec(2)).expect("keys");
        keys.write_frames(&vec![0.5; 2 * 2_400]).expect("written");
        drop(keys);
        take_folder(&folder, "2026-10-14_damaged", "{ not json");
        take_folder(
            &folder,
            "2026-10-13_newer",
            r#"{ "version": 2, "name": "Future" }"#,
        );
        fs::create_dir_all(folder.join("Not a take")).expect("a folder without take.json");
        fs::write(folder.join("notes.txt"), b"a file").expect("a file");
        let engine = running(six_inputs());

        let listed = list_with(&engine, &desk.session, desk.places()).expect("listed");

        assert_eq!(listed.folder, folder.display().to_string());
        assert!(!listed.missing);
        assert_eq!(listed.unreadable, 2, "the damaged and the newer take.json");
        let names: Vec<&str> = listed.takes.iter().map(|take| take.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["Show", "Crash", "Rehearsal"],
            "the newest first"
        );
        let show = &listed.takes[0];
        assert_eq!(show.ended, "diskError");
        assert_eq!(show.reason.as_deref(), Some("the disk is full"));
        assert_eq!(show.seconds, 2.0);
        assert_eq!(show.markers[0].seconds, 1.0);
        assert_eq!(
            show.tracks[1],
            TakeTrackDto {
                file: "03-04 Keys.wav".to_owned(),
                kind: "pair".to_owned(),
                channels: vec![3, 4],
                name: "Keys".to_owned(),
                instrument: Some("keys".to_owned()),
                frames: 96_000,
            }
        );
        assert_eq!(
            show.folder,
            folder.join("2026-10-16_190000_Show").display().to_string()
        );
        let crash = &listed.takes[1];
        assert_eq!(crash.ended, "interrupted");
        assert_eq!(
            crash
                .tracks
                .iter()
                .map(|track| track.frames)
                .collect::<Vec<_>>(),
            vec![4_800, 2_400],
            "the frames each file holds"
        );
        assert_eq!(crash.frames, 2_400, "the frames every file holds");
        assert_eq!(listed.takes[2].ended, "stopped");
    }

    #[test]
    fn the_take_being_recorded_is_listed_as_recording() {
        let desk = Desk::new("list-recording");
        let folder = desk.default.folder.clone();
        let now = take_folder(
            &folder,
            "2026-10-16_210000_Live",
            &take_json("Live", "2026-10-16T19:00:00Z", "null"),
        );
        let engine = running(RecordStandIn {
            status: RecordingStatus {
                state: RecordingState::Recording,
                take_folder: Some(now),
                ..RecordingStatus::default()
            },
            ..six_inputs()
        });

        let listed = list_with(&engine, &desk.session, desk.places()).expect("listed");

        assert_eq!(listed.takes[0].ended, "recording");
    }

    #[test]
    fn a_folder_with_no_takes_yet_is_an_empty_list() {
        let desk = Desk::new("list-missing");

        let listed = list_with(&idle(), &desk.session, desk.places()).expect("listed");

        assert!(listed.missing);
        assert!(listed.takes.is_empty());
        assert_eq!(listed.unreadable, 0);
    }

    /// A file manager that only notes what it was asked to open.
    #[derive(Default)]
    struct NotedOpener {
        opened: Mutex<Vec<PathBuf>>,
    }

    impl FolderOpener for NotedOpener {
        fn open(&self, folder: &Path) -> Result<(), String> {
            self.opened
                .lock()
                .expect("noted")
                .push(folder.to_path_buf());
            Ok(())
        }
    }

    #[test]
    fn only_a_take_folder_inside_the_recordings_folder_is_opened() {
        let desk = Desk::new("open");
        let folder = desk.default.folder.clone();
        let take = take_folder(
            &folder,
            "2026-10-16_190000_Show, take 2",
            &take_json("Show", "2026-10-16T17:00:00Z", r#"{ "how": "stopped" }"#),
        );
        let elsewhere = take_folder(
            &desk.root,
            "2026-10-16_190000_Elsewhere",
            &take_json(
                "Elsewhere",
                "2026-10-16T17:00:00Z",
                r#"{ "how": "stopped" }"#,
            ),
        );
        let plain = folder.join("Not a take");
        fs::create_dir_all(&plain).expect("plain folder");
        let opener = NotedOpener::default();

        open_with(
            &desk.session,
            desk.places(),
            &opener,
            &take.display().to_string(),
        )
        .expect("opened");

        assert_eq!(
            *opener.opened.lock().expect("noted"),
            vec![folder.join("2026-10-16_190000_Show, take 2")]
        );
        let up = take.join("..").display().to_string();
        let sneaky = take
            .join("..")
            .join("..")
            .join("2026-10-16_190000_Elsewhere");
        for refused in [
            elsewhere.display().to_string(),
            folder.display().to_string(),
            plain.display().to_string(),
            up,
            sneaky.display().to_string(),
            "2026-10-16_190000_Show, take 2".to_owned(),
            folder.join("missing").display().to_string(),
            "explorer.exe & calc".to_owned(),
        ] {
            let error =
                open_with(&desk.session, desk.places(), &opener, &refused).expect_err("refused");
            assert!(error.contains("not a take folder"), "{refused}: {error}");
        }
        assert_eq!(
            opener.opened.lock().expect("noted").len(),
            1,
            "nothing else was opened"
        );
    }
}
