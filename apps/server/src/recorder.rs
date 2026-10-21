//! Multitrack recording: the armed inputs, each to its own WAV file, while the show runs.
//!
//! ```text
//!  engine thread                                       disk thread ("recorder")
//!  RecorderSink: copy the armed channels of each  ─▶ rtrb ─▶ split every frame into its tracks,
//!  block (and the local mix, mixed with MixPlan)     ring    write them with WavWriter, refresh the
//!  into the take's ring                                      headers every second, sync every 5 s
//! ```
//!
//! A take is one folder, `<YYYY-MM-DD_HHMMSS>_<take name>`, never an existing one: a mono file per
//! armed input, a stereo file per armed linked pair (arming either side arms the pair), a stereo file
//! of the local mix when asked, and `take.json`, which describes it (written when the take starts and
//! again, atomically, when it ends). The track layout is fixed when the take starts. Tracks hold the
//! inputs exactly as captured, before any processing (the block's raw samples, before the channel
//! strips of [crate::processing]); the local mix is mixed here with [MixPlan] from the processed
//! block, as the local output mixes it, but from its own copy of the plan.
//!
//! The sink never allocates, locks or panics on the engine thread. The take's ring (a few seconds)
//! is allocated when the take starts, on the engine's side; the take reaches the sink through a
//! preallocated command ring and goes back through a return ring when it ends, to be freed on the
//! engine's side, so neither starting nor stopping frees anything on the engine thread. A block the
//! ring has no room for is dropped whole and counted: the take reports its dropped frames. Only the
//! disk thread touches the files.
//!
//! A disk error (a failed write, a full disk) stops only the take: the sink feeds it no more, the
//! file that failed keeps the audio of its last refresh, the other files are finished, and
//! `take.json` says why the take ended. Capture, the phones and the local output never notice.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rtrb::{Consumer, Producer, RingBuffer};
use serde::Serialize;

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::{ChannelKind, ChannelMeta};
use crate::mix::{MixPlan, MixRamp, MixState, OUTPUT_CHANNELS};
use crate::pipeline::{Block, Sink, MAX_BLOCK_FRAMES};
use crate::wav::{SampleFormat, WavError, WavSpec, WavTarget, WavWriter, HEADER_BYTES};

/// The seconds of audio a take's ring holds: what the disk may fall behind before frames drop.
const RING_SECONDS: usize = 4;

/// Takes (or stops) waiting for the sink to take them.
const COMMAND_SLOTS: usize = 4;

/// The most frames the disk thread takes out of the ring at once.
const CHUNK_FRAMES: usize = 4_096;

/// The local mix's file in a take folder.
const LOCAL_TRACK_FILE: &str = "Local mix.wav";

/// The take's description in its folder.
const TAKE_FILE: &str = "take.json";

/// The longest take or track name kept in a file name, in characters.
const MAX_NAME_CHARS: usize = 60;

/// The longest marker name, in characters.
const MAX_MARKER_CHARS: usize = 64;

/// How many folders named alike a take tries before giving up.
const MAX_FOLDER_ATTEMPTS: u32 = 100;

/// How long [crate::EngineHandle::stop_recording] waits for the files to be finished.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(10);

/// What a track holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    /// One input, in a mono file.
    Input,
    /// A linked pair of inputs, in one stereo file.
    Pair,
    /// The engine's local mix, in a stereo file.
    Local,
}

impl TrackKind {
    /// The kind's name in `take.json`.
    pub fn name(self) -> &'static str {
        match self {
            TrackKind::Input => "input",
            TrackKind::Pair => "pair",
            TrackKind::Local => "local",
        }
    }
}

/// One file of a take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeTrack {
    /// The file's name inside the take folder.
    pub file: String,
    pub kind: TrackKind,
    /// The inputs it holds, 1-based (CH 1 is 1), in file channel order; empty for the local mix.
    pub channels: Vec<u16>,
    /// The input's name when the take started (a pair's first input's), else "CH n".
    pub name: String,
    /// What the input carries when the take started; `None` for the local mix.
    pub instrument: Option<ChannelKind>,
    /// The frames the file holds once the take ended (what a reader of it finds); 0 while the
    /// take records. After a disk error the files may differ: the one that failed keeps the
    /// frames of its last refresh.
    pub frames: u64,
}

/// How a take ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TakeEnd {
    /// The engineer stopped it.
    Stopped,
    /// The engine stopped while it recorded.
    EngineStopped,
    /// Writing failed; the take stopped on its own.
    DiskError { reason: String },
}

/// A named point in a take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeMarker {
    /// The frame it marks, counted from the take's first frame: the first frame of the block
    /// being recorded when it was added.
    pub frame: u64,
    pub name: String,
}

/// What a take of a request would take on the disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiskEstimate {
    /// The bytes every second of the take adds, over all its files.
    pub bytes_per_second: u64,
    /// The free space on the folder's disk; `None` when it cannot be read.
    pub free_bytes: Option<u64>,
    /// The whole minutes that free space holds.
    pub minutes_left: Option<u64>,
}

/// A finished take, as `take.json` describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TakeSummary {
    pub folder: PathBuf,
    pub name: String,
    /// When the take started, UTC, RFC 3339 (`2026-10-16T19:04:05Z`).
    pub started: String,
    pub sample_rate: u32,
    pub format: SampleFormat,
    /// The input device the take was recorded from.
    pub device: String,
    /// Each with the frames its file holds.
    pub tracks: Vec<TakeTrack>,
    /// The frames every file holds: the fewest of the tracks' frames. All files hold the same
    /// unless a disk error stopped the take part-way through writing them.
    pub frames: u64,
    pub markers: Vec<TakeMarker>,
    /// Frames the disk could not keep up with, left out of every file.
    pub dropped_frames: u64,
    pub ended: TakeEnd,
}

/// What to record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordRequest {
    /// The folder takes are made in; created when missing.
    pub folder: PathBuf,
    /// The take's name, part of its folder's name (made safe for a file name).
    pub take_name: String,
    /// [SampleFormat::Int24] (the default) or [SampleFormat::Float32].
    pub format: SampleFormat,
    /// The inputs to record, 0-based. Arming either side of a linked pair records the pair as one
    /// stereo track.
    pub armed: Vec<usize>,
    /// Also record the local mix, as a stereo track.
    pub include_local: bool,
    /// The local time's offset from UTC, for the folder's name only (`take.json` is in UTC).
    pub utc_offset_minutes: i32,
}

/// Whether a take records.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum RecordingState {
    #[default]
    Idle,
    Recording,
    /// The take was asked to stop and its files are being finished.
    Stopping,
    /// The take stopped on its own (a disk error); [crate::EngineHandle::stop_recording] collects
    /// it.
    Failed {
        reason: String,
    },
}

/// The recorder as the status reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RecordingStatus {
    pub state: RecordingState,
    /// The take's folder, while there is a take.
    pub take_folder: Option<PathBuf>,
    /// Frames recorded so far.
    pub frames: u64,
    /// Frames dropped because the disk fell behind.
    pub dropped_frames: u64,
    /// Bytes in the take's files so far.
    pub bytes_written: u64,
    /// The take's track names, in file order.
    pub tracks: Vec<String>,
    /// The take's markers so far.
    pub markers: Vec<TakeMarker>,
    /// The free space on the take's disk; `None` without a take or when it cannot be read.
    pub free_bytes: Option<u64>,
    /// The whole minutes of this take the free space still holds.
    pub minutes_left: Option<u64>,
    /// The most recent take that ended, however it ended.
    pub last_take: Option<TakeSummary>,
}

/// The free space, in bytes, on the disk that holds `path` (or its nearest existing parent), for
/// this user; `None` when it cannot be read, and on systems other than Windows.
pub fn free_space(path: &Path) -> Option<u64> {
    let existing = path.ancestors().find(|folder| folder.is_dir())?;
    disk::free_bytes(existing)
}

#[cfg(windows)]
mod disk {
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        /// `GetDiskFreeSpaceExW`: each output is a `ULARGE_INTEGER`, which is a `u64`; any may be
        /// null.
        fn GetDiskFreeSpaceExW(
            directory: *const u16,
            free_to_caller: *mut u64,
            total: *mut u64,
            total_free: *mut u64,
        ) -> i32;
    }

    /// The bytes free to this user on the disk of `directory`, which exists.
    pub(super) fn free_bytes(directory: &Path) -> Option<u64> {
        let wide: Vec<u16> = directory.as_os_str().encode_wide().chain(Some(0)).collect();
        let mut free = 0u64;
        // SAFETY: `wide` is a NUL-terminated UTF-16 path that outlives the call, `free` is a u64
        // the call may write, and the other two outputs are null, which the call allows.
        let succeeded = unsafe {
            GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut free,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (succeeded != 0).then_some(free)
    }
}

#[cfg(not(windows))]
mod disk {
    use std::path::Path;

    /// The standard library reads no free space; elsewhere it is not reported.
    pub(super) fn free_bytes(_directory: &Path) -> Option<u64> {
        None
    }
}

/// The whole minutes `free_bytes` hold at `bytes_per_second`; `None` when nothing is written.
pub fn minutes_left(free_bytes: u64, bytes_per_second: u64) -> Option<u64> {
    (bytes_per_second > 0).then(|| free_bytes / bytes_per_second / 60)
}

/// The bytes a second of `tracks` takes in `format`, over all their files.
pub(crate) fn take_bytes_per_second(tracks: &[PlannedTrack], format: SampleFormat) -> u64 {
    tracks
        .iter()
        .map(|track| {
            WavSpec {
                channels: track.width() as u16,
                sample_rate: TARGET_SAMPLE_RATE,
                format,
            }
            .bytes_per_second()
        })
        .sum()
}

/// One track of a take being set up: its description and the source channels it holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PlannedTrack {
    pub(crate) track: TakeTrack,
    /// The 0-based source channels it holds; empty for the local mix.
    pub(crate) sources: Vec<usize>,
}

impl PlannedTrack {
    /// The samples this track takes from each recorded frame.
    fn width(&self) -> usize {
        match self.track.kind {
            TrackKind::Local => usize::from(OUTPUT_CHANNELS),
            TrackKind::Input | TrackKind::Pair => self.sources.len(),
        }
    }
}

/// The tracks for `armed` (0-based inputs) on a device with `source_channels` inputs whose details
/// are `details` (one entry per input, as the status reports them), in input order, with the local
/// mix last when `include_local`.
pub(crate) fn plan_tracks(
    details: &[ChannelMeta],
    source_channels: usize,
    armed: &[usize],
    include_local: bool,
) -> Result<Vec<PlannedTrack>, String> {
    if let Some(channel) = armed.iter().find(|channel| **channel >= source_channels) {
        return Err(format!(
            "CH {} cannot be armed: the input device has {source_channels} inputs",
            channel + 1
        ));
    }
    let detail = |channel: usize| details.get(channel).cloned().unwrap_or_default();
    let mut tracks = Vec::new();
    let mut channel = 0;
    while channel < source_channels {
        let meta = detail(channel);
        let pair = meta.linked_to_next && channel + 1 < source_channels;
        let sources: Vec<usize> = if pair {
            vec![channel, channel + 1]
        } else {
            vec![channel]
        };
        channel += sources.len();
        if !sources.iter().any(|source| armed.contains(source)) {
            continue;
        }
        let first = sources[0] + 1;
        let (fallback, prefix) = if pair {
            (
                format!("CH {first}-{}", first + 1),
                format!("{first:02}-{:02}", first + 1),
            )
        } else {
            (format!("CH {first}"), format!("{first:02}"))
        };
        let name = if meta.name.trim().is_empty() {
            fallback.clone()
        } else {
            meta.name.clone()
        };
        tracks.push(PlannedTrack {
            track: TakeTrack {
                file: format!("{prefix} {}.wav", sanitize(&name, &fallback)),
                kind: if pair {
                    TrackKind::Pair
                } else {
                    TrackKind::Input
                },
                channels: sources
                    .iter()
                    .map(|source| u16::try_from(source + 1).unwrap_or(u16::MAX))
                    .collect(),
                name,
                instrument: Some(meta.kind),
                frames: 0,
            },
            sources,
        });
    }
    if include_local {
        tracks.push(PlannedTrack {
            track: TakeTrack {
                file: LOCAL_TRACK_FILE.to_owned(),
                kind: TrackKind::Local,
                channels: Vec::new(),
                name: "Local mix".to_owned(),
                instrument: None,
                frames: 0,
            },
            sources: Vec::new(),
        });
    }
    if tracks.is_empty() {
        return Err("arm at least one input, or record the local mix".to_owned());
    }
    Ok(tracks)
}

/// `name` made safe for a file name: characters Windows forbids become `-`, at most
/// [MAX_NAME_CHARS] characters, no leading or trailing space and no trailing dot; `fallback` when
/// nothing is left.
pub(crate) fn sanitize(name: &str, fallback: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(
                    character,
                    '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
                )
            {
                '-'
            } else {
                character
            }
        })
        .take(MAX_NAME_CHARS)
        .collect();
    let trimmed = cleaned
        .trim_start()
        .trim_end_matches(|character: char| character == '.' || character.is_whitespace());
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// A calendar date and time of day.
struct Stamp {
    year: i64,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
}

/// `time` shifted by `offset_minutes`, as a calendar date and time (proleptic Gregorian). A time
/// before 1970 reads as 1970-01-01.
fn stamp(time: SystemTime, offset_minutes: i32) -> Stamp {
    let since = time.duration_since(UNIX_EPOCH).map_or(0, |since| {
        i64::try_from(since.as_secs()).unwrap_or(i64::MAX / 2)
    });
    let seconds = since + i64::from(offset_minutes) * 60;
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    let rest = seconds.rem_euclid(86_400);
    Stamp {
        year,
        month,
        day,
        hour: (rest / 3_600) as u32,
        minute: (rest % 3_600 / 60) as u32,
        second: (rest % 60) as u32,
    }
}

/// The date `days` after 1970-01-01 (H. Hinnant's civil-from-days algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// `time` in UTC as RFC 3339, to the second: `2026-10-16T19:04:05Z`.
pub(crate) fn rfc3339_utc(time: SystemTime) -> String {
    let at = stamp(time, 0);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        at.year, at.month, at.day, at.hour, at.minute, at.second
    )
}

/// Makes a new take folder in `parent` for a take started at `time`, named
/// `<YYYY-MM-DD_HHMMSS>_<take name>` in local time (`utc_offset_minutes` from UTC), with ` (2)`,
/// ` (3)`... when that folder exists: an existing folder is never used.
pub(crate) fn create_take_folder(
    parent: &Path,
    time: SystemTime,
    utc_offset_minutes: i32,
    take_name: &str,
) -> Result<PathBuf, String> {
    fs::create_dir_all(parent).map_err(|error| {
        format!(
            "could not make the recordings folder \"{}\": {error}",
            parent.display()
        )
    })?;
    let at = stamp(time, utc_offset_minutes);
    let base = format!(
        "{:04}-{:02}-{:02}_{:02}{:02}{:02}_{}",
        at.year,
        at.month,
        at.day,
        at.hour,
        at.minute,
        at.second,
        sanitize(take_name, "Take")
    );
    for attempt in 1..=MAX_FOLDER_ATTEMPTS {
        let name = if attempt == 1 {
            base.clone()
        } else {
            format!("{base} ({attempt})")
        };
        let folder = parent.join(name);
        // `create_dir` fails on an existing folder, so a take never lands in another one.
        match fs::create_dir(&folder) {
            Ok(()) => return Ok(folder),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(format!(
                    "could not make the take folder \"{}\": {error}",
                    folder.display()
                ))
            }
        }
    }
    Err(format!(
        "\"{}\" already holds {MAX_FOLDER_ATTEMPTS} takes named \"{base}\"",
        parent.display()
    ))
}

/// What a take shares between the sink, the disk thread and the engine.
#[derive(Default)]
pub(crate) struct TakeShared {
    /// Frames the sink put in the ring: the take's length so far. Dropped blocks are not in it,
    /// as they are in no file, so it is the file position of the next frame (and of a marker).
    frames: AtomicU64,
    /// Frames the ring had no room for.
    dropped: AtomicU64,
    /// Frames the disk thread has written to every file and refreshed.
    written: AtomicU64,
    /// Bytes in the take's files.
    bytes: AtomicU64,
    /// The sink has let go of the take: no frame follows.
    closed: AtomicBool,
    /// The disk thread stopped on an error: the sink feeds the take no more.
    failed: AtomicBool,
    /// Why the disk thread stopped. Never locked on the engine thread.
    failure: Mutex<Option<String>>,
    /// How the engine ended the take. Never locked on the engine thread.
    end: Mutex<Option<TakeEnd>>,
    /// The take's markers, in the order added. Never locked on the engine thread.
    markers: Mutex<MarkerBook>,
}

/// A take's markers, and whether it still takes new ones.
#[derive(Default)]
struct MarkerBook {
    list: Vec<TakeMarker>,
    /// The disk thread has taken the list for the take's end: no marker joins it any more, so
    /// what the status shows is what `take.json` lists.
    sealed: bool,
}

impl TakeShared {
    /// The markers' lock, taken back from a thread that panicked holding it: the list itself is
    /// never left half-changed (a push is the only change).
    fn marker_book(&self) -> std::sync::MutexGuard<'_, MarkerBook> {
        self.markers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn markers(&self) -> Vec<TakeMarker> {
        self.marker_book().list.clone()
    }

    /// The markers for the take's end, after which no marker is added.
    fn seal_markers(&self) -> Vec<TakeMarker> {
        let mut book = self.marker_book();
        book.sealed = true;
        book.list.clone()
    }

    /// No frame follows: the disk thread finishes the take.
    fn close(&self) {
        self.closed.store(true, Ordering::Release);
    }

    /// Records how the engine ended the take, unless that was already said.
    fn end_as(&self, end: TakeEnd) {
        if let Ok(mut stored) = self.end.lock() {
            stored.get_or_insert(end);
        }
    }

    /// Why the disk thread stopped the take, once it has.
    fn failure(&self) -> Option<String> {
        if !self.failed.load(Ordering::Acquire) {
            return None;
        }
        self.failure
            .lock()
            .ok()
            .and_then(|failure| failure.clone())
            .or_else(|| Some("the take could not be written".to_owned()))
    }

    /// Stops the take on `reason`: the sink feeds it no more from its next block.
    fn fail(&self, reason: String) {
        if let Ok(mut stored) = self.failure.lock() {
            stored.get_or_insert(reason);
        }
        self.failed.store(true, Ordering::Release);
    }
}

/// The sink's end of a take: its ring and the layout of what goes in it.
pub(crate) struct TakeFeed {
    producer: Producer<f32>,
    shared: Arc<TakeShared>,
    /// The source channels each recorded frame starts with, in track order.
    picks: Box<[usize]>,
    /// Each recorded frame ends with the local mix's left and right.
    local: bool,
    /// The channels of the source the take was set up for.
    source_channels: usize,
    /// Samples per recorded frame.
    width: usize,
}

impl Drop for TakeFeed {
    /// A feed let go of, wherever it ends (handed back, never sent, or held by an engine thread
    /// that ended), ends its take: no frame can follow.
    fn drop(&mut self) {
        self.shared.close();
    }
}

impl TakeFeed {
    /// Puts the armed channels of `block` as captured (its raw samples, before the channel
    /// processing) and the local mix in `stereo` (mixed from the processed block), when the take
    /// records it, into the ring, or drops the block whole and counts it when the ring has no room.
    fn record(&mut self, block: &Block<'_>, stereo: &[f32]) {
        let channels = block.channels.max(1);
        let frames = block.raw.len() / channels;
        if frames == 0 || self.shared.failed.load(Ordering::Acquire) {
            return;
        }
        // The layout is fixed when the take starts and the engine refuses a source change while
        // it records; a block of another layout is dropped rather than misread.
        if channels != self.source_channels || stereo.len() < frames * 2 {
            self.shared
                .dropped
                .fetch_add(frames as u64, Ordering::Relaxed);
            return;
        }
        let Ok(chunk) = self.producer.write_chunk_uninit(frames * self.width) else {
            self.shared
                .dropped
                .fetch_add(frames as u64, Ordering::Relaxed);
            return;
        };
        let picks = &self.picks;
        let mixed = if self.local { 2 } else { 0 };
        let samples = block
            .raw
            .chunks_exact(channels)
            .zip(stereo.chunks_exact(2))
            .flat_map(|(frame, mix)| {
                picks
                    .iter()
                    .map(move |pick| frame.get(*pick).copied().unwrap_or(0.0))
                    .chain(mix.iter().copied().take(mixed))
            });
        chunk.fill_from_iter(samples);
        self.shared
            .frames
            .fetch_add(frames as u64, Ordering::Release);
    }
}

/// A take's ring for `tracks` from a source of `source_channels` channels, holding `ring_frames`
/// recorded frames: the sink's end, the disk thread's end and what they share.
pub(crate) fn take_feed(
    tracks: &[PlannedTrack],
    source_channels: usize,
    ring_frames: usize,
) -> (TakeFeed, Consumer<f32>, Arc<TakeShared>) {
    let picks: Box<[usize]> = tracks
        .iter()
        .flat_map(|track| track.sources.iter().copied())
        .collect();
    let local = tracks
        .iter()
        .any(|track| track.track.kind == TrackKind::Local);
    let width = tracks.iter().map(PlannedTrack::width).sum::<usize>().max(1);
    let (producer, consumer) = RingBuffer::new(ring_frames.max(1) * width);
    let shared = Arc::new(TakeShared::default());
    let feed = TakeFeed {
        producer,
        shared: Arc::clone(&shared),
        picks,
        local,
        source_channels,
        width,
    };
    (feed, consumer, shared)
}

/// The recorder's sink on the engine thread.
pub(crate) struct RecorderSink {
    mix: Arc<MixState>,
    commands: Consumer<Option<TakeFeed>>,
    returns: Producer<TakeFeed>,
    /// A take the return ring had no room for, handed back at the next chance (the ring is sized
    /// so this cannot happen; it is the guard against dropping one here).
    parked: Option<TakeFeed>,
    feed: Option<TakeFeed>,
    /// Room for the local mix of the longest block.
    stereo: Box<[f32]>,
    /// The local mix's weights in the previous block mixed for a take, so a fader move glides
    /// (see [MixRamp]), and its master limiter's state. Its own, not the local output's: each is
    /// mixed and limited on its own. Reset for every take, whose first block has nothing before it.
    ramp: MixRamp,
}

/// The engine's end of the recorder sink's lines: takes go to the sink, and the takes it lets go of
/// come back here to be freed off the engine thread.
pub(crate) struct RecorderLines {
    commands: Producer<Option<TakeFeed>>,
    returns: Consumer<TakeFeed>,
}

/// The recorder's sink, mixing the local mix `mix` for a take that records it, and the engine's
/// end of its lines.
pub(crate) fn recorder_sink(mix: Arc<MixState>) -> (RecorderSink, RecorderLines) {
    let (command_producer, commands) = RingBuffer::new(COMMAND_SLOTS);
    let (returns, return_consumer) = RingBuffer::new(COMMAND_SLOTS + 1);
    let sink = RecorderSink {
        mix,
        commands,
        returns,
        parked: None,
        feed: None,
        stereo: vec![0.0; MAX_BLOCK_FRAMES * usize::from(OUTPUT_CHANNELS)].into_boxed_slice(),
        ramp: MixRamp::default(),
    };
    let lines = RecorderLines {
        commands: command_producer,
        returns: return_consumer,
    };
    (sink, lines)
}

impl RecorderLines {
    /// Sends a take to the sink (`None` stops it), after freeing the takes it gave back. An error
    /// when the engine thread has ended, or when changes come faster than the sink takes them (it
    /// takes them every block, or every idle wait without audio); a take not sent is let go of.
    pub(crate) fn send(&mut self, feed: Option<TakeFeed>) -> Result<(), String> {
        self.reap();
        if self.ended() {
            return Err("the engine thread has stopped".to_owned());
        }
        self.commands
            .push(feed)
            .map_err(|_| "the recorder is still changing; try again".to_owned())
    }

    /// Frees the takes the sink has let go of, here, off the engine thread; returns how many.
    pub(crate) fn reap(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(feed) = self.returns.pop() {
            drop(feed);
            freed += 1;
        }
        freed
    }

    /// The sink is gone with its engine thread.
    fn ended(&self) -> bool {
        self.commands.is_abandoned()
    }
}

impl RecorderSink {
    /// Takes the takes the engine sent, in order; one let go of is closed and handed back.
    fn take_commands(&mut self) {
        if let Some(parked) = self.parked.take() {
            self.hand_back(parked);
        }
        while let Ok(command) = self.commands.pop() {
            self.ramp.reset();
            if let Some(previous) = std::mem::replace(&mut self.feed, command) {
                previous.shared.close();
                self.hand_back(previous);
            }
        }
    }

    fn hand_back(&mut self, feed: TakeFeed) {
        if let Err(rtrb::PushError::Full(feed)) = self.returns.push(feed) {
            self.parked = Some(feed);
        }
    }
}

impl Sink for RecorderSink {
    fn process(&mut self, block: &Block<'_>) {
        self.take_commands();
        let Self {
            mix,
            feed,
            stereo,
            ramp,
            ..
        } = self;
        let Some(feed) = feed else {
            return;
        };
        let channels = block.channels.max(1);
        let frames = block.samples.len() / channels;
        let stereo = stereo
            .get_mut(..frames * usize::from(OUTPUT_CHANNELS))
            .unwrap_or_default();
        if feed.local && !stereo.is_empty() {
            MixPlan::new(&mix.snapshot(), channels).mix_master(
                ramp,
                mix.limiter(),
                mix.fx_returns(),
                &block.wet,
                block.samples,
                stereo,
            );
        }
        feed.record(block, stereo);
    }

    /// Takes a take, or its stop, while no audio comes.
    fn silence(&mut self, _frames: usize) {
        self.take_commands();
    }
}

/// How often the disk thread hands the files their audio.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DiskTimings {
    /// Between header refreshes: what a crash can lose.
    pub(crate) refresh: Duration,
    /// Between syncs to the disk itself.
    pub(crate) sync: Duration,
    /// Between looks at the ring.
    pub(crate) poll: Duration,
}

impl Default for DiskTimings {
    fn default() -> Self {
        Self {
            refresh: Duration::from_secs(1),
            sync: Duration::from_secs(5),
            poll: Duration::from_millis(20),
        }
    }
}

/// How a take is set up: its ring and its disk thread's timings.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TakeOptions {
    pub(crate) ring_frames: usize,
    pub(crate) timings: DiskTimings,
}

impl Default for TakeOptions {
    fn default() -> Self {
        Self {
            ring_frames: RING_SECONDS * TARGET_SAMPLE_RATE as usize,
            timings: DiskTimings::default(),
        }
    }
}

/// Opens a track's file: [FileOpener] on the disk; a test opens files that fail on demand.
pub(crate) trait TrackOpener: Send + 'static {
    type Target: WavTarget + Send + 'static;

    fn open(&mut self, path: &Path, spec: WavSpec) -> Result<WavWriter<Self::Target>, WavError>;
}

/// New files on the disk, never overwriting one.
pub(crate) struct FileOpener;

impl TrackOpener for FileOpener {
    type Target = File;

    fn open(&mut self, path: &Path, spec: WavSpec) -> Result<WavWriter<File>, WavError> {
        WavWriter::create(path, spec)
    }
}

/// What `take.json` says of a take besides what the disk thread counts.
#[derive(Clone, Debug)]
struct TakeInfo {
    folder: PathBuf,
    name: String,
    started: String,
    format: SampleFormat,
    device: String,
    tracks: Vec<TakeTrack>,
}

impl TakeInfo {
    /// The take whose files hold `track_frames` (one count per track, in order), with `markers`
    /// and the dropped frames `shared` counted, that ended as `ended`. Its frames are the frames
    /// every file holds.
    fn summary(
        &self,
        shared: &TakeShared,
        track_frames: &[u64],
        markers: Vec<TakeMarker>,
        ended: TakeEnd,
    ) -> TakeSummary {
        let tracks: Vec<TakeTrack> = self
            .tracks
            .iter()
            .zip(track_frames.iter().copied().chain(std::iter::repeat(0)))
            .map(|(track, frames)| TakeTrack {
                frames,
                ..track.clone()
            })
            .collect();
        TakeSummary {
            folder: self.folder.clone(),
            name: self.name.clone(),
            started: self.started.clone(),
            sample_rate: TARGET_SAMPLE_RATE,
            format: self.format,
            device: self.device.clone(),
            frames: tracks.iter().map(|track| track.frames).min().unwrap_or(0),
            tracks,
            markers,
            dropped_frames: shared.dropped.load(Ordering::Relaxed),
            ended,
        }
    }
}

/// `take.json`, version 1.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TakeJson<'a> {
    version: u32,
    name: &'a str,
    started: &'a str,
    sample_rate: u32,
    format: &'static str,
    device: &'a str,
    tracks: Vec<TrackJson<'a>>,
    frames: u64,
    markers: Vec<MarkerJson<'a>>,
    dropped_frames: u64,
    /// `null` while the take records: a take a crash ended says so.
    ended: Option<EndJson<'a>>,
}

#[derive(Serialize)]
struct MarkerJson<'a> {
    frame: u64,
    name: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackJson<'a> {
    file: &'a str,
    kind: &'static str,
    channels: &'a [u16],
    name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    instrument: Option<&'static str>,
    frames: u64,
}

#[derive(Serialize)]
#[serde(tag = "how", rename_all = "camelCase")]
enum EndJson<'a> {
    Stopped,
    EngineStopped,
    DiskError { reason: &'a str },
}

fn format_name(format: SampleFormat) -> &'static str {
    match format {
        SampleFormat::Int16 => "int16",
        SampleFormat::Int24 => "int24",
        SampleFormat::Int32 => "int32",
        SampleFormat::Float32 => "float32",
    }
}

/// Writes `take.json` for `summary` (`ended` left `null` while the take records) through a
/// temporary file put on the disk and renamed over it, so a reader never sees half of one.
fn write_take_file(summary: &TakeSummary, recording: bool) -> io::Result<()> {
    let json = TakeJson {
        version: 1,
        name: &summary.name,
        started: &summary.started,
        sample_rate: summary.sample_rate,
        format: format_name(summary.format),
        device: &summary.device,
        tracks: summary
            .tracks
            .iter()
            .map(|track| TrackJson {
                file: &track.file,
                kind: track.kind.name(),
                channels: &track.channels,
                name: &track.name,
                instrument: track.instrument.map(ChannelKind::name),
                frames: track.frames,
            })
            .collect(),
        frames: summary.frames,
        markers: summary
            .markers
            .iter()
            .map(|marker| MarkerJson {
                frame: marker.frame,
                name: &marker.name,
            })
            .collect(),
        dropped_frames: summary.dropped_frames,
        ended: (!recording).then_some(match &summary.ended {
            TakeEnd::Stopped => EndJson::Stopped,
            TakeEnd::EngineStopped => EndJson::EngineStopped,
            TakeEnd::DiskError { reason } => EndJson::DiskError { reason },
        }),
    };
    let temporary = summary.folder.join(format!("{TAKE_FILE}.tmp"));
    let mut file = File::create(&temporary)?;
    serde_json::to_writer_pretty(&mut file, &json).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temporary, summary.folder.join(TAKE_FILE))
}

/// One track's file on the disk thread.
struct TrackFile<T: WavTarget> {
    /// `None` once it failed: it keeps the header of its last refresh.
    writer: Option<WavWriter<T>>,
    file: String,
    /// The samples it takes from each recorded frame.
    width: usize,
    /// Its samples of the frames being written.
    buffer: Vec<f32>,
    /// The frames its header counts (its last refresh, or its finish): what a reader finds.
    held: u64,
}

impl<T: WavTarget> TrackFile<T> {
    /// Runs `step` on the writer; a failure is described with the file's name.
    fn with(
        &mut self,
        step: impl FnOnce(&mut WavWriter<T>) -> Result<(), WavError>,
    ) -> Result<(), String> {
        match &mut self.writer {
            Some(writer) => {
                step(writer).map_err(|error| format!("could not write \"{}\": {error}", self.file))
            }
            None => Ok(()),
        }
    }

    fn bytes(&self) -> u64 {
        self.writer.as_ref().map_or(0, |writer| {
            HEADER_BYTES + writer.frames() * u64::from(writer.spec().bytes_per_frame())
        })
    }
}

/// The disk thread: takes the recorded frames out of the ring, splits them into the tracks and
/// writes them, refreshes the headers every `timings.refresh` and syncs every `timings.sync`,
/// until the sink lets go of the take; then finishes every file and writes `take.json`. On a
/// failed write it stops the take, finishes the other files, and says why in `take.json`.
fn run_disk<T: WavTarget>(
    mut ring: Consumer<f32>,
    mut files: Vec<TrackFile<T>>,
    shared: Arc<TakeShared>,
    info: TakeInfo,
    timings: DiskTimings,
) -> TakeSummary {
    let width = files.iter().map(|file| file.width).sum::<usize>().max(1);
    let mut written = 0u64;
    let mut refreshed = Instant::now();
    let mut synced = refreshed;
    let mut failure = None;
    loop {
        // Read before draining: every frame pushed before the sink let go is in the ring now.
        let closing = shared.closed.load(Ordering::Acquire) || ring.is_abandoned();
        if let Err(reason) = drain(&mut ring, &mut files, width, &mut written) {
            failure = Some(reason);
            break;
        }
        if closing {
            break;
        }
        let now = Instant::now();
        let keep = if now.duration_since(synced) >= timings.sync {
            synced = now;
            refreshed = now;
            Some(true)
        } else if now.duration_since(refreshed) >= timings.refresh {
            refreshed = now;
            Some(false)
        } else {
            None
        };
        if let Some(sync) = keep {
            if let Err(reason) = keep_files(&mut files, sync) {
                failure = Some(reason);
                break;
            }
            shared.written.store(written, Ordering::Release);
        }
        shared
            .bytes
            .store(files.iter().map(TrackFile::bytes).sum(), Ordering::Relaxed);
        thread::park_timeout(timings.poll);
    }
    if let Some(reason) = &failure {
        // The sink stops feeding the take at its next block; the failed file keeps its last
        // refresh, and the others are finished below.
        shared.fail(reason.clone());
    }
    shared
        .bytes
        .store(files.iter().map(TrackFile::bytes).sum(), Ordering::Relaxed);
    for file in &mut files {
        if let Some(writer) = file.writer.take() {
            let frames = writer.frames();
            match writer.finish() {
                Ok(_) => file.held = frames,
                Err(error) => {
                    failure.get_or_insert_with(|| {
                        format!("could not finish \"{}\": {error}", file.file)
                    });
                }
            }
        }
    }
    shared.written.store(written, Ordering::Release);
    let ended = match failure {
        Some(reason) => TakeEnd::DiskError { reason },
        None => shared
            .end
            .lock()
            .ok()
            .and_then(|end| end.clone())
            .unwrap_or(TakeEnd::Stopped),
    };
    let track_frames: Vec<u64> = files.iter().map(|file| file.held).collect();
    // From here no marker joins the take, so take.json lists every marker the status showed.
    let markers = shared.seal_markers();
    let mut summary = info.summary(&shared, &track_frames, markers, ended);
    if let Err(error) = write_take_file(&summary, false) {
        if !matches!(summary.ended, TakeEnd::DiskError { .. }) {
            summary.ended = TakeEnd::DiskError {
                reason: format!("could not write {TAKE_FILE}: {error}"),
            };
        }
    }
    if let TakeEnd::DiskError { reason } = &summary.ended {
        shared.fail(reason.clone());
    }
    summary
}

/// Writes every whole frame waiting in the ring to the tracks; counts them into `written`.
fn drain<T: WavTarget>(
    ring: &mut Consumer<f32>,
    files: &mut [TrackFile<T>],
    width: usize,
    written: &mut u64,
) -> Result<(), String> {
    loop {
        let frames = (ring.slots() / width).min(CHUNK_FRAMES);
        if frames == 0 {
            return Ok(());
        }
        let Ok(chunk) = ring.read_chunk(frames * width) else {
            return Ok(());
        };
        for file in files.iter_mut() {
            file.buffer.clear();
        }
        let (head, tail) = chunk.as_slices();
        let mut samples = head.iter().chain(tail.iter()).copied();
        for _ in 0..frames {
            for file in files.iter_mut() {
                file.buffer.extend(samples.by_ref().take(file.width));
            }
        }
        chunk.commit_all();
        for file in files.iter_mut() {
            let samples = std::mem::take(&mut file.buffer);
            let result = file.with(|writer| writer.write_frames(&samples));
            file.buffer = samples;
            if result.is_err() {
                // The writer stopped itself: its file keeps the header of its last refresh.
                file.writer = None;
                return result;
            }
        }
        *written += frames as u64;
    }
}

/// Refreshes every file's header, and puts it on the disk when `sync`.
fn keep_files<T: WavTarget>(files: &mut [TrackFile<T>], sync: bool) -> Result<(), String> {
    for file in files.iter_mut() {
        let result = file.with(|writer| {
            if sync {
                writer.sync()
            } else {
                writer.refresh()
            }
        });
        if result.is_err() {
            // The writer stopped itself: its file keeps the header of its last refresh.
            file.writer = None;
            return result;
        }
        if let Some(writer) = &file.writer {
            file.held = writer.frames();
        }
    }
    Ok(())
}

/// A take in progress, as the engine holds it.
pub(crate) struct RunningTake {
    shared: Arc<TakeShared>,
    disk: Option<JoinHandle<TakeSummary>>,
    info: TakeInfo,
    /// The bytes a second of the take adds over all its files.
    bytes_per_second: u64,
    /// The engine was asked to stop it.
    stopping: bool,
}

impl RunningTake {
    fn disk_finished(&self) -> bool {
        self.disk.as_ref().is_none_or(JoinHandle::is_finished)
    }

    fn wake(&self) {
        if let Some(disk) = &self.disk {
            disk.thread().unpark();
        }
    }

    /// Waits for the disk thread and returns the take it finished.
    fn join(mut self) -> TakeSummary {
        match self.disk.take().map(JoinHandle::join) {
            Some(Ok(summary)) => summary,
            // The disk thread ended without saying what its files hold: their last refresh
            // covered at least the frames it reported written.
            _ => self.info.summary(
                &self.shared,
                &vec![self.shared.written.load(Ordering::Acquire); self.info.tracks.len()],
                self.shared.seal_markers(),
                TakeEnd::DiskError {
                    reason: "the recorder's disk thread failed".to_owned(),
                },
            ),
        }
    }
}

/// Sets up a take of `tracks` from `device` (a source of `source_channels` channels), started at
/// `started`: its folder, its files (opened by `opener`), its `take.json`, its ring and its disk
/// thread. Nothing is left behind when it fails.
pub(crate) fn open_take<O: TrackOpener>(
    request: &RecordRequest,
    device: &str,
    tracks: Vec<PlannedTrack>,
    source_channels: usize,
    started: SystemTime,
    mut opener: O,
    options: TakeOptions,
) -> Result<(TakeFeed, RunningTake), String> {
    if !matches!(request.format, SampleFormat::Int24 | SampleFormat::Float32) {
        return Err("takes are recorded as 24-bit integer or 32-bit float".to_owned());
    }
    let folder = create_take_folder(
        &request.folder,
        started,
        request.utc_offset_minutes,
        &request.take_name,
    )?;
    let info = TakeInfo {
        folder: folder.clone(),
        name: request.take_name.clone(),
        started: rfc3339_utc(started),
        format: request.format,
        device: device.to_owned(),
        tracks: tracks.iter().map(|planned| planned.track.clone()).collect(),
    };
    let mut files = Vec::with_capacity(tracks.len());
    for planned in &tracks {
        let spec = WavSpec {
            channels: planned.width() as u16,
            sample_rate: TARGET_SAMPLE_RATE,
            format: request.format,
        };
        match opener.open(&folder.join(&planned.track.file), spec) {
            Ok(writer) => files.push(TrackFile {
                writer: Some(writer),
                file: planned.track.file.clone(),
                width: planned.width(),
                buffer: Vec::with_capacity(CHUNK_FRAMES * planned.width()),
                held: 0,
            }),
            Err(error) => {
                drop(files);
                let _ = fs::remove_dir_all(&folder);
                return Err(format!(
                    "could not make \"{}\": {error}",
                    folder.join(&planned.track.file).display()
                ));
            }
        }
    }
    let fresh = TakeShared::default();
    let opening = info.summary(&fresh, &[], Vec::new(), TakeEnd::Stopped);
    if let Err(error) = write_take_file(&opening, true) {
        drop(files);
        let _ = fs::remove_dir_all(&folder);
        return Err(format!("could not write {TAKE_FILE}: {error}"));
    }
    let (feed, ring, shared) = take_feed(&tracks, source_channels, options.ring_frames);
    let disk_shared = Arc::clone(&shared);
    let disk_info = info.clone();
    let disk = thread::Builder::new()
        .name("recorder".to_owned())
        .spawn(move || run_disk(ring, files, disk_shared, disk_info, options.timings));
    let disk = match disk {
        Ok(disk) => disk,
        Err(error) => {
            drop(feed);
            let _ = fs::remove_dir_all(&folder);
            return Err(format!("could not start the recorder: {error}"));
        }
    };
    Ok((
        feed,
        RunningTake {
            shared,
            disk: Some(disk),
            info,
            bytes_per_second: take_bytes_per_second(&tracks, request.format),
            stopping: false,
        },
    ))
}

/// The engine's side of the recorder: the lines to its sink and the take, if any.
pub(crate) struct Recorder {
    lines: RecorderLines,
    take: Option<RunningTake>,
    last: Option<TakeSummary>,
}

impl Recorder {
    pub(crate) fn new(lines: RecorderLines) -> Self {
        Self {
            lines,
            take: None,
            last: None,
        }
    }

    /// An error while a take records (or is finishing): its source must not change.
    pub(crate) fn refuse_source_change(&self) -> Result<(), String> {
        match &self.take {
            Some(take) if take.shared.failure().is_none() => Err(
                "a take is recording: stop the recording before switching the input device (a take never mixes two devices)"
                    .to_owned(),
            ),
            _ => Ok(()),
        }
    }

    /// An error while a take records; a take that stopped on its own is collected first.
    pub(crate) fn check_idle(&mut self) -> Result<(), String> {
        if self
            .take
            .as_ref()
            .is_some_and(|take| take.shared.failure().is_some() && take.disk_finished())
        {
            self.request_stop(TakeEnd::Stopped)?;
            self.wait(STOP_WAIT)?;
        }
        if self.take.is_some() {
            return Err("a take is already recording".to_owned());
        }
        Ok(())
    }

    /// Hands `feed` to the sink and keeps `take` as the take that records. When the sink cannot
    /// take it, the take is ended and its folder removed.
    pub(crate) fn start(&mut self, feed: TakeFeed, take: RunningTake) -> Result<(), String> {
        if let Err(error) = self.lines.send(Some(feed)) {
            // The feed was let go of, so the disk thread finishes the empty take.
            take.wake();
            let folder = take.info.folder.clone();
            take.join();
            let _ = fs::remove_dir_all(folder);
            return Err(error);
        }
        self.take = Some(take);
        Ok(())
    }

    /// Asks the sink to let go of the take, which ends as `end` (unless the disk ended it).
    pub(crate) fn request_stop(&mut self, end: TakeEnd) -> Result<(), String> {
        let Some(take) = &mut self.take else {
            return Err("nothing is recording".to_owned());
        };
        if take.stopping {
            return Ok(());
        }
        take.shared.end_as(end);
        if self.lines.ended() {
            // No engine thread feeds the take any more.
            take.shared.close();
        } else {
            self.lines.send(None)?;
        }
        take.stopping = true;
        take.wake();
        Ok(())
    }

    /// Waits up to `wait` for the take's files to be finished, and returns it. An error, with
    /// the take still finishing, when that takes longer.
    pub(crate) fn wait(&mut self, wait: Duration) -> Result<TakeSummary, String> {
        let deadline = Instant::now() + wait;
        loop {
            self.lines.reap();
            let Some(take) = &self.take else {
                return Err("nothing is recording".to_owned());
            };
            if take.disk_finished() {
                break;
            }
            if self.lines.ended() {
                take.shared.close();
            }
            if Instant::now() >= deadline {
                return Err("the take's files are still being finished; try again".to_owned());
            }
            take.wake();
            thread::sleep(Duration::from_millis(2));
        }
        let Some(take) = self.take.take() else {
            return Err("nothing is recording".to_owned());
        };
        let summary = take.join();
        self.last = Some(summary.clone());
        Ok(summary)
    }

    /// Ends the take after the engine thread has ended: nothing feeds it any more.
    pub(crate) fn engine_stopped(&mut self) {
        if let Some(take) = self.take.take() {
            take.shared.end_as(TakeEnd::EngineStopped);
            take.shared.close();
            take.wake();
            self.last = Some(take.join());
        }
        self.lines.reap();
    }

    pub(crate) fn status(&self) -> RecordingStatus {
        let Some(take) = &self.take else {
            return RecordingStatus {
                last_take: self.last.clone(),
                ..RecordingStatus::default()
            };
        };
        let state = match take.shared.failure() {
            Some(reason) => RecordingState::Failed { reason },
            None if take.stopping => RecordingState::Stopping,
            None => RecordingState::Recording,
        };
        let free_bytes = free_space(&take.info.folder);
        RecordingStatus {
            state,
            take_folder: Some(take.info.folder.clone()),
            frames: take.shared.frames.load(Ordering::Acquire),
            dropped_frames: take.shared.dropped.load(Ordering::Relaxed),
            bytes_written: take.shared.bytes.load(Ordering::Relaxed),
            tracks: take
                .info
                .tracks
                .iter()
                .map(|track| track.name.clone())
                .collect(),
            markers: take.shared.markers(),
            free_bytes,
            minutes_left: free_bytes.and_then(|free| minutes_left(free, take.bytes_per_second)),
            last_take: self.last.clone(),
        }
    }

    /// Marks the frame being recorded with `name` (at most [MAX_MARKER_CHARS] characters; when
    /// blank, the first `Marker n` from the marker count on that no marker has): the first frame
    /// of the block the sink records next, a position in the files (dropped blocks are in no
    /// file), so a marker is accurate to the block. An error when no take records, or once the
    /// take stopped taking markers (asked to stop, failed, or ended).
    pub(crate) fn add_marker(&self, name: &str) -> Result<TakeMarker, String> {
        let Some(take) = self.take.as_ref().filter(|take| !take.stopping) else {
            return Err("no take is recording".to_owned());
        };
        // Checked under the lock the disk thread seals the markers with when it builds the take's
        // end, and after a failure is stored, so a marker the status shows is always in take.json.
        let mut book = take.shared.marker_book();
        if book.sealed {
            return Err("the take is no longer recording".to_owned());
        }
        if let Some(reason) = take.shared.failure() {
            return Err(format!("the take stopped recording: {reason}"));
        }
        let trimmed = name.trim();
        let name = if trimmed.is_empty() {
            (book.list.len() + 1..)
                .map(|number| format!("Marker {number}"))
                .find(|candidate| book.list.iter().all(|marker| marker.name != *candidate))
                .unwrap_or_else(|| "Marker".to_owned())
        } else {
            trimmed.chars().take(MAX_MARKER_CHARS).collect()
        };
        let marker = TakeMarker {
            frame: take.shared.frames.load(Ordering::Acquire),
            name,
        };
        book.list.push(marker.clone());
        Ok(marker)
    }

    /// The frames the disk thread has written to every file and refreshed.
    #[cfg(test)]
    pub(crate) fn written(&self) -> u64 {
        self.take
            .as_ref()
            .map_or(0, |take| take.shared.written.load(Ordering::Acquire))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::pipeline::tests::allocations;
    use crate::pipeline::SourceFormat;
    use crate::wav::WavReader;
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom};

    /// An empty scratch folder for one test's files.
    pub(crate) fn scratch_directory(test: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bandear-recorder-tests")
            .join(test);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn meta(name: &str, kind: ChannelKind) -> ChannelMeta {
        ChannelMeta {
            name: name.to_owned(),
            kind,
            ..ChannelMeta::default()
        }
    }

    /// Four inputs: CH 1 "Vocal", CH 2 unnamed, CH 3-4 "Keys" as a linked pair.
    fn four_inputs() -> Vec<ChannelMeta> {
        vec![
            meta("Vocal", ChannelKind::Voice),
            ChannelMeta::default(),
            ChannelMeta {
                linked_to_next: true,
                ..meta("Keys", ChannelKind::Keys)
            },
            meta("", ChannelKind::Keys),
        ]
    }

    pub(crate) fn request(folder: &Path, armed: Vec<usize>, format: SampleFormat) -> RecordRequest {
        RecordRequest {
            folder: folder.to_owned(),
            take_name: "Soundcheck".to_owned(),
            format,
            armed,
            include_local: false,
            utc_offset_minutes: 0,
        }
    }

    fn block(samples: &[f32], channels: usize) -> Block<'_> {
        Block {
            samples,
            raw: samples,
            wet: crate::effects::Wet::NONE,
            channels,
            sequence: 0,
        }
    }

    /// `frames` frames of `channels` channels in which every sample tells where it is: channel `c`
    /// of frame `f` (counted from `first`) is `(c + 1) / 10 + f / 10_000`.
    fn numbered(first: usize, frames: usize, channels: usize) -> Vec<f32> {
        (first..first + frames)
            .flat_map(|frame| {
                (0..channels)
                    .map(move |channel| (channel + 1) as f32 / 10.0 + frame as f32 / 10_000.0)
            })
            .collect()
    }

    /// Every frame of the WAV file at `path`, and its channel count.
    fn read_wav(path: &Path) -> (u16, Vec<f32>) {
        let mut reader = WavReader::open(path).expect("the file reads");
        let channels = reader.spec().channels;
        let mut samples = vec![0.0; reader.frames() as usize * usize::from(channels)];
        let frames = reader.read_frames(&mut samples).expect("the audio reads");
        samples.truncate(frames * usize::from(channels));
        (channels, samples)
    }

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_reader(File::open(path).expect("take.json exists"))
            .expect("take.json is JSON")
    }

    /// The start of the tests' takes: 2023-11-14 22:13:20 UTC.
    fn started() -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(1_700_000_000)
    }

    /// Waits until `done` holds, for at most five seconds.
    pub(crate) fn until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        done()
    }

    /// Timings that refresh the headers after every look at the ring.
    pub(crate) fn eager() -> TakeOptions {
        TakeOptions {
            ring_frames: 4_800,
            timings: DiskTimings {
                refresh: Duration::ZERO,
                sync: Duration::from_secs(5),
                poll: Duration::from_millis(1),
            },
        }
    }

    #[test]
    fn armed_inputs_pairs_and_the_local_mix_become_tracks() {
        // CH 1, CH 4 (the second side of the Keys pair, which arms the pair) and the local mix.
        let tracks = plan_tracks(&four_inputs(), 4, &[3, 0, 3], true).expect("a valid plan");

        let described: Vec<_> = tracks
            .iter()
            .map(|planned| {
                (
                    planned.track.file.as_str(),
                    planned.track.kind,
                    planned.track.channels.clone(),
                    planned.track.name.as_str(),
                    planned.track.instrument,
                    planned.sources.clone(),
                )
            })
            .collect();
        assert_eq!(
            described,
            vec![
                (
                    "01 Vocal.wav",
                    TrackKind::Input,
                    vec![1],
                    "Vocal",
                    Some(ChannelKind::Voice),
                    vec![0]
                ),
                (
                    "03-04 Keys.wav",
                    TrackKind::Pair,
                    vec![3, 4],
                    "Keys",
                    Some(ChannelKind::Keys),
                    vec![2, 3]
                ),
                (
                    "Local mix.wav",
                    TrackKind::Local,
                    vec![],
                    "Local mix",
                    None,
                    vec![]
                ),
            ]
        );

        // An unnamed input is named by its number; inputs past the details have the defaults.
        let unnamed = plan_tracks(&four_inputs(), 6, &[1, 5], false).expect("a valid plan");
        assert_eq!(unnamed[0].track.file, "02 CH 2.wav");
        assert_eq!(unnamed[0].track.name, "CH 2");
        assert_eq!(unnamed[0].track.instrument, Some(ChannelKind::Other));
        assert_eq!(unnamed[1].track.file, "06 CH 6.wav");
    }

    #[test]
    fn arming_an_input_the_device_lacks_or_nothing_at_all_is_refused() {
        let past = plan_tracks(&four_inputs(), 4, &[4], false).expect_err("CH 5 is not there");
        assert!(past.contains("CH 5"), "{past}");
        let nothing = plan_tracks(&four_inputs(), 4, &[], false).expect_err("nothing to record");
        assert!(nothing.contains("arm"), "{nothing}");
        assert_eq!(
            plan_tracks(&four_inputs(), 4, &[], true)
                .expect("the local mix alone is a take")
                .len(),
            1
        );
    }

    #[test]
    fn names_are_made_safe_for_file_names() {
        assert_eq!(
            sanitize("Sunday: live / take?", "Take"),
            "Sunday- live - take-"
        );
        assert_eq!(sanitize("  Encore... ", "Take"), "Encore");
        assert_eq!(sanitize("a\tb", "Take"), "a-b");
        assert_eq!(sanitize(" . ", "Take"), "Take");
        assert_eq!(sanitize(&"x".repeat(80), "Take").chars().count(), 60);
    }

    #[test]
    fn the_start_time_is_written_in_rfc_3339_utc() {
        assert_eq!(rfc3339_utc(UNIX_EPOCH), "1970-01-01T00:00:00Z");
        assert_eq!(
            rfc3339_utc(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "2000-02-29T00:00:00Z"
        );
        assert_eq!(rfc3339_utc(started()), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn a_take_folder_is_named_by_its_start_and_take_and_never_reused() {
        let parent = scratch_directory("folders").join("takes");

        let first = create_take_folder(&parent, started(), 0, "Soundcheck").expect("created");
        let second = create_take_folder(&parent, started(), 0, "Soundcheck").expect("created");
        let local = create_take_folder(&parent, started(), 120, "Show: 2").expect("created");

        let name = |path: &Path| path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name(&first), "2023-11-14_221320_Soundcheck");
        assert_eq!(name(&second), "2023-11-14_221320_Soundcheck (2)");
        assert_eq!(name(&local), "2023-11-15_001320_Show- 2");
        assert!(first.is_dir() && second.is_dir() && local.is_dir());
    }

    /// A take driven through the sink and its disk thread, as the engine drives it.
    struct Bench {
        sink: RecorderSink,
        recorder: Recorder,
        mix: Arc<MixState>,
    }

    fn bench() -> Bench {
        let mix = Arc::new(MixState::default());
        let (mut sink, lines) = recorder_sink(Arc::clone(&mix));
        sink.source_changed(SourceFormat {
            channels: 4,
            block_frames: 64,
        });
        Bench {
            sink,
            recorder: Recorder::new(lines),
            mix,
        }
    }

    impl Bench {
        fn start<O: TrackOpener>(&mut self, request: &RecordRequest, opener: O) -> PathBuf {
            self.start_with(request, opener, eager())
        }

        fn start_with<O: TrackOpener>(
            &mut self,
            request: &RecordRequest,
            opener: O,
            options: TakeOptions,
        ) -> PathBuf {
            let tracks = plan_tracks(&four_inputs(), 4, &request.armed, request.include_local)
                .expect("a valid plan");
            let (feed, take) =
                open_take(request, "Test input", tracks, 4, started(), opener, options)
                    .expect("the take opens");
            let folder = take.info.folder.clone();
            self.recorder.start(feed, take).expect("the sink listens");
            folder
        }

        fn stop(&mut self) -> TakeSummary {
            self.recorder
                .request_stop(TakeEnd::Stopped)
                .expect("the sink listens");
            // The engine thread takes the stop with its next block or idle wait.
            self.sink.silence(0);
            self.recorder.wait(STOP_WAIT).expect("the take finishes")
        }
    }

    /// Each armed input goes to its own mono file, a pair to one stereo file and the local mix to
    /// its own stereo file, sample for sample, and `take.json` describes the take.
    #[test]
    fn every_track_gets_its_own_channels_and_take_json_describes_the_take() {
        let parent = scratch_directory("tracks");
        let mut bench = bench();
        let request = RecordRequest {
            include_local: true,
            ..request(&parent, vec![0, 2], SampleFormat::Float32)
        };
        let folder = bench.start(&request, FileOpener);
        assert_eq!(
            bench.recorder.status().state,
            RecordingState::Recording,
            "the take records"
        );
        let input = numbered(0, 3 * 64, 4);
        for samples in input.chunks(64 * 4) {
            bench.sink.process(&block(samples, 4));
        }

        let summary = bench.stop();

        let channel = |samples: &[f32], channel: usize| -> Vec<f32> {
            samples.chunks(4).map(|frame| frame[channel]).collect()
        };
        let (channels, vocal) = read_wav(&folder.join("01 Vocal.wav"));
        assert_eq!((channels, vocal), (1, channel(&input, 0)));
        let (channels, keys) = read_wav(&folder.join("03-04 Keys.wav"));
        assert_eq!(channels, 2);
        let pairs: Vec<f32> = input
            .chunks(4)
            .flat_map(|frame| [frame[2], frame[3]])
            .collect();
        assert_eq!(keys, pairs);
        let (channels, local) = read_wav(&folder.join(LOCAL_TRACK_FILE));
        let mut mixed = vec![0.0; 3 * 64 * 2];
        MixPlan::new(&bench.mix.snapshot(), 4).mix_into(&input, &mut mixed);
        assert_eq!((channels, local), (2, mixed));

        assert_eq!(summary.frames, 192);
        assert_eq!(summary.dropped_frames, 0);
        assert_eq!(summary.ended, TakeEnd::Stopped);
        assert_eq!(summary.folder, folder);
        assert_eq!(summary.started, "2023-11-14T22:13:20Z");
        let json = read_json(&folder.join(TAKE_FILE));
        assert_eq!(json["version"], 1);
        assert_eq!(json["name"], "Soundcheck");
        assert_eq!(json["started"], "2023-11-14T22:13:20Z");
        assert_eq!(json["sampleRate"], 48_000);
        assert_eq!(json["format"], "float32");
        assert_eq!(json["device"], "Test input");
        assert_eq!(json["frames"], 192);
        assert_eq!(json["droppedFrames"], 0);
        assert_eq!(json["ended"], serde_json::json!({ "how": "stopped" }));
        assert_eq!(
            json["tracks"],
            serde_json::json!([
                { "file": "01 Vocal.wav", "kind": "input", "channels": [1], "name": "Vocal",
                  "instrument": "voice", "frames": 192 },
                { "file": "03-04 Keys.wav", "kind": "pair", "channels": [3, 4], "name": "Keys",
                  "instrument": "keys", "frames": 192 },
                { "file": "Local mix.wav", "kind": "local", "channels": [], "name": "Local mix",
                  "frames": 192 },
            ])
        );
        assert!(summary.tracks.iter().all(|track| track.frames == 192));
        assert!(
            !folder.join("take.json.tmp").exists(),
            "written through a temporary file"
        );
        assert_eq!(bench.recorder.status().state, RecordingState::Idle);
        assert_eq!(bench.recorder.status().last_take, Some(summary));
    }

    /// `take.json` is there from the start, so even a take a crash ended is described.
    #[test]
    fn take_json_is_written_when_the_take_starts() {
        let parent = scratch_directory("started");
        let mut bench = bench();
        let folder = bench.start(&request(&parent, vec![1], SampleFormat::Int24), FileOpener);

        let json = read_json(&folder.join(TAKE_FILE));
        assert_eq!(json["format"], "int24");
        assert_eq!(json["frames"], 0);
        assert_eq!(json["ended"], serde_json::Value::Null);
        assert_eq!(bench.recorder.status().tracks, vec!["CH 2".to_owned()]);
        bench.stop();
    }

    /// The input tracks keep the inputs as captured, before the channel processing, while the
    /// local mix track is the mix the engineer hears: mixed from the processed block.
    #[test]
    fn input_tracks_keep_the_raw_inputs_and_the_local_mix_the_processed_ones() {
        let tracks = plan_tracks(&four_inputs(), 4, &[0, 2], true).expect("a valid plan");
        let (feed, mut consumer, _shared) = take_feed(&tracks, 4, 4_800);
        let mut bench = bench();
        bench
            .recorder
            .lines
            .send(Some(feed))
            .expect("the sink listens");
        let raw = numbered(0, 64, 4);
        let processed: Vec<f32> = raw.iter().map(|sample| -0.5 * sample).collect();

        bench.sink.process(&Block {
            samples: &processed,
            raw: &raw,
            wet: crate::effects::Wet::NONE,
            channels: 4,
            sequence: 0,
        });

        let chunk = consumer
            .read_chunk(64 * 5)
            .expect("one block of frames: CH 1, CH 3-4 and the local mix");
        let (first, second) = chunk.as_slices();
        let recorded: Vec<f32> = first.iter().chain(second).copied().collect();
        let mut mixed = vec![0.0; 64 * 2];
        MixPlan::new(&bench.mix.snapshot(), 4).mix_into(&processed, &mut mixed);
        let expected: Vec<f32> = raw
            .chunks(4)
            .zip(mixed.chunks(2))
            .flat_map(|(frame, mix)| [frame[0], frame[2], frame[3], mix[0], mix[1]])
            .collect();
        assert_eq!(recorded, expected);
    }

    /// A ring with no room for a block drops the block whole and counts it; what fits is kept.
    #[test]
    fn a_full_ring_drops_whole_blocks_and_counts_them() {
        let tracks = plan_tracks(&four_inputs(), 4, &[0, 1], false).expect("a valid plan");
        let (feed, consumer, shared) = take_feed(&tracks, 4, 128);
        let mut bench = bench();
        bench
            .recorder
            .lines
            .send(Some(feed))
            .expect("the sink listens");
        let input = numbered(0, 64, 4);

        for _ in 0..5 {
            bench.sink.process(&block(&input, 4));
        }

        assert_eq!(shared.frames.load(Ordering::Relaxed), 128);
        assert_eq!(shared.dropped.load(Ordering::Relaxed), 192);
        assert_eq!(consumer.slots(), 2 * 128);
    }

    /// Recording or not, and the blocks that start and stop a take, the sink allocates and frees
    /// nothing on the engine thread; the take it lets go of is freed where the engine reaps it.
    #[test]
    fn the_recorder_sink_never_allocates_or_frees_on_the_engine_thread() {
        let tracks = plan_tracks(&four_inputs(), 4, &[0, 2], true).expect("a valid plan");
        let (feed, mut consumer, _shared) = take_feed(&tracks, 4, 4_800);
        let mut bench = bench();
        let input = numbered(0, 64, 4);
        let drain = |consumer: &mut Consumer<f32>| {
            let slots = consumer.slots();
            if let Ok(chunk) = consumer.read_chunk(slots) {
                chunk.commit_all();
            }
        };

        let idle = allocations::count(|| {
            for _ in 0..500 {
                bench.sink.process(&block(&input, 4));
            }
        });
        bench
            .recorder
            .lines
            .send(Some(feed))
            .expect("the sink listens");
        let start = allocations::count(|| bench.sink.process(&block(&input, 4)));
        let recording = allocations::count(|| {
            for _ in 0..500 {
                bench.sink.process(&block(&input, 4));
                drain(&mut consumer);
            }
        });
        bench.recorder.lines.send(None).expect("the sink listens");
        let stop = allocations::count(|| bench.sink.process(&block(&input, 4)));

        assert_eq!(
            [idle, start, recording, stop],
            [(0, 0), (0, 0), (0, 0), (0, 0)]
        );
        let (_, freed) = allocations::count(|| assert_eq!(bench.recorder.lines.reap(), 1));
        assert!(freed > 0, "the take is freed by the reap");
    }

    /// A file that fails its writes when `fail` is set, like a disk that filled up.
    pub(crate) struct Flaky {
        file: File,
        fail: Option<Arc<AtomicBool>>,
    }

    impl Write for Flaky {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            if self
                .fail
                .as_ref()
                .is_some_and(|fail| fail.load(Ordering::Relaxed))
            {
                return Err(io::Error::other("there is no space left on the disk"));
            }
            self.file.write(buffer)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.file.flush()
        }
    }

    impl Seek for Flaky {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.file.seek(position)
        }
    }

    impl WavTarget for Flaky {
        fn sync(&mut self) -> io::Result<()> {
            self.file.sync_data()
        }
    }

    /// Opens new files, the one whose name starts with `failing` failing when `fail` is set.
    pub(crate) struct FailingOpener {
        pub(crate) failing: &'static str,
        pub(crate) fail: Arc<AtomicBool>,
    }

    impl TrackOpener for FailingOpener {
        type Target = Flaky;

        fn open(&mut self, path: &Path, spec: WavSpec) -> Result<WavWriter<Flaky>, WavError> {
            let file = OpenOptions::new().write(true).create_new(true).open(path)?;
            let failing = path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(self.failing));
            WavWriter::new(
                Flaky {
                    file,
                    fail: failing.then(|| Arc::clone(&self.fail)),
                },
                spec,
            )
        }
    }

    /// A write that fails stops only the take: it reports the reason, the sink feeds it no more,
    /// the failed file keeps the audio of its last refresh, the other file is finished, and
    /// `take.json` says the disk ended it. The second file fails, so the first may already hold
    /// the frames the failing pass wrote: every count the take reports is what its files hold.
    #[test]
    fn a_failed_write_stops_the_take_and_keeps_what_was_refreshed() {
        let parent = scratch_directory("failed");
        let fail = Arc::new(AtomicBool::new(false));
        let mut bench = bench();
        let folder = bench.start(
            &request(&parent, vec![0, 1], SampleFormat::Float32),
            FailingOpener {
                failing: "02",
                fail: Arc::clone(&fail),
            },
        );
        let shared = Arc::clone(&bench.recorder.take.as_ref().expect("a take").shared);
        let input = numbered(0, 64, 4);
        bench.sink.process(&block(&input, 4));
        bench.sink.process(&block(&input, 4));
        assert!(until(|| shared.written.load(Ordering::Acquire) == 128));

        fail.store(true, Ordering::Relaxed);
        bench.sink.process(&block(&input, 4));
        assert!(until(|| matches!(
            bench.recorder.status().state,
            RecordingState::Failed { .. }
        )));
        let RecordingState::Failed { reason } = bench.recorder.status().state else {
            unreachable!("checked above");
        };
        assert!(reason.contains("02 CH 2.wav"), "{reason}");
        assert!(reason.contains("no space left"), "{reason}");
        bench.sink.process(&block(&input, 4));
        assert_eq!(
            shared.frames.load(Ordering::Relaxed),
            192,
            "a failed take is fed no more"
        );

        let summary = bench.stop();
        assert_eq!(summary.ended, TakeEnd::DiskError { reason });
        let failed = read_wav(&folder.join("02 CH 2.wav")).1.len() as u64;
        assert_eq!(failed, 128, "the failed file keeps its last refresh");
        // The failing refresh may come before or after the third block is written, so the first
        // file holds 128 or 192 frames: the take reports exactly what each file holds.
        let first = read_wav(&folder.join("01 Vocal.wav")).1.len() as u64;
        assert!(first == 128 || first == 192, "{first}");
        let held: Vec<u64> = summary.tracks.iter().map(|track| track.frames).collect();
        assert_eq!(held, vec![first, failed]);
        assert_eq!(summary.frames, 128, "the frames every file holds");
        let json = read_json(&folder.join(TAKE_FILE));
        assert_eq!(json["frames"], 128);
        assert_eq!(json["tracks"][0]["frames"], first);
        assert_eq!(json["tracks"][1]["frames"], failed);
        assert_eq!(json["ended"]["how"], "diskError");
        assert!(json["ended"]["reason"]
            .as_str()
            .is_some_and(|reason| reason.contains("no space left")));
    }

    /// A marker lands on the first frame of the block being recorded, in the files' frames; a
    /// blank name is numbered; `take.json` lists the markers, and a take that ended takes none.
    #[test]
    fn markers_land_on_the_frame_being_recorded() {
        let parent = scratch_directory("markers");
        let mut bench = bench();
        let folder = bench.start(&request(&parent, vec![0], SampleFormat::Int24), FileOpener);
        let input = numbered(0, 64, 4);
        for _ in 0..3 {
            bench.sink.process(&block(&input, 4));
        }

        let verse = bench.recorder.add_marker("Verse").expect("a take records");
        for _ in 0..2 {
            bench.sink.process(&block(&input, 4));
        }
        let blank = bench.recorder.add_marker("  ").expect("a take records");
        let long = bench
            .recorder
            .add_marker(&"x".repeat(80))
            .expect("a take records");

        assert_eq!(
            (verse.frame, verse.name.as_str()),
            (192, "Verse"),
            "after three blocks of 64"
        );
        assert_eq!((blank.frame, blank.name.as_str()), (320, "Marker 2"));
        assert_eq!(long.name.chars().count(), 64);
        assert_eq!(
            bench.recorder.status().markers,
            vec![verse.clone(), blank.clone(), long.clone()]
        );
        let summary = bench.stop();
        assert_eq!(summary.markers, vec![verse, blank, long]);
        let json = read_json(&folder.join(TAKE_FILE));
        assert_eq!(
            json["markers"][0],
            serde_json::json!({ "frame": 192, "name": "Verse" })
        );
        assert_eq!(
            json["markers"][1],
            serde_json::json!({ "frame": 320, "name": "Marker 2" })
        );
        let refused = bench
            .recorder
            .add_marker("Late")
            .expect_err("no take records");
        assert!(refused.contains("recording"), "{refused}");
    }

    /// A take's frames are the frames every file holds, and each track says what its file holds,
    /// even when a disk error left them apart.
    #[test]
    fn the_take_counts_the_frames_every_file_holds() {
        let tracks = plan_tracks(&four_inputs(), 4, &[0, 1, 2], false).expect("a valid plan");
        let info = TakeInfo {
            folder: PathBuf::from("take"),
            name: "Take".to_owned(),
            started: rfc3339_utc(started()),
            format: SampleFormat::Int24,
            device: "Test input".to_owned(),
            tracks: tracks.into_iter().map(|planned| planned.track).collect(),
        };

        let summary = info.summary(
            &TakeShared::default(),
            &[192, 128, 192],
            Vec::new(),
            TakeEnd::Stopped,
        );

        assert_eq!(summary.frames, 128);
        assert_eq!(
            summary
                .tracks
                .iter()
                .map(|track| track.frames)
                .collect::<Vec<_>>(),
            vec![192, 128, 192]
        );
    }

    /// Dropped blocks are in no file, so a marker after them still lands on its frame in the
    /// files: the disk thread does not look at the ring until the take stops, so most blocks
    /// overflow it, and the marker is where the files end.
    #[test]
    fn a_marker_after_dropped_blocks_lands_on_its_frame_in_the_files() {
        let parent = scratch_directory("dropped-marker");
        let mut bench = bench();
        let folder = bench.start_with(
            &request(&parent, vec![0], SampleFormat::Float32),
            FileOpener,
            TakeOptions {
                ring_frames: 128,
                timings: DiskTimings {
                    refresh: Duration::from_secs(60),
                    sync: Duration::from_secs(60),
                    poll: Duration::from_secs(60),
                },
            },
        );
        let input = numbered(0, 64, 4);
        for _ in 0..10 {
            bench.sink.process(&block(&input, 4));
        }

        let marker = bench
            .recorder
            .add_marker("After the drops")
            .expect("a take");
        let summary = bench.stop();

        assert!(summary.dropped_frames > 0, "the ring overflowed");
        assert_eq!(marker.frame + summary.dropped_frames, 10 * 64);
        assert_eq!(
            read_wav(&folder.join("01 Vocal.wav")).1.len() as u64,
            marker.frame
        );
        assert_eq!(summary.frames, marker.frame);
    }

    /// Once the disk thread has built the take's end (here the engine thread is gone, so the take
    /// closes without being asked to stop), no marker is taken: the status shows exactly the
    /// markers `take.json` lists.
    #[test]
    fn a_take_that_ended_takes_no_more_markers() {
        let parent = scratch_directory("sealed");
        let mut bench = bench();
        let folder = bench.start(&request(&parent, vec![0], SampleFormat::Int24), FileOpener);
        bench.sink.process(&block(&numbered(0, 64, 4), 4));
        let intro = bench.recorder.add_marker("Intro").expect("a take records");
        let take = bench.recorder.take.as_ref().expect("a take");
        take.shared.close();
        take.wake();
        assert!(until(|| bench
            .recorder
            .take
            .as_ref()
            .is_some_and(RunningTake::disk_finished)));

        let late = bench
            .recorder
            .add_marker("Late")
            .expect_err("the take ended");

        assert!(late.contains("no longer recording"), "{late}");
        assert_eq!(bench.recorder.status().markers, vec![intro.clone()]);
        let json = read_json(&folder.join(TAKE_FILE));
        assert_eq!(
            json["markers"],
            serde_json::json!([{ "frame": 64, "name": "Intro" }])
        );
        assert_eq!(bench.stop().markers, vec![intro]);
    }

    /// A blank name takes the first "Marker n" from the marker count on that no marker has.
    #[test]
    fn default_marker_names_never_repeat() {
        let parent = scratch_directory("marker-names");
        let mut bench = bench();
        bench.start(&request(&parent, vec![0], SampleFormat::Int24), FileOpener);

        let names: Vec<String> = ["Marker 2", "", "", "Marker 5", ""]
            .iter()
            .map(|name| bench.recorder.add_marker(name).expect("a take").name)
            .collect();

        assert_eq!(
            names,
            ["Marker 2", "Marker 3", "Marker 4", "Marker 5", "Marker 6"]
        );
        bench.stop();
    }

    /// A thread that panicked holding the markers' lock loses none of them.
    #[test]
    fn markers_survive_a_poisoned_lock() {
        let shared = Arc::new(TakeShared::default());
        shared.marker_book().list.push(TakeMarker {
            frame: 64,
            name: "Verse".to_owned(),
        });
        let holder = Arc::clone(&shared);
        let panicked = thread::spawn(move || {
            let _book = holder.marker_book();
            panic!("a thread failed holding the markers (expected by this test)");
        })
        .join();
        assert!(panicked.is_err());
        assert!(shared.markers.is_poisoned());

        assert_eq!(shared.markers().len(), 1);
        assert_eq!(shared.seal_markers()[0].name, "Verse");
    }

    /// The minutes left are the free space over the bytes per second of every armed track.
    #[test]
    fn minutes_left_are_the_free_space_over_the_armed_tracks_rate() {
        // CH 1 (mono), CH 3-4 (stereo) and the local mix (stereo): five channels.
        let tracks = plan_tracks(&four_inputs(), 4, &[0, 2], true).expect("a valid plan");
        assert_eq!(
            take_bytes_per_second(&tracks, SampleFormat::Int24),
            5 * 3 * 48_000
        );
        assert_eq!(
            take_bytes_per_second(&tracks, SampleFormat::Float32),
            5 * 4 * 48_000
        );

        assert_eq!(minutes_left(144_000 * 60 * 10, 144_000), Some(10));
        assert_eq!(minutes_left(144_000 * 60 * 10 - 1, 144_000), Some(9));
        assert_eq!(minutes_left(0, 144_000), Some(0));
        assert_eq!(minutes_left(1_000, 0), None, "nothing is written");
    }

    /// The free space is read for a folder, or for its nearest parent when it is not made yet;
    /// the status of a take reports it with the minutes it holds.
    #[test]
    fn free_space_is_read_for_the_takes_disk() {
        let parent = scratch_directory("space");
        let free = free_space(&parent);
        let unmade = free_space(&parent.join("not").join("made"));
        if cfg!(windows) {
            assert!(free.is_some_and(|bytes| bytes > 0), "{free:?}");
            assert!(unmade.is_some(), "the nearest parent is read");
        } else {
            assert_eq!((free, unmade), (None, None));
        }

        let mut bench = bench();
        bench.start(&request(&parent, vec![0], SampleFormat::Int24), FileOpener);
        let status = bench.recorder.status();
        assert_eq!(status.free_bytes.is_some(), cfg!(windows));
        assert_eq!(
            status.minutes_left,
            status
                .free_bytes
                .and_then(|bytes| minutes_left(bytes, 144_000))
        );
        bench.stop();
    }
}
