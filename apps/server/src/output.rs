//! The engineer's local output: the engine's local mix played on an output device.
//!
//! ```text
//!  engine thread                                       output device callback
//!  LocalOutputSink: mix the local mix (MixPlan, ─▶ rtrb ─▶ OutputPlayer: hold the ring near its
//!  f32) for each block, meter it, push it           ring   target, write the stereo pair into
//!                                                          the chosen channels, convert
//! ```
//!
//! The output runs on its own device clock, which under WASAPI is not the capture's, and there is
//! no resampler: the player absorbs the drift by padding and dropping frames. It keeps the ring
//! near a target fill of one device callback plus two engine blocks. Until the ring holds the
//! target it plays silence (priming). When a callback finds less audio than it needs it plays what
//! there is, pads the rest with silence, counts the padded frames as an underrun and primes again.
//! When the fill rises two blocks above the target it drops the oldest frames back to the target
//! and counts them. A full ring (a stalled device) drops whole blocks at the sink, counted the same.
//!
//! Latency: the ring delays the local mix by the target fill, one device callback plus two blocks
//! (10 ms + 10.7 ms with WASAPI's 480-frame period and the default 256-frame block; 5.3 + 10.7 ms
//! with ASIO at 256 frames), and never by more than one callback plus four blocks before frames are
//! dropped: 31.3 ms and 26.7 ms in those two cases, plus the device's own output buffer.
//!
//! While a take plays back in place of the inputs, the output's callback is its clock: each
//! callback tells the take's [PlaybackClock] the frames it played (see [crate::soundcheck]).
//!
//! The output callback never allocates, locks or panics. Opening follows the engine's host rules
//! (see [offered_outputs]): never ASIO by default, and under ASIO only the outputs of the driver the
//! capture already holds.

use std::error::Error;
use std::sync::atomic::{AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{
    BufferSize, Device, OutputCallbackInfo, SampleFormat, SizedSample, Stream, StreamConfig,
    SupportedStreamConfig,
};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::capture::{
    clamp_buffer_frames, input_host, is_supported_sample_format, sample_format_rank, AudioHost,
    TARGET_SAMPLE_RATE,
};
use crate::engine::OutputMeter;
use crate::mix::{MixPlan, MixRamp, MixState, OUTPUT_CHANNELS};
use crate::pipeline::{Block, Sink, SourceFormat, DEFAULT_BLOCK_FRAMES, MAX_BLOCK_FRAMES};
use crate::protocol::to_pcm16;
use crate::soundcheck::PlaybackClock;

/// The smallest output ring, in stereo frames: half a second at 48 kHz.
const RING_MIN_FRAMES: usize = TARGET_SAMPLE_RATE as usize / 2;

/// The fewest blocks an output ring holds.
const RING_MIN_BLOCKS: usize = 8;

/// Blocks above the device callback the player keeps in the ring.
const TARGET_BLOCKS: usize = 2;

/// Blocks above the target the fill may rise before frames are dropped.
const HIGH_WATER_BLOCKS: usize = 2;

/// One output device the local mix can be played on, with its output channel count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDevice {
    pub name: String,
    pub channels: u16,
}

/// Where to play the local mix: an output device by its exact name, and the first channel of the
/// stereo pair, 0-based (outputs 1-2 are `0`, outputs 3-4 are `2`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalOutputRequest {
    pub device: String,
    pub first_channel: u16,
}

/// Whether the local output plays.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum LocalOutputState {
    /// No output is chosen: the default.
    #[default]
    Off,
    Running,
    /// The output stopped on its own; only the local output, never the engine. It stays failed
    /// until the engineer chooses an output again or turns it off.
    Failed {
        reason: String,
    },
}

/// The local output as the status reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LocalOutputStatus {
    /// The output in use (or the one that failed); `None` when it is off.
    pub output: Option<LocalOutputRequest>,
    pub state: LocalOutputState,
    /// Frames of silence played because no audio was there (since this output was chosen).
    pub underrun_frames: u64,
    /// Frames dropped to bring the delay back to its target, or because the ring was full.
    pub dropped_frames: u64,
}

const RUNNING: u8 = 1;
const FAILED: u8 = 2;

/// What one open output shares between the sink, its device callback, its error callback and the
/// status.
pub(crate) struct OutputShared {
    state: AtomicU8,
    reason: Mutex<Option<String>>,
    underrun_frames: AtomicU64,
    dropped_frames: AtomicU64,
    /// The engine's block, which the player's target follows across device switches.
    block_frames: AtomicUsize,
}

impl OutputShared {
    fn new() -> Self {
        Self {
            state: AtomicU8::new(RUNNING),
            reason: Mutex::new(None),
            underrun_frames: AtomicU64::new(0),
            dropped_frames: AtomicU64::new(0),
            block_frames: AtomicUsize::new(DEFAULT_BLOCK_FRAMES),
        }
    }

    /// Marks the output failed with `reason`. Called from the stream's error callback (never from
    /// the data callback) or by the engine; the sink stops feeding it from its next block.
    pub(crate) fn fail(&self, reason: String) {
        if let Ok(mut stored) = self.reason.lock() {
            *stored = Some(reason);
        }
        self.state.store(FAILED, Ordering::Release);
    }

    fn running(&self) -> bool {
        self.state.load(Ordering::Acquire) == RUNNING
    }

    fn state(&self) -> LocalOutputState {
        if self.running() {
            return LocalOutputState::Running;
        }
        let reason = self
            .reason
            .lock()
            .ok()
            .and_then(|reason| reason.clone())
            .unwrap_or_else(|| "the output device stopped".to_owned());
        LocalOutputState::Failed { reason }
    }

    /// The status of this output, playing `request`.
    pub(crate) fn status(&self, request: &LocalOutputRequest) -> LocalOutputStatus {
        LocalOutputStatus {
            output: Some(request.clone()),
            state: self.state(),
            underrun_frames: self.underrun_frames.load(Ordering::Relaxed),
            dropped_frames: self.dropped_frames.load(Ordering::Relaxed),
        }
    }
}

/// The sink's end of one output's ring.
pub(crate) struct OutputFeed {
    producer: Producer<f32>,
    shared: Arc<OutputShared>,
}

/// The device callback's end of one output's ring: where the pair goes and how full to keep it.
pub(crate) struct OutputPlayer {
    consumer: Consumer<f32>,
    shared: Arc<OutputShared>,
    channels: usize,
    first: usize,
    /// The ring has reached its target since the last underrun.
    primed: bool,
    /// The ring has reached its target at least once: silence from then on is an underrun.
    started: bool,
    /// The clock a take played back follows (see [crate::soundcheck]): every callback tells it
    /// the frames the device played.
    clock: Option<Arc<PlaybackClock>>,
}

/// Both ends of a new output ring, for a device with `channels` outputs playing the pair that
/// starts at `first`, and their shared state.
pub(crate) fn output_link(
    channels: usize,
    first: usize,
    block_frames: usize,
) -> (OutputFeed, OutputPlayer, Arc<OutputShared>) {
    let shared = Arc::new(OutputShared::new());
    shared.block_frames.store(block_frames, Ordering::Relaxed);
    let frames = RING_MIN_FRAMES.max(RING_MIN_BLOCKS * block_frames);
    let (producer, consumer) = RingBuffer::new(frames * usize::from(OUTPUT_CHANNELS));
    let feed = OutputFeed {
        producer,
        shared: Arc::clone(&shared),
    };
    let player = OutputPlayer {
        consumer,
        shared: Arc::clone(&shared),
        channels: channels.max(1),
        first,
        primed: false,
        started: false,
        clock: None,
    };
    (feed, player, shared)
}

/// A sample format an output device takes, converted from f32 in [-1, 1].
pub(crate) trait OutputSample: Copy + Send + 'static {
    fn from_f32(sample: f32) -> Self;
}

impl OutputSample for f32 {
    fn from_f32(sample: f32) -> Self {
        if sample.is_nan() {
            0.0
        } else {
            sample.clamp(-1.0, 1.0)
        }
    }
}

impl OutputSample for i16 {
    fn from_f32(sample: f32) -> Self {
        to_pcm16(sample)
    }
}

impl OutputSample for u16 {
    fn from_f32(sample: f32) -> Self {
        (i32::from(to_pcm16(sample)) + 32_768) as u16
    }
}

impl OutputSample for i32 {
    fn from_f32(sample: f32) -> Self {
        (f64::from(sample) * 2_147_483_648.0)
            .round()
            .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
    }
}

impl OutputPlayer {
    /// Makes this output the clock of a take played back: each callback tells `clock` the frames
    /// the device played.
    pub(crate) fn set_clock(&mut self, clock: Arc<PlaybackClock>) {
        self.clock = Some(clock);
    }

    /// Fills one device buffer of interleaved frames: the local mix's left and right into the
    /// chosen pair, silence in every other channel. Called from the output callback: it never
    /// allocates, locks or panics.
    pub(crate) fn fill<T: OutputSample>(&mut self, data: &mut [T]) {
        let silence = T::from_f32(0.0);
        let frames = data.len() / self.channels;
        let block = self.shared.block_frames.load(Ordering::Relaxed).max(1);
        let target = frames + TARGET_BLOCKS * block;
        let mut available = self.consumer.slots() / 2;

        // Too far ahead (the output clock runs slower, or the device stalled): drop the oldest
        // frames back to the target.
        if available > target + HIGH_WATER_BLOCKS * block {
            let excess = available - target;
            if let Ok(chunk) = self.consumer.read_chunk(2 * excess) {
                chunk.commit_all();
                self.shared
                    .dropped_frames
                    .fetch_add(excess as u64, Ordering::Relaxed);
                available = target;
            }
        }
        if !self.primed && available >= target {
            self.primed = true;
            self.started = true;
        }
        let playable = if self.primed {
            available.min(frames)
        } else {
            0
        };

        let first = self.first;
        let mut played = 0;
        if let Ok(chunk) = self.consumer.read_chunk(2 * playable) {
            let (head, tail) = chunk.as_slices();
            let mut samples = head.iter().chain(tail.iter()).copied();
            for frame in data.chunks_exact_mut(self.channels).take(playable) {
                let left = samples.next().unwrap_or(0.0);
                let right = samples.next().unwrap_or(0.0);
                write_frame(frame, first, T::from_f32(left), T::from_f32(right), silence);
            }
            chunk.commit_all();
            played = playable;
        }
        for frame in data.chunks_exact_mut(self.channels).skip(played) {
            write_frame(frame, first, silence, silence, silence);
        }
        for sample in data.chunks_exact_mut(self.channels).into_remainder() {
            *sample = silence;
        }

        let missing = frames - played;
        if missing > 0 && self.started {
            // The callback ran out of audio: what is missing is padded, and the ring fills to its
            // target again before playing, so one late block costs one gap, not many.
            self.shared
                .underrun_frames
                .fetch_add(missing as u64, Ordering::Relaxed);
            self.primed = false;
        }
        // The device played `frames`, whatever they held: that is the time a take played back
        // follows.
        if let Some(clock) = &self.clock {
            clock.played(frames);
        }
    }
}

#[cfg(test)]
impl OutputPlayer {
    /// Stereo frames waiting in the ring.
    pub(crate) fn queued(&self) -> usize {
        self.consumer.slots() / 2
    }
}

/// One device frame: `left` and `right` on the pair that starts at `first`, `silence` everywhere
/// else.
fn write_frame<T: OutputSample>(frame: &mut [T], first: usize, left: T, right: T, silence: T) {
    for (channel, sample) in frame.iter_mut().enumerate() {
        *sample = if channel == first {
            left
        } else if channel == first + 1 {
            right
        } else {
            silence
        };
    }
}

/// The local output's sink: every block is mixed with the local mix, metered, and pushed to the
/// output's ring when one plays.
///
/// The output is changed between blocks: the engine sends a new feed (or `None` to stop) through
/// [OutputControl], which the sink takes before its next block (or within one idle wait when no
/// audio comes). So a change or a removal never waits on the engine thread and never touches
/// capture or the phones.
///
/// Nothing here allocates or frees on the engine thread: the stereo buffer is sized for the
/// longest block when the sink is built, the feeds travel in preallocated `rtrb` rings, and a feed
/// the sink lets go of is handed back to [OutputControl] to be freed on the engine's side, never
/// dropped here. (Only when the engine thread ends does it drop what it still holds.)
pub(crate) struct LocalOutputSink {
    mix: Arc<MixState>,
    meter: OutputMeter,
    commands: Consumer<Option<OutputFeed>>,
    returns: Producer<OutputFeed>,
    /// A feed the return ring had no room for, handed back at the next chance. The ring is sized
    /// so this cannot happen (see [OutputControl::send]); it is the guard against dropping one.
    parked: Option<OutputFeed>,
    feed: Option<OutputFeed>,
    /// Room for the stereo mix of the longest block.
    stereo: Box<[f32]>,
    block_frames: usize,
    /// The local mix's weights in the previous block, so a fader move glides (see [MixRamp]).
    ramp: MixRamp,
}

/// The engine's end of the local output sink's lines: feeds go to the sink, and the feeds it lets
/// go of come back here to be freed off the engine thread.
pub(crate) struct OutputControl {
    commands: Producer<Option<OutputFeed>>,
    returns: Consumer<OutputFeed>,
}

/// Feeds (or stops) waiting for the sink to take them.
const COMMAND_SLOTS: usize = 4;

/// The local output's sink for the local mix `mix`, metered into `meter`, and the engine's end of
/// its lines.
pub(crate) fn local_output_sink(
    mix: Arc<MixState>,
    meter: OutputMeter,
) -> (LocalOutputSink, OutputControl) {
    let (command_producer, commands) = RingBuffer::new(COMMAND_SLOTS);
    // Every command lets go of at most one feed, and the engine frees the returned feeds before
    // each command it sends, so at most the commands sent since then can come back.
    let (returns, return_consumer) = RingBuffer::new(COMMAND_SLOTS + 1);
    let sink = LocalOutputSink {
        mix,
        meter,
        commands,
        returns,
        parked: None,
        feed: None,
        stereo: vec![0.0; MAX_BLOCK_FRAMES * usize::from(OUTPUT_CHANNELS)].into_boxed_slice(),
        block_frames: DEFAULT_BLOCK_FRAMES,
        ramp: MixRamp::default(),
    };
    let control = OutputControl {
        commands: command_producer,
        returns: return_consumer,
    };
    (sink, control)
}

impl OutputControl {
    /// Sends `feed` to the sink (`None` stops the output), after freeing the feeds it gave back.
    /// An error when the engine thread has ended, or when changes come faster than the sink takes
    /// them (it takes them every block, or every idle wait without audio).
    pub(crate) fn send(&mut self, feed: Option<OutputFeed>) -> Result<(), String> {
        self.reap();
        if self.commands.is_abandoned() {
            return Err("the engine thread has stopped".to_owned());
        }
        self.commands
            .push(feed)
            .map_err(|_| "the local output is still changing; try again".to_owned())
    }

    /// Frees the feeds the sink has let go of, here, off the engine thread; returns how many.
    pub(crate) fn reap(&mut self) -> usize {
        let mut freed = 0;
        while let Ok(feed) = self.returns.pop() {
            drop(feed);
            freed += 1;
        }
        freed
    }
}

impl LocalOutputSink {
    /// Takes the feeds the engine sent, in order, and hands back each one let go of.
    fn take_commands(&mut self) {
        if let Some(parked) = self.parked.take() {
            self.hand_back(parked);
        }
        while let Ok(command) = self.commands.pop() {
            if let Some(feed) = &command {
                feed.shared
                    .block_frames
                    .store(self.block_frames, Ordering::Relaxed);
            }
            if let Some(previous) = std::mem::replace(&mut self.feed, command) {
                self.hand_back(previous);
            }
        }
    }

    fn hand_back(&mut self, feed: OutputFeed) {
        if let Err(rtrb::PushError::Full(feed)) = self.returns.push(feed) {
            self.parked = Some(feed);
        }
    }
}

impl Sink for LocalOutputSink {
    fn process(&mut self, block: &Block<'_>) {
        self.take_commands();

        let frames = block.samples.len() / block.channels.max(1);
        let Some(stereo) = self.stereo.get_mut(..frames * usize::from(OUTPUT_CHANNELS)) else {
            return;
        };
        MixPlan::new(&self.mix.snapshot(), block.channels).mix_ramped(
            &self.ramp,
            block.samples,
            stereo,
        );
        self.meter.observe(stereo);

        if let Some(feed) = &mut self.feed {
            if feed.shared.running() && feed.producer.push_entire_slice(stereo).is_err() {
                feed.shared
                    .dropped_frames
                    .fetch_add(frames as u64, Ordering::Relaxed);
            }
        }
    }

    /// Lets the meter fall, and takes any change of output while no audio comes.
    fn silence(&mut self, frames: usize) {
        self.take_commands();
        self.meter.fall(frames);
    }

    fn source_changed(&mut self, format: SourceFormat) {
        self.block_frames = format.block_frames;
        if let Some(feed) = &self.feed {
            feed.shared
                .block_frames
                .store(format.block_frames, Ordering::Relaxed);
        }
    }
}

/// The outputs the engine offers on `host` while it captures from `capture` (its name and output
/// channels, when the capture device has any).
///
/// Under ASIO only the capture driver's own outputs: ASIO holds one driver at a time and listing
/// any other would load it, so `system` is never called and no driver is opened. On any other host
/// every output device the system lists (`system`), WASAPI's on Windows; the demo source plays on
/// those too, so the engineer hears it on the computer's own outputs. The default host is never
/// ASIO, so no ASIO output is ever offered unless the engineer chose ASIO for capture.
pub(crate) fn offered_outputs(
    host: AudioHost,
    capture: Option<OutputDevice>,
    system: impl FnOnce() -> Result<Vec<OutputDevice>, String>,
) -> Result<Vec<OutputDevice>, String> {
    match host {
        AudioHost::Asio => Ok(capture
            .into_iter()
            .filter(|device| device.channels > 0)
            .collect()),
        AudioHost::Auto | AudioHost::Wasapi | AudioHost::Demo => system(),
    }
}

/// Checks `request` against the outputs `offered` on `host`: the device must be one of them, named
/// exactly, and its pair must lie within the device's outputs.
pub(crate) fn check_request(
    host: AudioHost,
    offered: &[OutputDevice],
    request: &LocalOutputRequest,
) -> Result<(), String> {
    let name = &request.device;
    let Some(device) = offered.iter().find(|device| device.name == *name) else {
        return Err(match host {
            AudioHost::Asio => format!(
                "\"{name}\" is not an output the engine offers: under ASIO only the capture driver's own outputs can be used"
            ),
            AudioHost::Auto | AudioHost::Wasapi | AudioHost::Demo => {
                format!("\"{name}\" is not an output device of this computer")
            }
        });
    };
    let last = u32::from(request.first_channel) + 2;
    if last > u32::from(device.channels) {
        return Err(format!(
            "outputs {}-{last} are past \"{name}\", which has {} outputs",
            last - 1,
            device.channels
        ));
    }
    Ok(())
}

/// The system host's output devices, with their output channel counts at 48 kHz.
pub(crate) fn system_output_devices() -> Result<Vec<OutputDevice>, String> {
    let host = input_host(AudioHost::Auto);
    let devices = host
        .output_devices()
        .map_err(|error| format!("could not enumerate output devices: {error}"))?;
    Ok(devices
        .map(|device| OutputDevice {
            name: device.to_string(),
            channels: output_channels(&device),
        })
        .collect())
}

/// The most output channels `device` offers at 48 kHz in a supported sample format, or 0.
pub(crate) fn output_channels(device: &Device) -> u16 {
    device
        .supported_output_configs()
        .map(|configs| {
            configs
                .filter(|range| {
                    is_supported_sample_format(range.sample_format())
                        && range.min_sample_rate() <= TARGET_SAMPLE_RATE
                        && TARGET_SAMPLE_RATE <= range.max_sample_rate()
                })
                .map(|range| range.channels())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

/// Finds a system output device by its exact name.
pub(crate) fn system_output_device(name: &str) -> Result<Device, String> {
    input_host(AudioHost::Auto)
        .output_devices()
        .map_err(|error| format!("could not enumerate output devices: {error}"))?
        .find(|device| device.to_string() == name)
        .ok_or_else(|| format!("output device \"{name}\" is no longer available"))
}

/// The configuration to open `device` with: 48 kHz (there is no resampler), at least `needed`
/// channels, the device's own default when it qualifies (WASAPI's shared-mode mix format), else the
/// cheapest sample format with the fewest channels.
fn select_output_config(device: &Device, needed: u16) -> Result<SupportedStreamConfig, String> {
    let qualifies = |config: &SupportedStreamConfig| {
        config.sample_rate() == TARGET_SAMPLE_RATE
            && config.channels() >= needed
            && is_supported_sample_format(config.sample_format())
    };
    if let Ok(default) = device.default_output_config() {
        if qualifies(&default) {
            return Ok(default);
        }
    }
    device
        .supported_output_configs()
        .map_err(|error| format!("could not read the output configurations: {error}"))?
        .filter(|range| {
            is_supported_sample_format(range.sample_format())
                && range.channels() >= needed
                && range.min_sample_rate() <= TARGET_SAMPLE_RATE
                && TARGET_SAMPLE_RATE <= range.max_sample_rate()
        })
        .min_by_key(|range| (sample_format_rank(range.sample_format()), range.channels()))
        .map(|range| range.with_sample_rate(TARGET_SAMPLE_RATE))
        .ok_or_else(|| {
            format!(
                "the output has no configuration with {needed} channels at {TARGET_SAMPLE_RATE} Hz (there is no resampler yet)"
            )
        })
}

/// An output opened and playing, not yet the engine's: what [crate::EngineHandle] installs. A
/// device opens one through cpal; a test opens one with [output_link] and drives the player itself.
pub(crate) struct OpenedOutput {
    pub(crate) stream: Option<Stream>,
    pub(crate) request: LocalOutputRequest,
    pub(crate) feed: OutputFeed,
    pub(crate) shared: Arc<OutputShared>,
}

/// Opens `device` for `request`'s pair and starts it, with a fresh ring sized for `block_frames`.
///
/// `buffer_frames` is the capture's buffer: an ASIO driver runs one buffer size for input and
/// output, so it is used (within the driver's range) when `host` is ASIO; elsewhere the device
/// keeps its own period. A device error later marks only this output failed. Every callback ticks
/// `clock`, which a take played back follows.
pub(crate) fn open_output(
    device: &Device,
    host: AudioHost,
    request: &LocalOutputRequest,
    buffer_frames: Option<u32>,
    block_frames: usize,
    clock: Arc<PlaybackClock>,
) -> Result<OpenedOutput, Box<dyn Error>> {
    let needed = request.first_channel.saturating_add(2);
    let supported = select_output_config(device, needed)?;
    let mut config = supported.config();
    config.buffer_size = match (host, buffer_frames) {
        (AudioHost::Asio, Some(frames)) => {
            BufferSize::Fixed(clamp_buffer_frames(frames, supported.buffer_size()))
        }
        _ => BufferSize::Default,
    };
    let (feed, mut player, shared) = output_link(
        usize::from(config.channels),
        usize::from(request.first_channel),
        block_frames,
    );
    player.set_clock(clock);
    let stream = match supported.sample_format() {
        SampleFormat::F32 => open_typed::<f32>(device, config, player, &shared),
        SampleFormat::I16 => open_typed::<i16>(device, config, player, &shared),
        SampleFormat::U16 => open_typed::<u16>(device, config, player, &shared),
        SampleFormat::I32 => open_typed::<i32>(device, config, player, &shared),
        format => Err(format!("the output uses unsupported sample format {format:?}").into()),
    }?;
    stream
        .play()
        .map_err(|error| format!("could not start the output: {error}"))?;
    Ok(OpenedOutput {
        stream: Some(stream),
        request: request.clone(),
        feed,
        shared,
    })
}

fn open_typed<T: SizedSample + OutputSample>(
    device: &Device,
    config: StreamConfig,
    mut player: OutputPlayer,
    shared: &Arc<OutputShared>,
) -> Result<Stream, Box<dyn Error>> {
    let failed = Arc::clone(shared);
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info: &OutputCallbackInfo| player.fill(data),
            move |error| failed.fail(format!("the output device failed: {error}")),
            None,
        )
        .map_err(|error| format!("could not open the output: {error}").into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mix::MixUpdate;
    use crate::pipeline::tests::allocations;

    fn request(device: &str, first_channel: u16) -> LocalOutputRequest {
        LocalOutputRequest {
            device: device.to_owned(),
            first_channel,
        }
    }

    fn device(name: &str, channels: u16) -> OutputDevice {
        OutputDevice {
            name: name.to_owned(),
            channels,
        }
    }

    /// `frames` stereo frames of `left` and `right`, pushed as the sink would.
    fn push(feed: &mut OutputFeed, frames: usize, left: f32, right: f32) {
        let samples: Vec<f32> = (0..frames).flat_map(|_| [left, right]).collect();
        feed.producer
            .push_entire_slice(&samples)
            .expect("the ring has room");
    }

    fn counts(shared: &OutputShared) -> (u64, u64) {
        (
            shared.underrun_frames.load(Ordering::Relaxed),
            shared.dropped_frames.load(Ordering::Relaxed),
        )
    }

    /// A 4-channel device playing the pair 3-4, with 4-frame blocks: the target is the callback
    /// (8 frames) plus two blocks, 16 frames.
    fn four_channel_player() -> (OutputFeed, OutputPlayer, Arc<OutputShared>) {
        output_link(4, 2, 4)
    }

    #[test]
    fn the_player_waits_for_its_target_then_plays_the_pair_into_its_channels() {
        let (mut feed, mut player, shared) = four_channel_player();
        let mut buffer = [9.0f32; 4 * 8];

        push(&mut feed, 15, 0.25, -0.5);
        player.fill(&mut buffer);
        assert!(
            buffer.iter().all(|sample| *sample == 0.0),
            "priming plays silence"
        );

        push(&mut feed, 1, 0.25, -0.5);
        player.fill(&mut buffer);
        for frame in buffer.chunks_exact(4) {
            assert_eq!(frame, [0.0, 0.0, 0.25, -0.5]);
        }
        assert_eq!(counts(&shared), (0, 0), "priming is no underrun");
        assert_eq!(
            feed.producer.buffer().capacity() - feed.producer.slots(),
            2 * 8
        );
    }

    #[test]
    fn an_underrun_pads_with_silence_counts_it_and_primes_again() {
        let (mut feed, mut player, shared) = four_channel_player();
        let mut buffer = [0.0f32; 4 * 8];
        push(&mut feed, 16, 0.5, 0.5);
        player.fill(&mut buffer);

        // Eight frames are left and three more arrive: one whole callback, then three of eight.
        push(&mut feed, 3, 0.25, 0.25);
        player.fill(&mut buffer);
        assert_eq!(counts(&shared).0, 0, "eight were there");
        player.fill(&mut buffer);
        let frames: Vec<f32> = buffer.chunks_exact(4).map(|frame| frame[2]).collect();
        assert_eq!(frames, [0.25, 0.25, 0.25, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(counts(&shared), (5, 0));

        // Primed again: silence until the target is back, and that silence is counted too.
        push(&mut feed, 8, 0.5, 0.5);
        player.fill(&mut buffer);
        assert!(buffer.iter().all(|sample| *sample == 0.0));
        assert_eq!(counts(&shared), (13, 0));
    }

    #[test]
    fn a_fill_two_blocks_above_the_target_is_dropped_back_to_it_and_counted() {
        let (mut feed, mut player, shared) = four_channel_player();
        let mut buffer = [0.0f32; 4 * 8];
        // The target is 16 frames and the high-water mark 24: 30 frames are 14 too many.
        push(&mut feed, 14, 0.1, 0.1);
        push(&mut feed, 16, 0.2, 0.2);

        player.fill(&mut buffer);

        assert_eq!(counts(&shared), (0, 14), "the oldest frames go");
        assert!(buffer.chunks_exact(4).all(|frame| frame[2] == 0.2));
        assert_eq!(
            feed.producer.buffer().capacity() - feed.producer.slots(),
            2 * 8
        );

        // At the high-water mark nothing is dropped.
        push(&mut feed, 16, 0.3, 0.3);
        player.fill(&mut buffer);
        assert_eq!(counts(&shared), (0, 14));
    }

    #[test]
    fn samples_convert_to_the_device_format() {
        assert_eq!(f32::from_f32(0.5), 0.5);
        assert_eq!(f32::from_f32(f32::NAN), 0.0);
        assert_eq!(i16::from_f32(0.5), 16_384);
        assert_eq!(u16::from_f32(0.0), 32_768, "u16 silence is the midpoint");
        assert_eq!(u16::from_f32(-1.0), 0);
        assert_eq!(i32::from_f32(1.0), i32::MAX);
        assert_eq!(i32::from_f32(-1.0), i32::MIN);
        assert_eq!(i32::from_f32(1.0 / 8_388_608.0), 256, "one 24-bit step");
    }

    #[test]
    fn the_output_callback_never_allocates() {
        let (mut feed, mut player, _shared) = output_link(8, 4, 256);
        let block = vec![0.25f32; 2 * 256];
        let mut buffer = vec![0i32; 8 * 480];
        for _ in 0..4 {
            feed.producer.push_entire_slice(&block).expect("room");
        }
        player.fill(&mut buffer);

        let (allocations, frees) = allocations::count(|| {
            for round in 0..2_000 {
                if round % 2 == 0 {
                    let _ = feed.producer.push_entire_slice(&block);
                    let _ = feed.producer.push_entire_slice(&block);
                }
                player.fill(&mut buffer);
            }
        });

        assert_eq!((allocations, frees), (0, 0));
    }

    fn sink_with(mix: Arc<MixState>) -> (LocalOutputSink, OutputControl, OutputMeter) {
        let meter = OutputMeter::default();
        let (mut sink, outputs) = local_output_sink(mix, meter.clone());
        sink.source_changed(SourceFormat {
            channels: 2,
            block_frames: 4,
        });
        (sink, outputs, meter)
    }

    fn block(samples: &[f32], sequence: u64) -> Block<'_> {
        Block {
            samples,
            raw: samples,
            channels: 2,
            sequence,
        }
    }

    /// The sink plays the local mix (here at half volume) and meters it, from the block after the
    /// output arrives; without an output it still meters.
    #[test]
    fn the_sink_plays_and_meters_the_local_mix() {
        let mix = Arc::new(MixState::default());
        mix.apply(&MixUpdate {
            volume_percent: Some(50),
            ..MixUpdate::default()
        })
        .expect("a valid update");
        let (mut sink, mut outputs, meter) = sink_with(Arc::clone(&mix));
        let input = [0.5f32, 0.25, 0.5, 0.25, 0.5, 0.25, 0.5, 0.25];

        sink.process(&block(&input, 0));
        assert_eq!(meter.read(), [25, 12], "metered with no output");

        let (feed, mut player, shared) = output_link(2, 0, 4);
        outputs.send(Some(feed)).expect("the sink listens");
        for sequence in 1..=4 {
            sink.process(&block(&input, sequence));
        }
        let mut buffer = [0.0f32; 2 * 8];
        player.fill(&mut buffer);
        for frame in buffer.chunks_exact(2) {
            assert_eq!(frame, [0.25, 0.125]);
        }
        assert_eq!(counts(&shared), (0, 0));
    }

    /// A new output takes over between two blocks and the previous one is fed no more; `None`
    /// stops the output. A failed output is not fed either.
    #[test]
    fn the_sink_moves_between_outputs_and_stops_feeding_a_failed_one() {
        let (mut sink, mut outputs, _meter) = sink_with(Arc::new(MixState::default()));
        let input = [0.5f32; 8];
        let (first, mut first_player, _) = output_link(2, 0, 4);
        let (second, mut second_player, second_shared) = output_link(2, 0, 4);
        let filled = |player: &mut OutputPlayer| player.consumer.slots() / 2;

        outputs.send(Some(first)).expect("the sink listens");
        sink.process(&block(&input, 0));
        outputs.send(Some(second)).expect("the sink listens");
        sink.process(&block(&input, 1));
        assert_eq!(filled(&mut first_player), 4);
        assert_eq!(filled(&mut second_player), 4);

        second_shared.fail("unplugged".to_owned());
        sink.process(&block(&input, 2));
        assert_eq!(filled(&mut second_player), 4, "a failed output is not fed");
        assert_eq!(
            second_shared.status(&request("B", 0)).state,
            LocalOutputState::Failed {
                reason: "unplugged".to_owned()
            }
        );

        outputs.send(None).expect("the sink listens");
        sink.process(&block(&input, 3));
        assert!(sink.feed.is_none());
    }

    /// The output a live change replaces is handed back, not freed: the block that makes the
    /// change frees nothing on the engine thread, even when the previous output's stream (here its
    /// player) is already gone, so the sink would hold the last reference to its ring.
    #[test]
    fn a_live_output_change_frees_nothing_on_the_engine_thread() {
        let (mut sink, mut outputs, _meter) = sink_with(Arc::new(MixState::default()));
        let input = [0.5f32; 8];
        let (first, first_player, _) = output_link(2, 0, 4);
        outputs.send(Some(first)).expect("the sink listens");
        sink.process(&block(&input, 0));
        drop(first_player);
        let (second, _second_player, _) = output_link(2, 0, 4);
        outputs.send(Some(second)).expect("the sink listens");

        let (allocations, frees) = allocations::count(|| sink.process(&block(&input, 1)));

        assert_eq!((allocations, frees), (0, 0));
        // The previous output is freed where the engine reaps it, off the engine thread.
        let (_, freed) = allocations::count(|| assert_eq!(outputs.reap(), 1));
        assert!(freed > 0, "the previous output's ring is freed by the reap");
    }

    /// With no audio coming, the sink still takes a change of output within one idle wait.
    #[test]
    fn a_change_of_output_is_taken_while_no_audio_comes() {
        let (mut sink, mut outputs, _meter) = sink_with(Arc::new(MixState::default()));
        let (feed, _player, _) = output_link(2, 0, 4);
        outputs.send(Some(feed)).expect("the sink listens");

        sink.silence(4_800);

        assert!(sink.feed.is_some());
    }

    /// Changes faster than the sink takes them are refused, never dropped on the engine thread;
    /// an ended engine thread refuses every change.
    #[test]
    fn changes_the_sink_has_not_taken_yet_are_bounded() {
        let (sink, mut outputs, _meter) = sink_with(Arc::new(MixState::default()));
        for _ in 0..COMMAND_SLOTS {
            outputs.send(None).expect("there is room");
        }
        let refused = outputs.send(None).expect_err("the line is full");
        assert!(refused.contains("still changing"), "{refused}");

        drop(sink);
        let ended = outputs.send(None).expect_err("nobody takes it");
        assert!(ended.contains("stopped"), "{ended}");
    }

    #[test]
    fn a_full_ring_drops_the_block_and_counts_it() {
        let (mut sink, mut outputs, _meter) = sink_with(Arc::new(MixState::default()));
        let (feed, _player, shared) = output_link(2, 0, 4);
        let capacity = feed.producer.buffer().capacity() / 2;
        outputs.send(Some(feed)).expect("the sink listens");
        let input = [0.5f32; 8];

        for sequence in 0..(capacity / 4 + 3) as u64 {
            sink.process(&block(&input, sequence));
        }

        assert_eq!(counts(&shared), (0, 12));
    }

    #[test]
    fn under_asio_only_the_capture_drivers_outputs_are_offered_and_nothing_is_listed() {
        let never = || -> Result<Vec<OutputDevice>, String> {
            panic!("listing outputs under ASIO would load other drivers")
        };

        let offered =
            offered_outputs(AudioHost::Asio, Some(device("Volt ASIO", 4)), never).unwrap();
        assert_eq!(offered, vec![device("Volt ASIO", 4)]);
        assert!(offered_outputs(AudioHost::Asio, None, never)
            .unwrap()
            .is_empty());
        assert!(
            offered_outputs(AudioHost::Asio, Some(device("Mic ASIO", 0)), never)
                .unwrap()
                .is_empty(),
            "a driver without outputs offers none"
        );
    }

    #[test]
    fn under_the_system_host_every_output_device_is_offered() {
        let system = || Ok(vec![device("Speakers", 2), device("Volt", 4)]);

        for host in [AudioHost::Auto, AudioHost::Wasapi] {
            assert_eq!(
                offered_outputs(host, Some(device("Volt", 4)), system).unwrap(),
                vec![device("Speakers", 2), device("Volt", 4)]
            );
        }
    }

    #[test]
    fn a_request_outside_the_offered_outputs_or_their_channels_is_refused() {
        let asio = [device("Volt ASIO", 4)];
        let refused = check_request(AudioHost::Asio, &asio, &request("Realtek ASIO", 0))
            .expect_err("another ASIO driver is refused");
        assert!(refused.contains("only the capture driver"), "{refused}");
        assert!(check_request(AudioHost::Asio, &asio, &request("Volt ASIO", 2)).is_ok());

        let past = check_request(AudioHost::Asio, &asio, &request("Volt ASIO", 3))
            .expect_err("outputs 4-5 do not exist");
        assert!(past.contains("4 outputs"), "{past}");

        let system = [device("Speakers", 2)];
        assert!(check_request(AudioHost::Auto, &system, &request("Speakers", 0)).is_ok());
        let unknown = check_request(AudioHost::Auto, &system, &request("Speaker", 0))
            .expect_err("names match exactly");
        assert!(unknown.contains("not an output"), "{unknown}");
    }

    #[test]
    fn the_status_reports_the_output_its_state_and_its_counts() {
        let (_feed, _player, shared) = output_link(2, 0, 4);
        shared.underrun_frames.store(7, Ordering::Relaxed);

        assert_eq!(
            shared.status(&request("Speakers", 0)),
            LocalOutputStatus {
                output: Some(request("Speakers", 0)),
                state: LocalOutputState::Running,
                underrun_frames: 7,
                dropped_frames: 0,
            }
        );
        assert_eq!(LocalOutputStatus::default().state, LocalOutputState::Off);
    }
}
