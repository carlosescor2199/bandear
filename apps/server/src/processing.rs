//! The per-input processing stage: a console's channel strip (see [crate::dsp::strip]) on every
//! input, before the mixes, so every musician hears the processed channel.
//!
//! ```text
//!  block (raw) ─▶ ProcessingStage ─▶ processed block ─▶ network, local output, recorder's local mix
//!       └──────────────────────────────────────────▶ recorder's input tracks (as captured)
//! ```
//!
//! It runs on the engine thread between the block's pop (or the demo's or take's fill) and the
//! sinks: the raw block is copied into a second buffer and each working strip processes its own
//! channels there in place. While every strip is idle (neutral settings, or turned off and glided
//! out) the stage does nothing at all, not even the copy: the sinks get the raw block, bit for bit,
//! so the phones' packets are what they were before processing existed.
//!
//! **Layout.** One strip per input, for the first [MAX_MIX_CHANNELS] inputs (the mixer's); a pair
//! the engineer linked ([crate::engine::ChannelMeta::linked_to_next]) is one stereo strip with one
//! detector on its first channel's settings. A pair's two sides keep the same settings: setting
//! either sets both, and linking a pair gives its second side the first side's.
//!
//! **Real time.** The engine side ([Processing]) keeps the settings, one per channel index, for
//! the whole engine run (device switches included), and works out each strip's design off the
//! engine thread. A design travels to the stage as plain numbers through a preallocated command
//! ring; a new layout (another channel count, a pair linked or unlinked) travels as a whole strip
//! set built off the engine thread, and the set it replaces goes back through a return ring to be
//! freed on the engine side. Strips whose channels are the same in both layouts carry their
//! running state over, so they do not restart. Nothing on the engine thread allocates or frees.
//!
//! **Meters.** Per channel, the level after processing and the gate's and compressor's gain
//! reduction, published through atomics ([ProcessingMeters]); a channel whose strip is idle reads
//! its input level, which is then the same thing.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};

use crate::dsp::strip::{StripDesign, StripProcessor, StripSettings};
use crate::engine::{ChannelMeta, LevelMeter};
use crate::mix::MAX_MIX_CHANNELS;
use crate::pipeline::SourceFormat;

/// Changes waiting for the stage to take them: every strip's design several times over, so a
/// session load and an editor dragging a control between two idle checks (100 ms without audio)
/// both fit. The stage takes them before every block.
const COMMAND_SLOTS: usize = 256;

/// Which channels each strip processes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct StripLayout {
    /// Per channel, the width of the strip that starts there: 1, 2 for a linked pair, 0 for the
    /// second side of a pair and for channels the source lacks.
    widths: [u8; MAX_MIX_CHANNELS],
}

impl StripLayout {
    /// The strips for a source whose channel details are `channels`, one entry per input (as
    /// [crate::control::ConfigSnapshot::channels] fits them): a strip per input, a linked pair as
    /// one. Inputs past the mixer's channels have none.
    pub(crate) fn new(channels: &[ChannelMeta]) -> Self {
        let count = channels.len().min(MAX_MIX_CHANNELS);
        let mut widths = [0u8; MAX_MIX_CHANNELS];
        let mut channel = 0;
        while channel < count {
            let paired = channels[channel].linked_to_next && channel + 1 < count;
            widths[channel] = if paired { 2 } else { 1 };
            channel += usize::from(widths[channel]);
        }
        Self { widths }
    }

    /// `channels` inputs, each on its own.
    #[cfg(test)]
    pub(crate) fn mono(channels: usize) -> Self {
        Self::new(&vec![
            ChannelMeta::default();
            channels.min(MAX_MIX_CHANNELS)
        ])
    }

    /// The width of the strip that starts at `channel`, 0 when none starts there.
    pub(crate) fn width(&self, channel: usize) -> usize {
        self.widths
            .get(channel)
            .map_or(0, |width| usize::from(*width))
    }

    /// The first channel of the strip that processes `channel`, and its width.
    pub(crate) fn strip_of(&self, channel: usize) -> Option<(usize, usize)> {
        if self.width(channel) > 0 {
            return Some((channel, self.width(channel)));
        }
        channel
            .checked_sub(1)
            .filter(|first| self.width(*first) == 2)
            .map(|first| (first, 2))
    }

    /// Every strip, as its first channel and width, in channel order.
    pub(crate) fn strips(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
        self.widths
            .iter()
            .enumerate()
            .filter(|(_, width)| **width > 0)
            .map(|(first, width)| (first, usize::from(*width)))
    }
}

/// A whole set of strips for one layout, built off the engine thread.
pub(crate) struct StripSet {
    layout: StripLayout,
    /// By first channel; an entry where no strip starts is never run.
    strips: [StripProcessor; MAX_MIX_CHANNELS],
    /// The design each strip was last given, so a strip carried over from the set this one
    /// replaces can take it.
    designs: [StripDesign; MAX_MIX_CHANNELS],
}

impl StripSet {
    /// The strips of `layout` with `settings` (indexed by channel; each strip takes its first
    /// channel's), applied. Allocates: call it off the engine thread.
    pub(crate) fn new(layout: StripLayout, settings: &[StripSettings]) -> Box<Self> {
        let mut set = Box::new(Self {
            layout,
            strips: std::array::from_fn(|_| StripProcessor::new(1)),
            designs: [StripDesign::default(); MAX_MIX_CHANNELS],
        });
        for (first, width) in layout.strips() {
            let design = settings.get(first).copied().unwrap_or_default().design();
            set.strips[first] = StripProcessor::new(width);
            set.strips[first].apply(&design);
            set.designs[first] = design;
        }
        set
    }

    fn any_working(&self) -> bool {
        self.layout
            .strips()
            .any(|(first, _)| self.strips[first].is_active())
    }
}

/// What the engine asks of the stage.
pub(crate) enum ProcessingCommand {
    /// The strip starting at `first` takes `design` (it glides there without a click).
    Design { first: usize, design: StripDesign },
    /// Another layout: this set takes over from the next block, carrying over the running state
    /// of every strip on the same channels; the set it replaces goes back to the engine.
    Strips(Box<StripSet>),
}

/// The processing meters, written by the engine thread and read by the status.
pub(crate) struct ProcessingMeters {
    /// Each channel's peak after processing, while its strip works.
    levels: LevelMeter,
    /// One bit per channel whose strip works: its level is read from [Self::levels], the others'
    /// from the input meters (the same audio).
    working: AtomicU32,
    /// Each channel's gate and compressor gain reduction in dB, as f32 bits.
    gate: [AtomicU32; MAX_MIX_CHANNELS],
    compressor: [AtomicU32; MAX_MIX_CHANNELS],
    /// Samples that were not finite numbers, processed as silence (see [StripProcessor]).
    non_finite: AtomicU64,
}

/// What [ProcessingMeters::read] reports for each channel.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ProcessingReading {
    /// The 0-100 peak after processing, on the input meters' scale.
    pub(crate) levels: Vec<u8>,
    /// The gate's gain reduction in dB (0 open).
    pub(crate) gate_reduction_db: Vec<f32>,
    /// The compressor's gain reduction in dB, before the makeup.
    pub(crate) compressor_reduction_db: Vec<f32>,
}

impl ProcessingMeters {
    /// Nothing metered yet.
    pub(crate) fn new() -> Self {
        Self {
            levels: LevelMeter::new(),
            working: AtomicU32::new(0),
            gate: std::array::from_fn(|_| AtomicU32::new(0)),
            compressor: std::array::from_fn(|_| AtomicU32::new(0)),
            non_finite: AtomicU64::new(0),
        }
    }

    /// The levels after processing and the gain reductions of `channels` channels, whose input
    /// levels `inputs` meters. A pure read.
    pub(crate) fn read(&self, channels: usize, inputs: &LevelMeter) -> ProcessingReading {
        let working = self.working.load(Ordering::Relaxed);
        let processed = self.levels.read(channels);
        let input = inputs.read(channels);
        let reduction = |values: &[AtomicU32; MAX_MIX_CHANNELS]| -> Vec<f32> {
            (0..channels)
                .map(|channel| {
                    values
                        .get(channel)
                        .map_or(0.0, |value| f32::from_bits(value.load(Ordering::Relaxed)))
                })
                .collect()
        };
        ProcessingReading {
            levels: (0..channels)
                .map(|channel| {
                    let processing = channel < MAX_MIX_CHANNELS && working & (1 << channel) != 0;
                    if processing {
                        processed[channel]
                    } else {
                        input[channel]
                    }
                })
                .collect(),
            gate_reduction_db: reduction(&self.gate),
            compressor_reduction_db: reduction(&self.compressor),
        }
    }

    /// Samples the strips took as silence because they were not finite numbers, since the engine
    /// started.
    pub(crate) fn non_finite_samples(&self) -> u64 {
        self.non_finite.load(Ordering::Relaxed)
    }

    /// Back to nothing: no level, no reduction. For a source that stopped or was replaced.
    pub(crate) fn clear(&self) {
        self.levels.clear();
        self.clear_reductions();
    }

    fn clear_reductions(&self) {
        for value in self.gate.iter().chain(&self.compressor) {
            value.store(0, Ordering::Relaxed);
        }
    }
}

/// The stage on the engine thread.
pub(crate) struct ProcessingStage {
    set: Box<StripSet>,
    commands: Consumer<ProcessingCommand>,
    returns: Producer<Box<StripSet>>,
    /// A set the return ring had no room for, handed back at the next chance (the ring is sized
    /// so this cannot happen; it is the guard against freeing one here).
    parked: Option<Box<StripSet>>,
    /// The processed block, sized for the source's blocks when it changes.
    processed: Vec<f32>,
    /// Some strip works: the stage copies and processes the block.
    running: bool,
    meters: Arc<ProcessingMeters>,
    /// Samples taken as silence so far, published to [ProcessingMeters::non_finite_samples].
    non_finite: u64,
}

/// The engine's end of the stage's lines: commands go to the stage, and the strip sets it lets go
/// of come back here to be freed off the engine thread.
pub(crate) struct ProcessingControl {
    commands: Producer<ProcessingCommand>,
    returns: Consumer<Box<StripSet>>,
}

/// A stage with no strips yet, and the engine's end of its lines.
pub(crate) fn processing_stage() -> (ProcessingStage, ProcessingControl) {
    let (command_producer, commands) = RingBuffer::new(COMMAND_SLOTS);
    // Every command replaces at most one set, and the engine frees the returned sets before each
    // command it sends, so at most the commands sent since then can come back.
    let (returns, return_consumer) = RingBuffer::new(COMMAND_SLOTS + 1);
    let stage = ProcessingStage {
        set: StripSet::new(StripLayout::default(), &[]),
        commands,
        returns,
        parked: None,
        processed: Vec::new(),
        running: false,
        meters: Arc::new(ProcessingMeters::new()),
        non_finite: 0,
    };
    let control = ProcessingControl {
        commands: command_producer,
        returns: return_consumer,
    };
    (stage, control)
}

impl ProcessingControl {
    /// Frees the strip sets the stage has let go of, here, off the engine thread.
    fn reap(&mut self) {
        while let Ok(set) = self.returns.pop() {
            drop(set);
        }
    }

    /// Sends `commands` in order, all of them or none: an error when the engine thread has ended,
    /// or when changes come faster than the stage takes them.
    fn send(&mut self, commands: Vec<ProcessingCommand>) -> Result<(), String> {
        self.reap();
        if self.commands.is_abandoned() {
            return Err("the engine thread has stopped".to_owned());
        }
        if self.commands.slots() < commands.len() {
            return Err("the channel processing is still changing; try again".to_owned());
        }
        for command in commands {
            // Room was checked above, and only this side pushes.
            if self.commands.push(command).is_err() {
                return Err("the channel processing is still changing; try again".to_owned());
            }
        }
        Ok(())
    }
}

impl ProcessingStage {
    /// Where the stage publishes its meters.
    pub(crate) fn meters(&self) -> Arc<ProcessingMeters> {
        Arc::clone(&self.meters)
    }

    /// Sizes the processed block for the new source's blocks, off the per-block path.
    pub(crate) fn source_changed(&mut self, format: SourceFormat) {
        self.processed.clear();
        self.processed
            .resize(format.block_frames * format.channels, 0.0);
        self.meters.clear();
    }

    /// Takes the designs and strip sets the engine sent, in order.
    pub(crate) fn take_commands(&mut self) {
        if let Some(parked) = self.parked.take() {
            self.hand_back(parked);
        }
        let mut changed = false;
        // A set still parked means the return ring is full: the next one waits in its ring.
        while self.parked.is_none() {
            let Ok(command) = self.commands.pop() else {
                break;
            };
            changed = true;
            match command {
                ProcessingCommand::Design { first, design } => {
                    if self.set.layout.width(first) > 0 {
                        self.set.strips[first].apply(&design);
                        self.set.designs[first] = design;
                    }
                }
                ProcessingCommand::Strips(mut set) => {
                    // Before a strip is carried over with what it counted.
                    self.count_non_finite();
                    for (first, width) in set.layout.strips() {
                        if self.set.layout.width(first) == width {
                            // Copies fixed-size state: nothing is allocated.
                            set.strips[first].clone_from(&self.set.strips[first]);
                            let design = set.designs[first];
                            set.strips[first].apply(&design);
                        }
                    }
                    std::mem::swap(&mut self.set, &mut set);
                    self.hand_back(set);
                    self.meters.clear_reductions();
                }
            }
        }
        if changed {
            let was_running = self.running;
            self.running = self.set.any_working();
            if self.running && !was_running {
                // Not metered while idle: start from this block, not from a level long gone.
                self.meters.levels.clear();
            }
            if !self.running {
                self.meters.working.store(0, Ordering::Relaxed);
            }
        }
    }

    fn hand_back(&mut self, set: Box<StripSet>) {
        if let Err(rtrb::PushError::Full(set)) = self.returns.push(set) {
            self.parked = Some(set);
        }
    }

    /// Adds what the strips counted to the published total.
    fn count_non_finite(&mut self) {
        let StripSet { layout, strips, .. } = &mut *self.set;
        let counted: u64 = layout
            .strips()
            .map(|(first, _)| strips[first].take_non_finite_samples())
            .sum();
        if counted > 0 {
            self.non_finite += counted;
            self.meters
                .non_finite
                .store(self.non_finite, Ordering::Relaxed);
        }
    }

    /// Processes `raw`, `channels`-interleaved, into the stage's own buffer and returns it, or
    /// `None` when no strip works: the raw block is then the processed one, untouched and not
    /// copied.
    pub(crate) fn process(&mut self, raw: &[f32], channels: usize) -> Option<&[f32]> {
        self.take_commands();
        if !self.running || channels == 0 {
            return None;
        }
        let processed = self.processed.get_mut(..raw.len())?;
        processed.copy_from_slice(raw);
        let mut working = 0u32;
        let StripSet { layout, strips, .. } = &mut *self.set;
        for (first, width) in layout.strips() {
            let strip = &mut strips[first];
            strip.process(processed, channels, first);
            let gate = strip.gate_reduction_db().to_bits();
            let compressor = strip.compressor_reduction_db().to_bits();
            let bits = if strip.is_active() {
                ((1u32 << width) - 1) << first
            } else {
                0
            };
            working |= bits;
            for channel in first..first + width {
                self.meters.gate[channel].store(gate, Ordering::Relaxed);
                self.meters.compressor[channel].store(compressor, Ordering::Relaxed);
            }
        }
        self.meters.levels.observe(processed, channels);
        self.meters.working.store(working, Ordering::Relaxed);
        self.running = working != 0;
        self.count_non_finite();
        Some(&self.processed[..raw.len()])
    }

    /// No audio came for `frames`: the levels after processing fall by that much silence, as the
    /// input meters do. The commands waiting are taken too.
    pub(crate) fn silence(&mut self, frames: usize) {
        self.take_commands();
        self.meters.levels.fall(frames);
    }

    /// Back to no level, for a take that starts or ends in place of the capture.
    pub(crate) fn clear_meters(&self) {
        self.meters.clear();
    }
}

/// The engine's side of the channel processing: the settings of every channel, kept for the
/// whole engine run, the layout the stage runs, and the lines to it.
pub(crate) struct Processing {
    control: ProcessingControl,
    /// Per channel index (CH 1 first), every one of the mixer's channels.
    settings: [StripSettings; MAX_MIX_CHANNELS],
    layout: StripLayout,
}

impl Processing {
    /// Neutral settings everywhere and no strips yet, as a new stage has.
    pub(crate) fn new(control: ProcessingControl) -> Self {
        Self {
            control,
            settings: [StripSettings::default(); MAX_MIX_CHANNELS],
            layout: StripLayout::default(),
        }
    }

    /// Every channel's settings, one per mixer channel (CH 1 first), inputs the device lacks
    /// included.
    pub(crate) fn settings(&self) -> Vec<StripSettings> {
        self.settings.to_vec()
    }

    /// The settings of `channel` (0 is CH 1); `None` past the mixer's channels.
    pub(crate) fn channel(&self, channel: usize) -> Option<StripSettings> {
        self.settings.get(channel).copied()
    }

    /// The layout the stage was last sent.
    #[cfg(test)]
    pub(crate) fn layout(&self) -> StripLayout {
        self.layout
    }

    /// Sets the processing of `channel` (0 is CH 1) to `settings`, held in their ranges, and
    /// returns what is kept. For a linked pair both sides take them. The strip glides to them from
    /// the next block; a channel the device lacks keeps them for when it is back.
    pub(crate) fn set_channel(
        &mut self,
        channel: usize,
        settings: StripSettings,
    ) -> Result<StripSettings, String> {
        if channel >= MAX_MIX_CHANNELS {
            return Err(format!(
                "channel processing is supported for CH 1 to CH {MAX_MIX_CHANNELS}, not CH {}",
                channel + 1
            ));
        }
        let settings = settings.clamped();
        let (first, width) = self.layout.strip_of(channel).unwrap_or((channel, 1));
        if self.layout.width(first) > 0 {
            self.control.send(vec![ProcessingCommand::Design {
                first,
                design: settings.design(),
            }])?;
        }
        for side in first..first + width {
            self.settings[side] = settings;
        }
        Ok(settings)
    }

    /// Replaces every channel's settings at once (a session load): `table` from CH 1 on, the
    /// channels past it neutral. A pair takes its first side's. Returns the settings now kept.
    pub(crate) fn replace(
        &mut self,
        table: &[StripSettings],
    ) -> Result<Vec<StripSettings>, String> {
        if table.len() > MAX_MIX_CHANNELS {
            return Err(format!(
                "channel processing is supported for at most {MAX_MIX_CHANNELS} channels, \
                 received {}",
                table.len()
            ));
        }
        let mut settings: [StripSettings; MAX_MIX_CHANNELS] = std::array::from_fn(|channel| {
            table.get(channel).copied().unwrap_or_default().clamped()
        });
        pair_settings(&self.layout, &mut settings);
        let commands = self
            .layout
            .strips()
            .map(|(first, _)| ProcessingCommand::Design {
                first,
                design: settings[first].design(),
            })
            .collect();
        self.control.send(commands)?;
        self.settings = settings;
        Ok(self.settings())
    }

    /// Follows the source's channel details `channels` (one entry per input): a new channel count,
    /// or a pair linked or unlinked, sends the stage a strip set for the new layout, built here;
    /// the same layout sends nothing, and the strips keep running as they are.
    pub(crate) fn follow(&mut self, channels: &[ChannelMeta]) -> Result<(), String> {
        let layout = StripLayout::new(channels);
        if layout == self.layout {
            return Ok(());
        }
        let mut settings = self.settings;
        pair_settings(&layout, &mut settings);
        self.control
            .send(vec![ProcessingCommand::Strips(StripSet::new(
                layout, &settings,
            ))])?;
        self.settings = settings;
        self.layout = layout;
        Ok(())
    }
}

/// Gives the second side of every pair in `layout` its first side's settings.
fn pair_settings(layout: &StripLayout, settings: &mut [StripSettings; MAX_MIX_CHANNELS]) {
    for (first, width) in layout.strips() {
        if width == 2 {
            settings[first + 1] = settings[first];
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::dsp::dynamics::CompressorSettings;

    /// The details of `channels` inputs with the pairs starting at `pairs` linked.
    pub(crate) fn metas(channels: usize, pairs: &[usize]) -> Vec<ChannelMeta> {
        (0..channels)
            .map(|channel| ChannelMeta {
                linked_to_next: pairs.contains(&channel),
                ..ChannelMeta::default()
            })
            .collect()
    }

    /// A strip that clearly works, as the load bench runs on every input (see
    /// [crate::bench::typical_strip]).
    pub(crate) fn typical() -> StripSettings {
        crate::bench::typical_strip()
    }

    /// The high-pass filter alone, at `frequency_hz`.
    pub(crate) fn high_pass(frequency_hz: f32) -> StripSettings {
        let mut settings = StripSettings::default();
        settings.eq.high_pass.enabled = true;
        settings.eq.high_pass.frequency_hz = frequency_hz;
        settings
    }

    /// A stage and its engine side, laid out for `channels` and sized for `block_frames` blocks.
    fn stage(channels: &[ChannelMeta], block_frames: usize) -> (ProcessingStage, Processing) {
        let (mut stage, control) = processing_stage();
        let mut processing = Processing::new(control);
        processing.follow(channels).expect("the stage listens");
        stage.source_changed(SourceFormat {
            channels: channels.len(),
            block_frames,
        });
        (stage, processing)
    }

    /// `frames` frames of `channels` channels, each channel a tone of its own at `amplitude`.
    fn tones(channels: usize, frames: usize, amplitude: f32, from: usize) -> Vec<f32> {
        (from..from + frames)
            .flat_map(|frame| {
                (0..channels).map(move |channel| {
                    let frequency = 60.0 + 110.0 * channel as f32;
                    let phase = std::f32::consts::TAU * frequency * frame as f32 / 48_000.0;
                    amplitude * phase.sin()
                })
            })
            .collect()
    }

    fn channel(block: &[f32], channels: usize, channel: usize) -> Vec<f32> {
        block.chunks(channels).map(|frame| frame[channel]).collect()
    }

    #[test]
    fn layouts_follow_the_inputs_and_their_pairs() {
        let layout = StripLayout::new(&metas(5, &[1, 3]));
        assert_eq!(
            layout.strips().collect::<Vec<_>>(),
            [(0, 1), (1, 2), (3, 2)]
        );
        assert_eq!(layout.strip_of(2), Some((1, 2)));
        assert_eq!(layout.strip_of(4), Some((3, 2)));
        assert_eq!(layout.strip_of(5), None);
        // A pair on the last input has no second side: a mono strip.
        assert_eq!(
            StripLayout::new(&metas(2, &[1]))
                .strips()
                .collect::<Vec<_>>(),
            [(0, 1), (1, 1)]
        );
        assert_eq!(StripLayout::mono(40).strips().count(), MAX_MIX_CHANNELS);
    }

    /// Neutral strips, pairs included, are no work at all: the stage hands back nothing and the
    /// sinks get the raw block. A strip turned on and then off again glides out and is idle again.
    #[test]
    fn a_neutral_stage_does_nothing_and_hands_the_raw_block_on() {
        let (mut stage, mut processing) = stage(&metas(4, &[2]), 64);
        let block = tones(4, 64, 0.5, 0);
        assert!(stage.process(&block, 4).is_none());

        processing.set_channel(0, high_pass(500.0)).unwrap();
        assert!(stage.process(&block, 4).is_some());
        processing.set_channel(0, StripSettings::default()).unwrap();
        for _ in 0..20 {
            stage.process(&block, 4);
        }
        assert!(stage.process(&block, 4).is_none(), "glided out and idle");
    }

    /// A high-pass filter on one channel changes that channel and leaves every other channel of
    /// the block bit for bit.
    #[test]
    fn a_high_pass_on_one_channel_changes_only_that_channel() {
        let (mut stage, mut processing) = stage(&metas(4, &[]), 256);
        processing.set_channel(2, high_pass(400.0)).unwrap();
        let mut last = Vec::new();
        for index in 0..40 {
            let raw = tones(4, 256, 0.5, index * 256);
            let processed = stage.process(&raw, 4).expect("a strip works").to_vec();
            for other in [0, 1, 3] {
                assert_eq!(
                    channel(&processed, 4, other)
                        .iter()
                        .map(|s| s.to_bits())
                        .collect::<Vec<_>>(),
                    channel(&raw, 4, other)
                        .iter()
                        .map(|s| s.to_bits())
                        .collect::<Vec<_>>(),
                    "channel {other}"
                );
            }
            last = processed;
        }
        // Channel 2 is a 280 Hz tone: 400 Hz filters cut it by about 4 dB.
        let peak = channel(&last, 4, 2)
            .iter()
            .fold(0.0f32, |peak, s| peak.max(s.abs()));
        assert!(peak < 0.4, "{peak}");
    }

    /// A linked pair is one strip with one detector: the loud left side drives the compressor,
    /// and the quiet right side, far under the threshold on its own, is turned down with it.
    #[test]
    fn a_linked_pair_shares_one_detector() {
        let compressor = StripSettings {
            compressor: CompressorSettings {
                enabled: true,
                threshold_db: -20.0,
                ratio: 4.0,
                knee_db: 0.0,
                attack_ms: 1.0,
                release_ms: 100.0,
                makeup_db: 0.0,
            },
            ..StripSettings::default()
        };
        for (pairs, shared) in [(vec![0], true), (vec![], false)] {
            let (mut stage, mut processing) = stage(&metas(2, &pairs), 256);
            processing.set_channel(0, compressor).unwrap();
            processing.set_channel(1, compressor).unwrap();
            let mut right = (Vec::new(), Vec::new());
            for index in 0..40 {
                let raw: Vec<f32> = tones(1, 256, 0.9, index * 256)
                    .into_iter()
                    .zip(tones(1, 256, 0.01, index * 256))
                    .flat_map(|(left, right)| [left, right])
                    .collect();
                let processed = stage.process(&raw, 2).expect("strips work");
                right = (channel(&raw, 2, 1), channel(processed, 2, 1));
            }
            let peak = |side: &[f32]| side.iter().fold(0.0f32, |peak, s| peak.max(s.abs()));
            let reading = stage.meters().read(2, &LevelMeter::new());
            if shared {
                assert!(
                    peak(&right.1) < 0.3 * peak(&right.0),
                    "the quiet side follows the loud one: {right:?}"
                );
                assert_eq!(
                    reading.compressor_reduction_db[0],
                    reading.compressor_reduction_db[1]
                );
                assert!(reading.compressor_reduction_db[1] > 6.0, "{reading:?}");
            } else {
                assert_eq!(right.1, right.0, "on its own the quiet side is untouched");
                assert_eq!(reading.compressor_reduction_db[1], 0.0);
            }
        }
    }

    /// The meters read each channel after its strip, the gate's and the compressor's reduction,
    /// and the input level for a channel whose strip is idle.
    #[test]
    fn the_stage_meters_each_channel_after_its_strip() {
        let (mut stage, mut processing) = stage(&metas(3, &[]), 256);
        let mut gate = StripSettings::default();
        gate.gate.enabled = true;
        gate.gate.threshold_db = -6.0;
        gate.gate.range_db = 40.0;
        processing.set_channel(0, gate).unwrap();
        processing.set_channel(1, typical()).unwrap();
        let inputs = LevelMeter::new();
        // A second: the gate's level from before it closed has fallen away since.
        for index in 0..200 {
            let raw = tones(3, 256, 0.25, index * 256);
            inputs.observe(&raw, 3);
            stage.process(&raw, 3);
        }
        let reading = stage.meters().read(3, &inputs);
        let input = inputs.read(3);
        assert!(reading.gate_reduction_db[0] > 39.0, "{reading:?}");
        assert!(reading.levels[0] < 2, "a closed gate: {reading:?}");
        assert!(reading.compressor_reduction_db[1] > 1.0, "{reading:?}");
        assert!(reading.levels[1] < input[1], "{reading:?} {input:?}");
        assert_eq!(reading.levels[2], input[2], "an idle strip reads its input");
        assert_eq!(
            (
                reading.gate_reduction_db[2],
                reading.compressor_reduction_db[2]
            ),
            (0.0, 0.0)
        );
    }

    /// The same layout sends nothing and keeps every strip running; a new one is a new set, in
    /// which the strips on the same channels carry on and the others start from their settings.
    /// The settings stay with their channels through all of it.
    #[test]
    fn a_new_layout_keeps_the_settings_and_the_running_strips() {
        let (mut stage, mut processing) = stage(&metas(2, &[]), 256);
        processing.set_channel(0, typical()).unwrap();
        processing.set_channel(1, high_pass(300.0)).unwrap();
        for index in 0..10 {
            stage.process(&tones(2, 256, 0.25, index * 256), 2);
        }
        processing.follow(&metas(2, &[])).unwrap();
        assert_eq!(processing.control.commands.slots(), COMMAND_SLOTS);

        // Three inputs: the first two strips carry over, the third is new and neutral.
        processing.follow(&metas(3, &[])).unwrap();
        stage.source_changed(SourceFormat {
            channels: 3,
            block_frames: 256,
        });
        stage.process(&tones(3, 256, 0.25, 0), 3);
        assert!(
            stage
                .meters()
                .read(3, &LevelMeter::new())
                .compressor_reduction_db[0]
                > 1.0
        );
        assert_eq!(processing.channel(0), Some(typical().clamped()));
        assert_eq!(processing.channel(1), Some(high_pass(300.0).clamped()));
        assert_eq!(processing.channel(2), Some(StripSettings::default()));

        // Linking 2-3 gives the pair the first side's settings; the old set comes back to be freed.
        processing.follow(&metas(3, &[1])).unwrap();
        assert_eq!(processing.channel(2), processing.channel(1));
        stage.process(&tones(3, 256, 0.25, 256), 3);
        processing.control.reap();
        assert_eq!(processing.control.returns.slots(), 0);
        assert_eq!(
            processing.layout().strips().collect::<Vec<_>>(),
            [(0, 1), (1, 2)]
        );

        // A device with one input: the strip set shrinks, the settings of the others are kept.
        processing.follow(&metas(1, &[])).unwrap();
        assert_eq!(processing.layout().strips().count(), 1);
        assert_eq!(processing.channel(1), Some(high_pass(300.0).clamped()));
        assert!(processing.set_channel(MAX_MIX_CHANNELS, typical()).is_err());
    }

    /// A session load replaces every channel at once; a pair takes its first side's settings.
    #[test]
    fn replacing_every_channel_at_once_follows_the_pairs() {
        let (mut stage, mut processing) = stage(&metas(4, &[2]), 64);
        let table = processing
            .replace(&[typical(), StripSettings::default(), high_pass(200.0)])
            .unwrap();
        assert_eq!(table.len(), MAX_MIX_CHANNELS);
        assert_eq!(table[0], typical().clamped());
        assert_eq!(
            table[3],
            high_pass(200.0).clamped(),
            "the pair's second side"
        );
        assert!(table[4..].iter().all(|settings| settings.is_neutral()));
        assert!(stage.process(&tones(4, 64, 0.5, 0), 4).is_some());
        assert!(processing
            .replace(&vec![StripSettings::default(); MAX_MIX_CHANNELS + 1])
            .is_err());
    }

    /// The engine thread's whole path, from the capture callback through the stage to a sink, with
    /// all 32 strips working (three of them linked pairs), allocates and frees nothing, while the
    /// engine sends new designs and new layouts between the blocks.
    #[test]
    fn the_engine_thread_never_allocates_with_every_strip_working_and_settings_changing() {
        use crate::pipeline::tests::{allocations, links, source, Recorder};
        use crate::pipeline::Pipeline;
        use std::time::Instant;

        let links = links();
        let (mut writer, reader) = source(32, 64, links.clone());
        let (stage, control) = processing_stage();
        let mut pipeline = Pipeline::with_processing(
            vec![Box::new(Recorder::new())],
            Arc::clone(&links.levels),
            stage,
        );
        pipeline.set_source(reader);
        let mut processing = Processing::new(control);
        processing.follow(&metas(32, &[0, 2, 4])).unwrap();
        processing.replace(&[typical(); MAX_MIX_CHANNELS]).unwrap();
        let callback = tones(32, 64, 0.3, 0);
        writer.write(&callback);
        pipeline.pump();
        let reading = pipeline.processing_meters().read(32, &links.levels);
        assert!(
            reading.compressor_reduction_db.iter().all(|db| *db > 0.0),
            "every strip works: {reading:?}"
        );

        let mut counted = (0, 0);
        for round in 0..200 {
            // The engine's side, not watched: a new design, or every tenth round a new layout.
            if round % 10 == 5 {
                let pairs: &[usize] = if round % 20 == 5 { &[0, 2] } else { &[0, 2, 4] };
                processing.follow(&metas(32, pairs)).unwrap();
            } else {
                let settings = if round % 2 == 0 {
                    typical()
                } else {
                    high_pass(150.0)
                };
                processing.set_channel(round % 32, settings).unwrap();
            }
            let (allocations, frees) = allocations::count(|| {
                writer.write(&callback);
                pipeline.pump();
                pipeline.tick(Instant::now());
            });
            counted = (counted.0 + allocations, counted.1 + frees);
        }
        assert_eq!(counted, (0, 0));
    }

    /// NaN reaching a working strip is counted on the meters, and the block goes on as numbers.
    #[test]
    fn non_finite_samples_are_counted_and_go_on_as_silence() {
        let (mut stage, mut processing) = stage(&metas(2, &[]), 64);
        processing.set_channel(1, typical()).unwrap();
        let mut raw = tones(2, 64, 0.5, 0);
        raw[1] = f32::NAN;
        raw[3] = f32::INFINITY;
        // Channel 0 has no working strip: its NaN is passed on as it came.
        raw[0] = f32::NAN;
        let processed = stage.process(&raw, 2).expect("a strip works");
        assert!(channel(processed, 2, 1).iter().all(|s| s.is_finite()));
        assert!(processed[0].is_nan());
        assert_eq!(stage.meters().non_finite_samples(), 2);
    }
}
