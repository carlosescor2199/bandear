//! Virtual soundcheck: a recorded take played back into the mixer in place of the live inputs.
//!
//! ```text
//!  disk thread ("soundcheck")                      engine thread (one)                    sinks
//!  read each input track with WavReader and   ─▶ rtrb ─▶ PlaybackReader: when the clock   ─▶ phones,
//!  lay its channels into frames of the device's  ring    makes a block due, take it from the    local
//!  inputs (inputs the take did not record are            ring (silence while it fills), meter   output,
//!  silent); keep the ring full                           it as capture meters its own           meters
//! ```
//!
//! A soundcheck stands in for the capture device. The engine thread reads the take's ring instead
//! of the capture's, in blocks of the capture's length and with the device's channel count, so
//! every mix, the local output and the meters behave as they do live. The capture stream keeps
//! running on its device, set aside: its callback lets its audio go without metering, counting or
//! queueing it ([crate::pipeline::CaptureStats::suspended]). Stopping the soundcheck takes the
//! engine thread back to it between two blocks, after dropping what it left in its ring; no device
//! is opened, closed or listed either way, so no ASIO driver is touched. The take's local-mix track
//! is never played: it is a mix of the inputs, not one of them.
//!
//! The clock. A take on disk has no clock of its own. When the local output plays, its device
//! callback is the clock: every callback counts the frames it played ([PlaybackClock::played]) and
//! wakes the engine thread, which makes that many frames due, so the output is fed exactly as fast
//! as it plays and never drifts. Without a local output (or when its callbacks stop for
//! [OUTPUT_GRACE], a device that failed) the engine thread's own timer is the clock: frames fall due
//! with the time that passes, counted exactly from when the timer took over, and the thread wakes
//! for each block. The timer runs on the system's wait granularity, so on a coarse system timer
//! blocks may go out a few at a time, at the same average rate.
//!
//! Real time. The engine thread never allocates, frees, locks, waits or panics for the take. Only
//! the disk thread reads files; it keeps a ring of [RING_SECONDS] seconds filled (allocated on the
//! engine's side when the soundcheck starts). Playback starts once the ring holds [PREROLL_FRAMES]
//! (or the rest of the take); until then the engine thread hands out silence, which is not an
//! underrun. When the ring runs short later, the block is padded with silence, the padded frames
//! are counted as underrun and the ring fills to the preroll again: the engine thread never waits
//! for the disk. Nothing the engine thread holds is freed there: when it lets the take go, the disk
//! thread still holds the other end of the ring (it ends only once it sees the engine let go) and
//! the engine's side holds the shared state.
//!
//! What plays. `take.json` (version 1, see [crate::recorder]) lists the tracks: an `input` track
//! is a mono file of one input, a `pair` track a stereo file of a linked pair, a `local` track the
//! local mix (skipped). The take is refused, before anything changes, when:
//!
//! - its layout is not the inputs' layout now: a track's input is past the device's inputs, an
//!   input is in two tracks, a pair is not a linked pair now, or a lone input is now one side of a
//!   linked pair (the tracks must be the ones a take of the device would have now);
//! - `take.json` or a track's file is missing or unreadable, or a file's channels differ from its
//!   track's;
//! - the take or a file is not at 48 kHz. Takes are recorded after decimation, so every take is at
//!   48 kHz whatever the device's rate (a take of a 96 kHz device plays as is); another rate is a
//!   file this engine did not make, and there is no resampler;
//! - a take records (a take never records playback), or another soundcheck plays.
//!
//! The take plays for as long as its shortest file (the frames every file holds). At its end it
//! stops (the soundcheck stays, paused at the end) or, looping, starts again at its first frame.

use std::fs::{self, File};
use std::io::{BufReader, Read, Seek};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use rtrb::{Consumer, Producer, RingBuffer};
use serde::Deserialize;

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::ChannelMeta;
use crate::pipeline::{frames_in, CaptureStats};
use crate::wav::WavReader;

/// The seconds of the take the disk thread keeps queued ahead of the engine.
pub(crate) const RING_SECONDS: usize = 2;

/// The frames the ring holds before playback starts, and again after it ran short: 250 ms.
pub(crate) const PREROLL_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 4;

/// The most frames the disk thread reads and queues at once.
const CHUNK_FRAMES: usize = 2_048;

/// How long the disk thread sleeps when the ring is full or it has nothing to do.
const DISK_POLL: Duration = Duration::from_millis(5);

/// How long the local output's callbacks may stop before the timer takes over as the clock.
pub(crate) const OUTPUT_GRACE: Duration = Duration::from_millis(250);

/// The most frames the clock lets fall due at once: an engine thread held up longer than this
/// skips the rest rather than bursting it out.
const MAX_DUE_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 4;

/// How long [crate::EngineHandle::stop_soundcheck] waits for the disk thread to end.
pub(crate) const STOP_WAIT: Duration = Duration::from_secs(2);

/// The take's description in its folder.
const TAKE_FILE: &str = "take.json";

/// What to play: the take in `take_folder`, from `start_frame` (0 for its start), looping or not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundcheckRequest {
    pub take_folder: PathBuf,
    pub start_frame: u64,
    pub looping: bool,
}

/// Whether a take plays in place of the inputs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SoundcheckState {
    /// The inputs are live: the default.
    #[default]
    Off,
    Playing,
    /// Paused by the engineer, or stopped at the end of a take that does not loop. The phones and
    /// the local output hear silence; the inputs stay set aside.
    Paused,
    /// Reading the take failed; only stopping the soundcheck is left. The inputs stay set aside
    /// (silent) until then.
    Failed {
        reason: String,
    },
}

/// What paces the take.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SoundcheckClock {
    /// The engine thread's timer: no local output plays (or its callbacks stopped).
    #[default]
    Timer,
    /// The local output's device callback.
    Output,
}

/// The soundcheck as the status reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SoundcheckStatus {
    pub state: SoundcheckState,
    /// The take's folder, while a take plays (or is paused or failed).
    pub take_folder: Option<PathBuf>,
    /// The take's name from its `take.json`.
    pub take_name: Option<String>,
    /// The next frame of the take to play, from 0.
    pub position_frames: u64,
    /// The frames the take plays for: the frames every one of its files holds.
    pub length_frames: u64,
    pub looping: bool,
    /// Frames of silence played because the disk fell behind.
    pub underrun_frames: u64,
    pub clock: SoundcheckClock,
}

/// Why a soundcheck could not start, or a change could not be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundcheckError {
    /// The engine is stopping.
    EngineStopping,
    /// A take is recording; the soundcheck must wait for it to stop.
    Recording,
    /// A soundcheck already plays a take.
    AlreadyPlaying,
    /// No soundcheck plays.
    NotPlaying,
    /// Recording was asked for while a soundcheck plays: a take never records playback.
    RecordingRefused,
    /// A device switch was asked for while a soundcheck plays.
    SwitchRefused,
    /// The take's folder has no readable `take.json`.
    TakeUnreadable { reason: String },
    /// A track's file is missing or cannot be read as its track.
    TrackUnreadable { file: String, reason: String },
    /// The take (`file` is `None`) or one of its files is not at the engine's 48 kHz.
    SampleRate {
        file: Option<String>,
        sample_rate: u32,
    },
    /// The take's inputs do not fit the inputs the device has now.
    Layout { reason: String },
    /// The take has no input track, or its files hold no audio.
    Empty,
    /// A frame past the end of the take.
    PastEnd { frame: u64, length: u64 },
    /// The soundcheck stopped on a read error; only stopping it is left.
    Failed { reason: String },
    /// The soundcheck could not start: the engine thread has ended or no disk thread could start.
    Unavailable { reason: String },
}

impl std::fmt::Display for SoundcheckError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SoundcheckError::EngineStopping => write!(formatter, "the engine is stopping"),
            SoundcheckError::Recording => write!(
                formatter,
                "a take is recording: stop the recording before starting the soundcheck"
            ),
            SoundcheckError::AlreadyPlaying => write!(
                formatter,
                "a soundcheck is already playing a take: stop it first"
            ),
            SoundcheckError::NotPlaying => write!(formatter, "no soundcheck is playing"),
            SoundcheckError::RecordingRefused => write!(
                formatter,
                "a soundcheck is playing a take: stop it before recording (a take never records playback)"
            ),
            SoundcheckError::SwitchRefused => write!(
                formatter,
                "a soundcheck is playing a take: stop it before switching the input device"
            ),
            SoundcheckError::TakeUnreadable { reason } => {
                write!(formatter, "the take cannot be read: {reason}")
            }
            SoundcheckError::TrackUnreadable { file, reason } => {
                write!(formatter, "the take's file \"{file}\" cannot be played: {reason}")
            }
            SoundcheckError::SampleRate {
                file: None,
                sample_rate,
            } => write!(
                formatter,
                "the take is at {sample_rate} Hz; only {TARGET_SAMPLE_RATE} Hz takes play (there is no resampler)"
            ),
            SoundcheckError::SampleRate {
                file: Some(file),
                sample_rate,
            } => write!(
                formatter,
                "the take's file \"{file}\" is at {sample_rate} Hz; only {TARGET_SAMPLE_RATE} Hz files play (there is no resampler)"
            ),
            SoundcheckError::Layout { reason } => write!(
                formatter,
                "the take does not fit the inputs of this device: {reason}"
            ),
            SoundcheckError::Empty => write!(formatter, "the take has no input audio to play"),
            SoundcheckError::PastEnd { frame, length } => write!(
                formatter,
                "frame {frame} is past the end of the take ({length} frames)"
            ),
            SoundcheckError::Failed { reason } => write!(
                formatter,
                "the soundcheck stopped playing: {reason}; stop it to go back to the inputs"
            ),
            SoundcheckError::Unavailable { reason } => {
                write!(formatter, "the soundcheck could not start: {reason}")
            }
        }
    }
}

impl std::error::Error for SoundcheckError {}

/// The clock a take played back follows: the frames the local output played, counted by its
/// device callback, and the engine thread to wake for them.
///
/// One for the engine, shared with every local output it opens. The output callback only adds to
/// a counter and, while a take plays, wakes the engine thread; both never wait or allocate.
pub(crate) struct PlaybackClock {
    /// Frames the local output has played, every callback included.
    output_frames: AtomicU64,
    /// A local output plays, so the take follows its callbacks rather than the timer.
    follow_output: AtomicBool,
    /// A take plays: each callback wakes the engine thread.
    active: AtomicBool,
    /// [OUTPUT_GRACE] in microseconds (tests lengthen it).
    grace_micros: AtomicU64,
    wake: OnceLock<Thread>,
}

impl PlaybackClock {
    pub(crate) fn new() -> Self {
        Self {
            output_frames: AtomicU64::new(0),
            follow_output: AtomicBool::new(false),
            active: AtomicBool::new(false),
            grace_micros: AtomicU64::new(OUTPUT_GRACE.as_micros() as u64),
            wake: OnceLock::new(),
        }
    }

    /// The engine thread each output callback wakes while a take plays.
    pub(crate) fn set_waker(&self, engine: Thread) {
        let _ = self.wake.set(engine);
    }

    /// The local output played `frames`. Called from the output callback: never waits.
    pub(crate) fn played(&self, frames: usize) {
        self.output_frames
            .fetch_add(frames as u64, Ordering::Relaxed);
        if self.active.load(Ordering::Relaxed) {
            if let Some(engine) = self.wake.get() {
                engine.unpark();
            }
        }
    }

    /// Whether a local output plays, so that its callbacks are the clock.
    pub(crate) fn follow_output(&self, follow: bool) {
        self.follow_output.store(follow, Ordering::Relaxed);
    }

    /// Whether a take plays.
    pub(crate) fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    fn grace(&self) -> Duration {
        Duration::from_micros(self.grace_micros.load(Ordering::Relaxed))
    }

    #[cfg(test)]
    pub(crate) fn set_grace(&self, grace: Duration) {
        self.grace_micros
            .store(grace.as_micros() as u64, Ordering::Relaxed);
    }
}

/// `take.json` as far as playback reads it; the other fields are left alone.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TakeJson {
    version: u32,
    name: String,
    sample_rate: u32,
    tracks: Vec<TrackJson>,
}

#[derive(Deserialize)]
struct TrackJson {
    file: String,
    kind: String,
    channels: Vec<u16>,
}

/// One input track of a take: its file and the inputs (0-based) its channels play into.
struct TrackSource<R: Read + Seek> {
    file: String,
    reader: WavReader<R>,
    inputs: Vec<usize>,
}

/// A take checked against the device's inputs and ready to play.
pub(crate) struct LoadedTake<R: Read + Seek = BufReader<File>> {
    pub(crate) folder: PathBuf,
    pub(crate) name: String,
    /// The frames every file holds.
    pub(crate) length: u64,
    /// The device's inputs: the width of every frame played.
    pub(crate) channels: usize,
    tracks: Vec<TrackSource<R>>,
}

/// The inputs (0-based) a take of the device would record together now, in input order: a linked
/// pair as one unit, every other input alone. The walk the recorder makes (see
/// [crate::recorder::plan_tracks]).
fn input_units(details: &[ChannelMeta], source_channels: usize) -> Vec<Vec<usize>> {
    let mut units = Vec::new();
    let mut channel = 0;
    while channel < source_channels {
        let linked = details.get(channel).is_some_and(|meta| meta.linked_to_next)
            && channel + 1 < source_channels;
        if linked {
            units.push(vec![channel, channel + 1]);
            channel += 2;
        } else {
            units.push(vec![channel]);
            channel += 1;
        }
    }
    units
}

/// Whether `file` names a file directly in the take's folder, and nothing else.
fn plain_file_name(file: &str) -> bool {
    let mut components = Path::new(file).components();
    matches!(
        (components.next(), components.next()),
        (Some(Component::Normal(name)), None) if name == std::ffi::OsStr::new(file)
    ) && !file.contains(['/', '\\'])
}

/// The input tracks of `tracks` with the inputs (0-based) each plays into, checked against the
/// device's `source_channels` inputs whose details are `details`.
fn route_tracks(
    tracks: &[TrackJson],
    details: &[ChannelMeta],
    source_channels: usize,
) -> Result<Vec<(String, Vec<usize>)>, SoundcheckError> {
    let units = input_units(details, source_channels);
    let mut taken = vec![false; source_channels];
    let mut routes = Vec::new();
    for track in tracks {
        let width = match track.kind.as_str() {
            "local" => continue,
            "input" => 1,
            "pair" => 2,
            other => {
                return Err(SoundcheckError::TakeUnreadable {
                    reason: format!("track \"{}\" is of an unknown kind \"{other}\"", track.file),
                })
            }
        };
        let channels = &track.channels;
        let consecutive = channels
            .windows(2)
            .all(|pair| u32::from(pair[1]) == u32::from(pair[0]) + 1);
        if channels.len() != width || !consecutive || channels.contains(&0) {
            return Err(SoundcheckError::TakeUnreadable {
                reason: format!(
                    "track \"{}\" lists the inputs {channels:?}, which a {} track cannot hold",
                    track.file, track.kind
                ),
            });
        }
        if !plain_file_name(&track.file) {
            return Err(SoundcheckError::TakeUnreadable {
                reason: format!(
                    "track file \"{}\" is not a file of the take's folder",
                    track.file
                ),
            });
        }
        let inputs: Vec<usize> = channels
            .iter()
            .map(|channel| usize::from(*channel) - 1)
            .collect();
        if let Some(past) = inputs.iter().find(|input| **input >= source_channels) {
            return Err(SoundcheckError::Layout {
                reason: format!(
                    "the take recorded CH {}, and this device has {source_channels} inputs",
                    past + 1
                ),
            });
        }
        if let Some(twice) = inputs.iter().find(|input| taken[**input]) {
            return Err(SoundcheckError::Layout {
                reason: format!("CH {} is in two tracks of the take", twice + 1),
            });
        }
        if !units.contains(&inputs) {
            let first = inputs[0] + 1;
            return Err(SoundcheckError::Layout {
                reason: if width == 2 {
                    format!(
                        "CH {first}-{} were recorded as a linked pair, which they are not now",
                        first + 1
                    )
                } else {
                    let pair = units
                        .iter()
                        .find(|unit| unit.contains(&inputs[0]))
                        .map_or(first, |unit| unit[0] + 1);
                    format!(
                        "CH {first} was recorded on its own, and it is a side of the linked pair CH {pair}-{} now",
                        pair + 1
                    )
                },
            });
        }
        for input in &inputs {
            taken[*input] = true;
        }
        routes.push((track.file.clone(), inputs));
    }
    if routes.is_empty() {
        return Err(SoundcheckError::Empty);
    }
    Ok(routes)
}

/// Checks that `reader`, the file of a track of `width` inputs, can play as that track.
fn check_track<R: Read + Seek>(
    file: &str,
    reader: &WavReader<R>,
    width: usize,
) -> Result<(), SoundcheckError> {
    let spec = reader.spec();
    if spec.sample_rate != TARGET_SAMPLE_RATE {
        return Err(SoundcheckError::SampleRate {
            file: Some(file.to_owned()),
            sample_rate: spec.sample_rate,
        });
    }
    if usize::from(spec.channels) != width {
        return Err(SoundcheckError::TrackUnreadable {
            file: file.to_owned(),
            reason: format!("it holds {} channels and its track {width}", spec.channels),
        });
    }
    Ok(())
}

/// Reads the take in `folder` and checks it against the device's `source_channels` inputs whose
/// details are `details`, opening every input track's file (never the local mix's).
pub(crate) fn load_take(
    folder: &Path,
    details: &[ChannelMeta],
    source_channels: usize,
) -> Result<LoadedTake, SoundcheckError> {
    let path = folder.join(TAKE_FILE);
    let bytes = fs::read(&path).map_err(|error| SoundcheckError::TakeUnreadable {
        reason: format!("{}: {error}", path.display()),
    })?;
    let take: TakeJson =
        serde_json::from_slice(&bytes).map_err(|error| SoundcheckError::TakeUnreadable {
            reason: format!("{TAKE_FILE} is not a take's description: {error}"),
        })?;
    if take.version != 1 {
        return Err(SoundcheckError::TakeUnreadable {
            reason: format!(
                "{TAKE_FILE} is version {}; this engine reads version 1",
                take.version
            ),
        });
    }
    if take.sample_rate != TARGET_SAMPLE_RATE {
        return Err(SoundcheckError::SampleRate {
            file: None,
            sample_rate: take.sample_rate,
        });
    }
    let mut tracks = Vec::new();
    for (file, inputs) in route_tracks(&take.tracks, details, source_channels)? {
        let reader = WavReader::open(folder.join(&file)).map_err(|error| {
            SoundcheckError::TrackUnreadable {
                file: file.clone(),
                reason: error.to_string(),
            }
        })?;
        check_track(&file, &reader, inputs.len())?;
        tracks.push(TrackSource {
            file,
            reader,
            inputs,
        });
    }
    let length = tracks
        .iter()
        .map(|track| track.reader.frames())
        .min()
        .unwrap_or(0);
    if length == 0 {
        return Err(SoundcheckError::Empty);
    }
    Ok(LoadedTake {
        folder: folder.to_owned(),
        name: take.name,
        length,
        channels: source_channels,
        tracks,
    })
}

/// What the disk thread, the engine thread and the engine's side share about one take.
pub(crate) struct PlaybackShared {
    looping: AtomicBool,
    paused: AtomicBool,
    /// The engine thread reached the end of a take that does not loop.
    ended: AtomicBool,
    /// The next frame the engine thread plays.
    position: AtomicU64,
    /// Frames of silence the engine thread padded because the ring ran short.
    underruns: AtomicU64,
    /// The disk thread has queued the take up to its end and stops there (looping is off).
    disk_end: AtomicBool,
    /// Frames the disk thread has queued.
    queued: AtomicU64,
    /// The take follows the local output's callbacks (else the timer), as the engine thread last
    /// found.
    output_clock: AtomicBool,
    /// The engine's side gave up on the take: the disk thread ends.
    closed: AtomicBool,
    failed: AtomicBool,
    /// Why reading failed, written once by the disk thread; never read on the engine thread.
    reason: Mutex<Option<String>>,
}

impl PlaybackShared {
    fn new(looping: bool, position: u64, output_clock: bool) -> Self {
        Self {
            looping: AtomicBool::new(looping),
            paused: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            position: AtomicU64::new(position),
            underruns: AtomicU64::new(0),
            disk_end: AtomicBool::new(false),
            queued: AtomicU64::new(0),
            output_clock: AtomicBool::new(output_clock),
            closed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
            reason: Mutex::new(None),
        }
    }

    fn fail(&self, reason: String) {
        if let Ok(mut stored) = self.reason.lock() {
            stored.get_or_insert(reason);
        }
        self.failed.store(true, Ordering::Release);
    }

    fn failure(&self) -> Option<String> {
        if !self.failed.load(Ordering::Acquire) {
            return None;
        }
        Some(
            self.reason
                .lock()
                .ok()
                .and_then(|reason| reason.clone())
                .unwrap_or_else(|| "reading the take failed".to_owned()),
        )
    }
}

/// What one step of the disk thread did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiskStep {
    /// Queued this many frames; there may be more to do at once.
    Queued(usize),
    /// Nothing to do until the ring has room or something changes.
    Idle,
    /// The engine let go of the take, or the engine's side gave up on it.
    Exit,
}

/// The disk thread's half of a take: its files, and the ring it keeps filled.
pub(crate) struct DiskPlayer<R: Read + Seek = BufReader<File>> {
    tracks: Vec<TrackSource<R>>,
    producer: Producer<f32>,
    shared: Arc<PlaybackShared>,
    channels: usize,
    length: u64,
    /// The next frame to read from the files.
    position: u64,
    /// Room for one chunk of device frames.
    chunk: Vec<f32>,
    /// Room for one chunk of a track's own frames (at most a stereo pair).
    scratch: Vec<f32>,
}

/// `sample` as the mix takes it: within [-1, 1], NaN as silence (float files are read as stored).
fn clean(sample: f32) -> f32 {
    if sample.is_nan() {
        0.0
    } else {
        sample.clamp(-1.0, 1.0)
    }
}

impl<R: Read + Seek> DiskPlayer<R> {
    /// Does the next piece of work: queues the next chunk of the take when the ring has room for
    /// it, goes back to the start of a looping take at its end, and stops at the end of one that
    /// does not loop.
    pub(crate) fn step(&mut self) -> DiskStep {
        if self.producer.is_abandoned() || self.shared.closed.load(Ordering::Acquire) {
            return DiskStep::Exit;
        }
        if self.shared.failed.load(Ordering::Acquire) {
            return DiskStep::Idle;
        }
        if self.position >= self.length {
            if !self.shared.looping.load(Ordering::Acquire) {
                self.shared.disk_end.store(true, Ordering::Release);
                return DiskStep::Idle;
            }
            if let Err(error) = self.seek_all(0) {
                self.shared.fail(error);
                return DiskStep::Idle;
            }
            self.shared.disk_end.store(false, Ordering::Release);
        }
        let frames = (self.length - self.position).min(CHUNK_FRAMES as u64) as usize;
        if self.producer.slots() < frames * self.channels {
            return DiskStep::Idle;
        }
        if let Err(error) = self.read(frames) {
            self.shared.fail(error);
            return DiskStep::Idle;
        }
        if self
            .producer
            .push_entire_slice(&self.chunk[..frames * self.channels])
            .is_err()
        {
            return DiskStep::Idle;
        }
        self.position += frames as u64;
        if self.position >= self.length && !self.shared.looping.load(Ordering::Acquire) {
            // Everything is queued: the engine need not wait for a preroll the rest cannot fill.
            self.shared.disk_end.store(true, Ordering::Release);
        }
        self.shared
            .queued
            .fetch_add(frames as u64, Ordering::Release);
        DiskStep::Queued(frames)
    }

    /// Reads the next `frames` of every track into the chunk, laid into the device's inputs.
    fn read(&mut self, frames: usize) -> Result<(), String> {
        let Self {
            tracks,
            chunk,
            scratch,
            channels,
            ..
        } = self;
        let channels = *channels;
        let chunk = &mut chunk[..frames * channels];
        chunk.fill(0.0);
        for track in tracks.iter_mut() {
            let width = track.inputs.len();
            let scratch = &mut scratch[..frames * width];
            let read = track
                .reader
                .read_frames(scratch)
                .map_err(|error| format!("reading \"{}\" failed: {error}", track.file))?;
            for (samples, frame) in scratch[..read * width]
                .chunks_exact(width)
                .zip(chunk.chunks_exact_mut(channels))
            {
                for (input, sample) in track.inputs.iter().zip(samples) {
                    if let Some(slot) = frame.get_mut(*input) {
                        *slot = clean(*sample);
                    }
                }
            }
        }
        Ok(())
    }

    /// Moves every file to `frame`.
    fn seek_all(&mut self, frame: u64) -> Result<(), String> {
        for track in &mut self.tracks {
            track
                .reader
                .seek(frame)
                .map_err(|error| format!("seeking in \"{}\" failed: {error}", track.file))?;
        }
        self.position = frame;
        Ok(())
    }

    /// The disk thread: steps until the take is let go of, sleeping while there is nothing to
    /// do. It holds its end of the ring until the engine thread has let go of the other, so the
    /// ring is freed here, never on the engine thread.
    pub(crate) fn run(mut self) {
        loop {
            match self.step() {
                DiskStep::Exit => return,
                DiskStep::Queued(_) => {}
                DiskStep::Idle => thread::park_timeout(DISK_POLL),
            }
        }
    }
}

/// The engine thread's half of a take: the ring it plays from and the clock that paces it.
pub(crate) struct PlaybackReader {
    consumer: Consumer<f32>,
    shared: Arc<PlaybackShared>,
    clock: Arc<PlaybackClock>,
    /// The capture's counters, whose `suspended` flag sets the capture aside.
    capture: Arc<CaptureStats>,
    channels: usize,
    length: u64,
    preroll: usize,
    position: u64,
    /// The ring has held the preroll since it last ran short.
    primed: bool,
    /// Frames due that no block has taken yet.
    credit: usize,
    /// The output clock's count when last read.
    last_output: u64,
    /// When the output clock last moved (or the take started).
    last_tick: Instant,
    /// When the timer took over as the clock, and the frames it has made due since.
    timer: Option<(Instant, u64)>,
}

impl PlaybackReader {
    /// The width of every frame played: the device's inputs.
    pub(crate) fn channels(&self) -> usize {
        self.channels
    }

    /// The capture's counters, whose `suspended` flag sets the capture aside while the take plays.
    pub(crate) fn capture(&self) -> &CaptureStats {
        &self.capture
    }

    /// The whole blocks of `block_frames` the clock has made due by `now`.
    ///
    /// The local output's callbacks are the clock while one plays and has played within the
    /// grace; otherwise the timer, which counts the frames in the time since it took over.
    pub(crate) fn due(&mut self, now: Instant, block_frames: usize) -> usize {
        let block_frames = block_frames.max(1);
        let ticks = self.clock.output_frames.load(Ordering::Relaxed);
        let played = ticks.wrapping_sub(self.last_output);
        self.last_output = ticks;
        let follow = self.clock.follow_output.load(Ordering::Relaxed);
        if follow && played > 0 {
            self.credit = self.credit.saturating_add(played as usize);
            self.last_tick = now;
            self.timer = None;
            self.shared.output_clock.store(true, Ordering::Relaxed);
        } else if !follow || now.saturating_duration_since(self.last_tick) >= self.clock.grace() {
            let (origin, counted) = self.timer.get_or_insert((now, 0));
            let total = frames_in(now.saturating_duration_since(*origin)) as u64;
            self.credit = self
                .credit
                .saturating_add(total.saturating_sub(*counted) as usize);
            *counted = total;
            self.shared.output_clock.store(false, Ordering::Relaxed);
        }
        self.credit = self.credit.min(MAX_DUE_FRAMES.max(block_frames));
        let blocks = self.credit / block_frames;
        self.credit -= blocks * block_frames;
        blocks
    }

    /// How long until the next block of `block_frames` is due on the timer.
    pub(crate) fn wait(&self, block_frames: usize) -> Duration {
        let missing = block_frames.saturating_sub(self.credit).max(1) as u64;
        Duration::from_micros(missing * 1_000_000 / u64::from(TARGET_SAMPLE_RATE))
    }

    /// Fills `block` (whole frames of the device's inputs) with the next frames of the take, or
    /// silence: while the ring fills to the preroll, while paused, at the end of a take that does
    /// not loop, after a read error, and for what the ring lacks (counted as underrun).
    pub(crate) fn fill(&mut self, block: &mut [f32]) {
        let channels = self.channels.max(1);
        let frames = block.len() / channels;
        let silent = self.shared.failed.load(Ordering::Acquire)
            || self.shared.paused.load(Ordering::Relaxed)
            || self.shared.ended.load(Ordering::Relaxed);
        if silent || !self.ready() {
            block.fill(0.0);
            return;
        }
        let shared = &*self.shared;
        let mut written = 0;
        while written < frames {
            if self.position >= self.length {
                if !shared.looping.load(Ordering::Acquire) {
                    shared.ended.store(true, Ordering::Relaxed);
                    break;
                }
                self.position = 0;
            }
            let wanted = (frames - written).min((self.length - self.position) as usize);
            let taken = wanted.min(self.consumer.slots() / channels);
            if taken == 0 {
                // The disk fell behind: pad the rest, and wait for the preroll again.
                shared
                    .underruns
                    .fetch_add((frames - written) as u64, Ordering::Relaxed);
                self.primed = false;
                break;
            }
            let Ok(chunk) = self.consumer.read_chunk(taken * channels) else {
                break;
            };
            let (head, tail) = chunk.as_slices();
            let out = &mut block[written * channels..(written + taken) * channels];
            for (slot, sample) in out.iter_mut().zip(head.iter().chain(tail.iter())) {
                *slot = *sample;
            }
            chunk.commit_all();
            written += taken;
            self.position += taken as u64;
        }
        block[written * channels..].fill(0.0);
        shared.position.store(self.position, Ordering::Relaxed);
    }

    /// Whether the ring holds enough to play: the preroll, or everything the disk will queue.
    fn ready(&mut self) -> bool {
        if !self.primed {
            let queued = self.consumer.slots() / self.channels.max(1);
            self.primed = queued >= self.preroll || self.shared.disk_end.load(Ordering::Acquire);
        }
        self.primed
    }
}

/// How a take's ring is set up.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkOptions {
    /// Frames the ring holds.
    pub(crate) ring_frames: usize,
    /// Frames the ring holds before playback starts.
    pub(crate) preroll: usize,
}

impl Default for LinkOptions {
    fn default() -> Self {
        Self {
            ring_frames: RING_SECONDS * TARGET_SAMPLE_RATE as usize,
            preroll: PREROLL_FRAMES,
        }
    }
}

/// Both halves of `take`'s playback from `start_frame`, and their shared state. `capture` is the
/// capture's counters, set aside while the take plays.
pub(crate) fn playback_link<R: Read + Seek>(
    mut take: LoadedTake<R>,
    start_frame: u64,
    looping: bool,
    clock: Arc<PlaybackClock>,
    capture: Arc<CaptureStats>,
    options: LinkOptions,
) -> Result<(DiskPlayer<R>, PlaybackReader, Arc<PlaybackShared>), SoundcheckError> {
    if start_frame > take.length {
        return Err(SoundcheckError::PastEnd {
            frame: start_frame,
            length: take.length,
        });
    }
    for track in &mut take.tracks {
        track
            .reader
            .seek(start_frame)
            .map_err(|error| SoundcheckError::TrackUnreadable {
                file: track.file.clone(),
                reason: error.to_string(),
            })?;
    }
    let channels = take.channels.max(1);
    let ring_frames = options.ring_frames.max(CHUNK_FRAMES);
    let (producer, consumer) = RingBuffer::new(ring_frames * channels);
    let shared = Arc::new(PlaybackShared::new(
        looping,
        start_frame,
        clock.follow_output.load(Ordering::Relaxed),
    ));
    let width = take
        .tracks
        .iter()
        .map(|track| track.inputs.len())
        .max()
        .unwrap_or(1);
    let disk = DiskPlayer {
        tracks: take.tracks,
        producer,
        shared: Arc::clone(&shared),
        channels,
        length: take.length,
        position: start_frame,
        chunk: vec![0.0; CHUNK_FRAMES * channels],
        scratch: vec![0.0; CHUNK_FRAMES * width],
    };
    let reader = PlaybackReader {
        consumer,
        shared: Arc::clone(&shared),
        last_output: clock.output_frames.load(Ordering::Relaxed),
        clock,
        capture,
        channels,
        length: take.length,
        preroll: options.preroll.min(ring_frames),
        position: start_frame,
        primed: false,
        credit: 0,
        last_tick: Instant::now(),
        timer: None,
    };
    Ok((disk, reader, shared))
}

/// A take playing: its shared state and its disk thread, on the engine's side.
pub(crate) struct ActiveSoundcheck {
    shared: Arc<PlaybackShared>,
    disk: Option<JoinHandle<()>>,
    folder: PathBuf,
    name: String,
    length: u64,
}

/// Loads the take of `request` for the device's `source_channels` inputs whose details are
/// `details`, and starts its disk thread: the reader for the engine thread, and the take on the
/// engine's side.
pub(crate) fn prepare(
    request: &SoundcheckRequest,
    details: &[ChannelMeta],
    source_channels: usize,
    clock: Arc<PlaybackClock>,
    capture: Arc<CaptureStats>,
) -> Result<(PlaybackReader, ActiveSoundcheck), SoundcheckError> {
    let take = load_take(&request.take_folder, details, source_channels)?;
    let folder = take.folder.clone();
    let name = take.name.clone();
    let length = take.length;
    let (disk, reader, shared) = playback_link(
        take,
        request.start_frame,
        request.looping,
        clock,
        capture,
        LinkOptions::default(),
    )?;
    let disk = thread::Builder::new()
        .name("soundcheck".to_owned())
        .spawn(move || disk.run())
        .map_err(|error| SoundcheckError::Unavailable {
            reason: format!("no disk thread: {error}"),
        })?;
    Ok((
        reader,
        ActiveSoundcheck {
            shared,
            disk: Some(disk),
            folder,
            name,
            length,
        },
    ))
}

impl ActiveSoundcheck {
    /// Waits up to `wait` for the disk thread to end once the engine thread has let go of the
    /// take, then ends it in any case and joins it.
    pub(crate) fn end(mut self, wait: Duration) {
        let Some(disk) = self.disk.take() else {
            return;
        };
        let deadline = Instant::now() + wait;
        while !disk.is_finished() && Instant::now() < deadline {
            disk.thread().unpark();
            thread::sleep(Duration::from_millis(1));
        }
        self.shared.closed.store(true, Ordering::Release);
        disk.thread().unpark();
        let _ = disk.join();
    }

    fn status(&self) -> SoundcheckStatus {
        let shared = &self.shared;
        let state = match shared.failure() {
            Some(reason) => SoundcheckState::Failed { reason },
            None if shared.paused.load(Ordering::Relaxed)
                || shared.ended.load(Ordering::Relaxed) =>
            {
                SoundcheckState::Paused
            }
            None => SoundcheckState::Playing,
        };
        SoundcheckStatus {
            state,
            take_folder: Some(self.folder.clone()),
            take_name: Some(self.name.clone()),
            position_frames: shared.position.load(Ordering::Relaxed),
            length_frames: self.length,
            looping: shared.looping.load(Ordering::Relaxed),
            underrun_frames: shared.underruns.load(Ordering::Relaxed),
            clock: if shared.output_clock.load(Ordering::Relaxed) {
                SoundcheckClock::Output
            } else {
                SoundcheckClock::Timer
            },
        }
    }
}

/// The engine's side of the soundcheck: the take playing, if any.
#[derive(Default)]
pub(crate) struct Soundcheck {
    active: Option<ActiveSoundcheck>,
}

impl Soundcheck {
    pub(crate) fn is_active(&self) -> bool {
        self.active.is_some()
    }

    /// An error while a take plays: nothing may be recorded.
    pub(crate) fn refuse_recording(&self) -> Result<(), SoundcheckError> {
        match self.active {
            Some(_) => Err(SoundcheckError::RecordingRefused),
            None => Ok(()),
        }
    }

    /// An error while a take plays: the input device must not change under it.
    pub(crate) fn refuse_source_change(&self) -> Result<(), SoundcheckError> {
        match self.active {
            Some(_) => Err(SoundcheckError::SwitchRefused),
            None => Ok(()),
        }
    }

    /// Keeps `active` as the take that plays.
    pub(crate) fn begin(&mut self, active: ActiveSoundcheck) {
        self.active = Some(active);
    }

    /// The take that plays, let go of.
    pub(crate) fn take(&mut self) -> Option<ActiveSoundcheck> {
        self.active.take()
    }

    pub(crate) fn status(&self) -> SoundcheckStatus {
        self.active
            .as_ref()
            .map(ActiveSoundcheck::status)
            .unwrap_or_default()
    }

    /// Ends the take after the engine thread has ended: nothing plays it any more.
    pub(crate) fn engine_stopped(&mut self) {
        if let Some(active) = self.active.take() {
            active.end(Duration::ZERO);
        }
    }

    /// Frames the disk thread has queued for the take that plays.
    #[cfg(test)]
    pub(crate) fn queued(&self) -> u64 {
        self.active
            .as_ref()
            .map_or(0, |active| active.shared.queued.load(Ordering::Acquire))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::ChannelKind;
    use crate::pipeline::tests::{allocations, links, source, Recorder};
    use crate::pipeline::Pipeline;
    use crate::recorder::tests::scratch_directory;
    use crate::wav::{SampleFormat, WavSpec, WavWriter};
    use serde_json::{json, Value};

    /// The frames of the tests' take.
    const LENGTH: u64 = 1_000;

    /// Channel `channel` of frame `frame` of the test file `file`: every sample says where it is.
    fn sample(file: usize, channel: usize, frame: u64) -> f32 {
        (file * 2 + channel + 1) as f32 / 10.0 + (frame % 1_000) as f32 / 100_000.0
    }

    /// A 32-bit float file of `frames` frames numbered by [sample] as test file `file`.
    fn write_wav(path: &Path, channels: u16, sample_rate: u32, frames: u64, file: usize) {
        let mut writer = WavWriter::create(
            path,
            WavSpec {
                channels,
                sample_rate,
                format: SampleFormat::Float32,
            },
        )
        .expect("the test file is made");
        let samples: Vec<f32> = (0..frames)
            .flat_map(|frame| {
                (0..usize::from(channels)).map(move |channel| sample(file, channel, frame))
            })
            .collect();
        writer
            .write_frames(&samples)
            .expect("the test file is written");
        writer.finish().expect("the test file is finished");
    }

    /// A take's `take.json` with `tracks`, as the recorder writes it.
    fn take_json(tracks: Value) -> Value {
        json!({
            "version": 1,
            "name": "Soundcheck",
            "started": "2026-10-16T19:04:05Z",
            "sampleRate": 48_000,
            "format": "float32",
            "device": "Test input (4)",
            "tracks": tracks,
            "frames": LENGTH,
            "markers": [],
            "droppedFrames": 0,
            "ended": { "how": "stopped" }
        })
    }

    fn write_json(folder: &Path, value: &Value) {
        fs::write(
            folder.join(TAKE_FILE),
            serde_json::to_vec_pretty(value).expect("JSON"),
        )
        .expect("take.json is written");
    }

    /// CH 1 "Vocal" alone, CH 3-4 "Keys" as a linked pair, and the local mix.
    fn standard_tracks() -> Value {
        json!([
            { "file": "01 Vocal.wav", "kind": "input", "channels": [1], "name": "Vocal",
              "instrument": "voice", "frames": LENGTH },
            { "file": "03-04 Keys.wav", "kind": "pair", "channels": [3, 4], "name": "Keys",
              "instrument": "keys", "frames": LENGTH },
            { "file": "Local mix.wav", "kind": "local", "channels": [], "name": "Local mix",
              "frames": LENGTH }
        ])
    }

    /// A take of [LENGTH] frames recorded on [four_inputs]: test files 0 (CH 1) and 1 (CH 3-4),
    /// and the local mix as test file 3, which must never be heard.
    fn take_folder(test: &str) -> PathBuf {
        let folder = scratch_directory(&format!("soundcheck-{test}"));
        write_wav(&folder.join("01 Vocal.wav"), 1, 48_000, LENGTH, 0);
        write_wav(&folder.join("03-04 Keys.wav"), 2, 48_000, LENGTH, 1);
        write_wav(&folder.join("Local mix.wav"), 2, 48_000, LENGTH, 3);
        write_json(&folder, &take_json(standard_tracks()));
        folder
    }

    /// A take of one input, CH 1, holding `value` for `frames` frames, for the engine's tests.
    pub(crate) fn constant_take(test: &str, frames: u64, value: f32) -> PathBuf {
        let folder = scratch_directory(&format!("soundcheck-{test}"));
        let mut writer = WavWriter::create(
            folder.join("01 CH 1.wav"),
            WavSpec {
                channels: 1,
                sample_rate: 48_000,
                format: SampleFormat::Float32,
            },
        )
        .expect("the test file is made");
        writer
            .write_frames(&vec![value; frames as usize])
            .expect("the test file is written");
        writer.finish().expect("the test file is finished");
        write_json(
            &folder,
            &take_json(json!([
                { "file": "01 CH 1.wav", "kind": "input", "channels": [1], "name": "CH 1",
                  "frames": frames }
            ])),
        );
        folder
    }

    /// A take of CH `channel` alone (4800 frames at 0.5), in a folder of its own.
    pub(crate) fn take_of_input(test: &str, channel: u16) -> PathBuf {
        let folder = constant_take(test, 4_800, 0.5);
        write_json(
            &folder,
            &take_json(json!([
                { "file": "01 CH 1.wav", "kind": "input", "channels": [channel], "name": "CH",
                  "frames": 4_800 }
            ])),
        );
        folder
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

    /// The refusal of a take that should not load.
    fn refusal(result: Result<LoadedTake, SoundcheckError>) -> SoundcheckError {
        match result {
            Ok(_) => panic!("the take was accepted"),
            Err(error) => error,
        }
    }

    /// A take of [take_folder] linked from `start`, with its clock.
    struct Playing {
        disk: DiskPlayer,
        reader: PlaybackReader,
        shared: Arc<PlaybackShared>,
        clock: Arc<PlaybackClock>,
    }

    fn playing(folder: &Path, start: u64, looping: bool, options: LinkOptions) -> Playing {
        let take = load_take(folder, &four_inputs(), 4).expect("the take loads");
        let clock = Arc::new(PlaybackClock::new());
        let (disk, reader, shared) = playback_link(
            take,
            start,
            looping,
            Arc::clone(&clock),
            Arc::new(CaptureStats::default()),
            options,
        )
        .expect("the take links");
        Playing {
            disk,
            reader,
            shared,
            clock,
        }
    }

    /// A ring of 4096 frames that plays from 128.
    fn small() -> LinkOptions {
        LinkOptions {
            ring_frames: 4_096,
            preroll: 128,
        }
    }

    /// Steps the disk until it has nothing more to do.
    fn fill_ring(disk: &mut DiskPlayer) {
        while let DiskStep::Queued(_) = disk.step() {}
    }

    /// The next block of `frames` frames, as the engine thread plays it.
    fn next(reader: &mut PlaybackReader, frames: usize) -> Vec<[f32; 4]> {
        let mut block = vec![9.0; frames * 4];
        reader.fill(&mut block);
        block
            .chunks_exact(4)
            .map(|frame| [frame[0], frame[1], frame[2], frame[3]])
            .collect()
    }

    /// Frame `frame` of the take as the four inputs hear it: CH 2 was not recorded.
    fn heard(frame: u64) -> [f32; 4] {
        [
            sample(0, 0, frame),
            0.0,
            sample(1, 0, frame),
            sample(1, 1, frame),
        ]
    }

    fn heard_range(frames: std::ops::Range<u64>) -> Vec<[f32; 4]> {
        frames.map(heard).collect()
    }

    #[test]
    fn a_take_plays_into_the_inputs_it_recorded_and_never_its_local_mix() {
        let folder = take_folder("routing");
        let mut run = playing(&folder, 0, false, small());
        assert_eq!(run.reader.channels(), 4);
        fill_ring(&mut run.disk);

        assert_eq!(next(&mut run.reader, 64), heard_range(0..64));
        assert_eq!(next(&mut run.reader, 64), heard_range(64..128));
        assert_eq!(run.shared.position.load(Ordering::Relaxed), 128);

        // The local mix's file is never opened: a take without it plays the same.
        fs::remove_file(folder.join("Local mix.wav")).unwrap();
        let mut run = playing(&folder, 0, false, small());
        fill_ring(&mut run.disk);
        assert_eq!(next(&mut run.reader, 64), heard_range(0..64));
    }

    #[test]
    fn a_take_that_does_not_fit_the_inputs_is_refused() {
        let folder = take_folder("layout");

        let SoundcheckError::Layout { reason } =
            refusal(load_take(&folder, &four_inputs()[..2], 2))
        else {
            panic!("an input past the device is a layout refusal");
        };
        assert!(
            reason.contains("CH 3") && reason.contains("2 inputs"),
            "{reason}"
        );

        let mut unlinked = four_inputs();
        unlinked[2].linked_to_next = false;
        let SoundcheckError::Layout { reason } = refusal(load_take(&folder, &unlinked, 4)) else {
            panic!("an unlinked pair is a layout refusal");
        };
        assert!(
            reason.contains("CH 3-4") && reason.contains("linked pair"),
            "{reason}"
        );

        let mut paired = four_inputs();
        paired[0].linked_to_next = true;
        let SoundcheckError::Layout { reason } = refusal(load_take(&folder, &paired, 4)) else {
            panic!("a lone input now in a pair is a layout refusal");
        };
        assert!(reason.contains("CH 1 was recorded on its own"), "{reason}");
        assert!(reason.contains("CH 1-2"), "{reason}");

        write_json(
            &folder,
            &take_json(json!([
                { "file": "01 Vocal.wav", "kind": "input", "channels": [1] },
                { "file": "01 Vocal.wav", "kind": "input", "channels": [1] }
            ])),
        );
        let SoundcheckError::Layout { reason } = refusal(load_take(&folder, &four_inputs(), 4))
        else {
            panic!("an input in two tracks is a layout refusal");
        };
        assert!(reason.contains("two tracks"), "{reason}");
    }

    #[test]
    fn a_take_at_another_rate_is_refused() {
        let folder = take_folder("rate");
        let mut json = take_json(standard_tracks());
        json["sampleRate"] = json!(96_000);
        write_json(&folder, &json);
        assert_eq!(
            refusal(load_take(&folder, &four_inputs(), 4)),
            SoundcheckError::SampleRate {
                file: None,
                sample_rate: 96_000
            }
        );

        write_json(&folder, &take_json(standard_tracks()));
        fs::remove_file(folder.join("01 Vocal.wav")).unwrap();
        write_wav(&folder.join("01 Vocal.wav"), 1, 44_100, LENGTH, 0);
        assert_eq!(
            refusal(load_take(&folder, &four_inputs(), 4)),
            SoundcheckError::SampleRate {
                file: Some("01 Vocal.wav".to_owned()),
                sample_rate: 44_100
            }
        );
    }

    #[test]
    fn a_missing_or_unreadable_file_is_refused() {
        let folder = take_folder("missing");
        let keys = folder.join("03-04 Keys.wav");

        fs::remove_file(&keys).unwrap();
        let missing = refusal(load_take(&folder, &four_inputs(), 4));
        assert!(
            matches!(&missing, SoundcheckError::TrackUnreadable { file, .. } if file == "03-04 Keys.wav"),
            "{missing:?}"
        );

        fs::write(&keys, b"not a WAV file at all").unwrap();
        let garbage = refusal(load_take(&folder, &four_inputs(), 4));
        assert!(
            matches!(&garbage, SoundcheckError::TrackUnreadable { file, .. } if file == "03-04 Keys.wav"),
            "{garbage:?}"
        );

        fs::remove_file(&keys).unwrap();
        write_wav(&keys, 1, 48_000, LENGTH, 1);
        let SoundcheckError::TrackUnreadable { reason, .. } =
            refusal(load_take(&folder, &four_inputs(), 4))
        else {
            panic!("a mono file for a pair is refused");
        };
        assert!(reason.contains("1 channels"), "{reason}");

        let mut json = take_json(standard_tracks());
        json["tracks"][0]["file"] = json!("..\\01 Vocal.wav");
        write_json(&folder, &json);
        assert!(matches!(
            refusal(load_take(&folder, &four_inputs(), 4)),
            SoundcheckError::TakeUnreadable { .. }
        ));

        let mut json = take_json(standard_tracks());
        json["version"] = json!(2);
        write_json(&folder, &json);
        let SoundcheckError::TakeUnreadable { reason } =
            refusal(load_take(&folder, &four_inputs(), 4))
        else {
            panic!("another version is refused");
        };
        assert!(reason.contains("version 2"), "{reason}");

        fs::remove_file(folder.join(TAKE_FILE)).unwrap();
        assert!(matches!(
            refusal(load_take(&folder, &four_inputs(), 4)),
            SoundcheckError::TakeUnreadable { .. }
        ));
    }

    #[test]
    fn a_take_with_nothing_to_play_or_a_start_past_its_end_is_refused() {
        let folder = take_folder("empty");
        write_json(
            &folder,
            &take_json(json!([
                { "file": "Local mix.wav", "kind": "local", "channels": [] }
            ])),
        );
        assert_eq!(
            refusal(load_take(&folder, &four_inputs(), 4)),
            SoundcheckError::Empty
        );

        write_json(&folder, &take_json(standard_tracks()));
        let take = load_take(&folder, &four_inputs(), 4).expect("the take loads");
        let past = playback_link(
            take,
            LENGTH + 1,
            false,
            Arc::new(PlaybackClock::new()),
            Arc::new(CaptureStats::default()),
            small(),
        );
        assert!(matches!(
            past,
            Err(SoundcheckError::PastEnd {
                frame: 1_001,
                length: LENGTH
            })
        ));
    }

    /// The take stops at its last frame: the block that holds it is padded with silence, and so
    /// is every block after it; nothing is counted as underrun.
    #[test]
    fn playback_stops_at_the_end_of_the_take() {
        let folder = take_folder("end");
        let mut run = playing(&folder, 0, false, small());
        fill_ring(&mut run.disk);

        for block in 0..15 {
            assert_eq!(
                next(&mut run.reader, 64),
                heard_range(64 * block..64 * (block + 1))
            );
        }
        let last = next(&mut run.reader, 64);

        assert_eq!(&last[..40], heard_range(960..1_000).as_slice());
        assert!(last[40..].iter().all(|frame| *frame == [0.0; 4]));
        assert!(run.shared.ended.load(Ordering::Relaxed));
        assert_eq!(run.shared.position.load(Ordering::Relaxed), LENGTH);
        assert!(next(&mut run.reader, 64)
            .iter()
            .all(|frame| *frame == [0.0; 4]));
        assert_eq!(run.shared.underruns.load(Ordering::Relaxed), 0);
        assert_eq!(run.disk.step(), DiskStep::Idle);
    }

    /// A looping take started near its end goes on from its first frame within the block.
    #[test]
    fn a_looping_take_starts_again_at_its_first_frame() {
        let folder = take_folder("loop");
        let mut run = playing(&folder, 900, true, small());
        fill_ring(&mut run.disk);

        assert_eq!(next(&mut run.reader, 64), heard_range(900..964));
        let wrapped = next(&mut run.reader, 64);

        assert_eq!(&wrapped[..36], heard_range(964..1_000).as_slice());
        assert_eq!(&wrapped[36..], heard_range(0..28).as_slice());
        assert_eq!(run.shared.position.load(Ordering::Relaxed), 28);
        assert!(!run.shared.ended.load(Ordering::Relaxed));
    }

    /// Before the ring holds the preroll the engine plays silence, which is no underrun; once
    /// playing, what the ring lacks is padded and counted, and the engine never waits.
    #[test]
    fn a_disk_that_falls_behind_costs_counted_silence_and_never_a_wait() {
        let folder = take_folder("underrun");
        let mut run = playing(&folder, 0, true, small());

        assert!(next(&mut run.reader, 64)
            .iter()
            .all(|frame| *frame == [0.0; 4]));
        assert_eq!(run.shared.underruns.load(Ordering::Relaxed), 0);
        assert_eq!(run.shared.position.load(Ordering::Relaxed), 0);

        // One chunk: the whole take, once.
        assert_eq!(run.disk.step(), DiskStep::Queued(1_000));
        for block in 0..15 {
            assert_eq!(
                next(&mut run.reader, 64),
                heard_range(64 * block..64 * (block + 1))
            );
        }
        let short = next(&mut run.reader, 64);
        assert_eq!(&short[..40], heard_range(960..1_000).as_slice());
        assert!(short[40..].iter().all(|frame| *frame == [0.0; 4]));
        assert_eq!(run.shared.underruns.load(Ordering::Relaxed), 24);

        // The disk catches up (from the take's start, as it loops); the ring fills to the
        // preroll again before the take goes on.
        fill_ring(&mut run.disk);
        assert_eq!(next(&mut run.reader, 64), heard_range(0..64));
        assert_eq!(run.shared.underruns.load(Ordering::Relaxed), 24);
    }

    #[test]
    fn the_timer_makes_frames_due_with_the_time_that_passes() {
        let folder = take_folder("timer");
        let mut run = playing(&folder, 0, false, small());
        let start = Instant::now();

        assert_eq!(run.reader.due(start, 64), 0);
        // 10 ms are 480 frames: seven blocks, 32 frames carried.
        assert_eq!(run.reader.due(start + Duration::from_millis(10), 64), 7);
        assert_eq!(run.reader.due(start + Duration::from_millis(20), 64), 8);
        assert!(!run.shared.output_clock.load(Ordering::Relaxed));
        // The local output's callbacks count for nothing while it does not play.
        run.clock.played(4_096);
        assert_eq!(run.reader.due(start + Duration::from_millis(20), 64), 0);
    }

    #[test]
    fn the_local_output_clock_makes_due_the_frames_it_played() {
        let folder = take_folder("output-clock");
        let mut run = playing(&folder, 0, false, small());
        run.clock.follow_output(true);
        let start = Instant::now();

        run.clock.played(128);
        assert_eq!(run.reader.due(start, 64), 2);
        assert!(run.shared.output_clock.load(Ordering::Relaxed));
        // Between callbacks nothing falls due, however long they take within the grace.
        assert_eq!(run.reader.due(start + Duration::from_millis(100), 64), 0);
        run.clock.played(100);
        assert_eq!(run.reader.due(start + Duration::from_millis(150), 64), 1);

        // The output's callbacks stop (a failed device): after the grace the timer takes over,
        // counting from then on, with the 36 frames carried.
        let late = start + Duration::from_millis(150) + OUTPUT_GRACE;
        assert_eq!(run.reader.due(late, 64), 0);
        assert!(!run.shared.output_clock.load(Ordering::Relaxed));
        assert_eq!(run.reader.due(late + Duration::from_millis(10), 64), 8);
    }

    /// The capture is set aside while a take plays: its callback neither meters nor queues, and
    /// what it queued before the take is dropped when the engine goes back to it.
    #[test]
    fn the_capture_is_set_aside_while_a_take_plays_and_comes_back_after() {
        let links = links();
        let (mut writer, live) = source(4, 64, links.clone());
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], Arc::clone(&links.levels));
        pipeline.set_source(live);
        let take = load_take(&take_folder("aside"), &four_inputs(), 4).expect("the take loads");
        let (mut disk, playback, _shared) = playback_link(
            take,
            0,
            false,
            Arc::new(PlaybackClock::new()),
            Arc::clone(&links.stats),
            small(),
        )
        .expect("the take links");
        fill_ring(&mut disk);

        // Less than a block, still in the capture's ring when the take begins.
        writer.write(&[0.75f32; 4 * 30]);
        pipeline.play(playback);
        let samples = links.stats.samples.load(Ordering::Relaxed);
        writer.write(&[0.5f32; 4 * 64]);
        assert_eq!(links.levels.read(4), vec![0, 0, 0, 0]);
        assert_eq!(links.stats.samples.load(Ordering::Relaxed), samples);

        let start = Instant::now();
        pipeline.pump_at(start);
        assert_eq!(pipeline.pump_at(start + Duration::from_micros(1_334)), 1);
        assert!(links.levels.read(4)[0] > 0, "the take is metered");

        pipeline.go_live();
        writer.write(&[-0.25f32; 4 * 64]);
        assert_eq!(pipeline.pump(), 1);

        let blocks = seen.lock().unwrap();
        let values: Vec<f32> = blocks.iter().map(|block| block.value).collect();
        assert_eq!(values, vec![sample(0, 0, 0), -0.25]);
        assert!(
            blocks[1].uniform,
            "the frames from before the take were dropped"
        );
        assert!(blocks.iter().all(|block| block.channels == 4));
        drop(blocks);
        drop(disk);
    }

    /// The engine thread's whole path for a take, from the clock to the sinks and back to the
    /// capture, makes no allocation and frees nothing, with the disk thread running beside it.
    #[test]
    fn a_take_plays_through_the_engine_thread_without_allocating() {
        let links = links();
        let (_writer, live) = source(4, 64, links.clone());
        let recorder = Recorder::new();
        let seen = Arc::clone(&recorder.seen);
        let mut pipeline = Pipeline::new(vec![Box::new(recorder)], Arc::clone(&links.levels));
        pipeline.set_source(live);
        let take =
            load_take(&take_folder("allocations"), &four_inputs(), 4).expect("the take loads");
        let clock = Arc::new(PlaybackClock::new());
        let (disk, playback, shared) = playback_link(
            take,
            0,
            true,
            Arc::clone(&clock),
            Arc::clone(&links.stats),
            LinkOptions::default(),
        )
        .expect("the take links");
        let disk = thread::spawn(move || disk.run());
        // Enough queued for every block below, so the run does not depend on the disk's pace.
        assert!(crate::recorder::tests::until(|| {
            shared.queued.load(Ordering::Acquire) >= 66_000
        }));
        let start = Instant::now();
        // 1334 µs is 64.03 frames: one block falls due at each step.
        let at = |block: u64| start + Duration::from_micros(block * 1_334);
        pipeline.play(playback);
        pipeline.pump_at(at(0));
        assert_eq!(pipeline.pump_at(at(1)), 1);

        let (allocations, frees) = allocations::count(|| {
            for block in 2..1_000 {
                pipeline.pump_at(at(block));
                pipeline.tick(at(block));
            }
            pipeline.go_live();
        });

        assert_eq!((allocations, frees), (0, 0));
        assert!(crate::recorder::tests::until(|| disk.is_finished()));
        disk.join().unwrap();
        let blocks = seen.lock().unwrap();
        assert_eq!(blocks.len(), 999);
        assert_eq!(blocks[0].value, sample(0, 0, 0));
        assert!(!links.stats.suspended.load(Ordering::Relaxed));
    }
}
