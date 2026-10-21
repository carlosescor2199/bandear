//! The shared reverb: one stereo algorithmic reverb fed by a mono send bus, returned to every mix
//! that asks for it (see [crate::effects]).
//!
//! **Network.** A pre-delay line, then eight parallel feedback comb filters per side, each with a
//! one-pole low-pass in its loop (the damping: high frequencies die first, as in a room), summed and
//! passed through four series all-pass filters that thicken the echoes into a smooth tail. The
//! right side's lines are a few samples longer than the left's, so the two sides decorrelate into a
//! wide stereo image; `width` mixes them back towards mono. This is the classic Schroeder-Moorer
//! design in the arrangement Jezar's public-domain "Freeverb" description made popular; the line
//! lengths are the ones that description gives at 44.1 kHz, scaled to the engine's 48 kHz.
//!
//! **Settings.** `size` (the comb feedback, so the decay time), `damping` (the loop's low-pass),
//! `preDelayMs` (0 to [MAX_PRE_DELAY_MS] before the first reflection) and `width`. Plain serde data
//! that persists with a session; [ReverbSettings::design] works the numbers out off the engine
//! thread.
//!
//! **Real time.** Every line is allocated in [Reverb::new] at its full size; processing indexes
//! into them and never allocates. Feedback, damping and width glide to new values over
//! [GLIDE_FRAMES]; a new pre-delay crossfades from the old tap to the new one over
//! [CROSSFADE_FRAMES], so no change clicks.
//!
//! **Silence.** Every value written into a line or a filter's state below [SILENCE_FLOOR] (about
//! -200 dBFS) is written as zero, so a dying tail ends in exact zeros instead of sinking into
//! subnormal numbers. Once the input has been silent and nothing but zeros was written for longer
//! than the longest line, every line holds zeros: the reverb is [idle](Reverb::is_idle) and its
//! output is exact silence, and the stage stops running it.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::{clamp_to, Glide, GLIDE_FRAMES, SAMPLE_RATE};

/// The comb filters' lengths at 44.1 kHz, the published Freeverb tunings.
const COMB_TUNINGS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
/// The all-pass filters' lengths at 44.1 kHz.
const ALLPASS_TUNINGS: [usize; 4] = [556, 441, 341, 225];
/// How much longer the right side's lines are, at 44.1 kHz.
const STEREO_SPREAD: usize = 23;
/// The all-pass filters' fixed feedback.
const ALLPASS_FEEDBACK: f32 = 0.5;
/// The gain into the combs: eight combs with feedback near 1 add up, so the input is scaled down.
const INPUT_GAIN: f32 = 0.015;
/// The gain out of the network, so a full send returns at about the level that went in.
const OUTPUT_GAIN: f32 = 3.0;

/// The longest pre-delay, in milliseconds.
pub const MAX_PRE_DELAY_MS: f32 = 200.0;
/// How long a new pre-delay crossfades from the old tap: 20 ms at 48 kHz.
pub(crate) const CROSSFADE_FRAMES: u32 = 960;
/// Values written into the lines below this magnitude (about -200 dBFS) are written as zero.
pub(crate) const SILENCE_FLOOR: f32 = 1e-10;

/// The ranges [ReverbSettings::clamped] holds the settings to.
pub const REVERB_SIZE_RANGE: RangeInclusive<f32> = 0.0..=100.0;
pub const REVERB_DAMPING_RANGE: RangeInclusive<f32> = 0.0..=100.0;
pub const REVERB_PRE_DELAY_RANGE: RangeInclusive<f32> = 0.0..=MAX_PRE_DELAY_MS;
pub const REVERB_WIDTH_RANGE: RangeInclusive<f32> = 0.0..=100.0;

/// `samples` at 44.1 kHz in samples at the engine's rate.
fn scaled(samples: usize) -> usize {
    ((samples as f64) * SAMPLE_RATE / 44_100.0).round() as usize
}

/// `value`, or zero when it is too small to hear (see [SILENCE_FLOOR]).
#[inline]
fn quiet(value: f32) -> f32 {
    if value.abs() < SILENCE_FLOOR {
        0.0
    } else {
        value
    }
}

/// The reverb's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReverbSettings {
    /// The room's size, 0-100%: how long the tail lasts.
    pub size_percent: f32,
    /// How fast the high frequencies die, 0-100%.
    pub damping_percent: f32,
    /// The time before the first reflection, 0-200 ms.
    pub pre_delay_ms: f32,
    /// The stereo width, 0% (mono) to 100%.
    pub width_percent: f32,
}

impl Default for ReverbSettings {
    fn default() -> Self {
        Self {
            size_percent: 50.0,
            damping_percent: 40.0,
            pre_delay_ms: 10.0,
            width_percent: 100.0,
        }
    }
}

impl ReverbSettings {
    /// These settings with every number held in its range (a NaN takes the default).
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        Self {
            size_percent: clamp_to(self.size_percent, &REVERB_SIZE_RANGE, default.size_percent),
            damping_percent: clamp_to(
                self.damping_percent,
                &REVERB_DAMPING_RANGE,
                default.damping_percent,
            ),
            pre_delay_ms: clamp_to(
                self.pre_delay_ms,
                &REVERB_PRE_DELAY_RANGE,
                default.pre_delay_ms,
            ),
            width_percent: clamp_to(
                self.width_percent,
                &REVERB_WIDTH_RANGE,
                default.width_percent,
            ),
        }
    }

    /// The reverb worked out for the engine thread.
    pub(crate) fn design(&self) -> ReverbDesign {
        let held = self.clamped();
        let size = held.size_percent / 100.0;
        let width = held.width_percent / 100.0;
        ReverbDesign {
            // Freeverb's room scale: feedback 0.7 for the smallest room, 0.98 for the largest.
            feedback: 0.7 + 0.28 * size,
            damping: 0.4 * held.damping_percent / 100.0,
            pre_delay_frames: (f64::from(held.pre_delay_ms) * 1e-3 * SAMPLE_RATE).round() as u32,
            direct: 0.5 + width / 2.0,
            cross: (1.0 - width) / 2.0,
        }
    }
}

/// A reverb's settings worked out for the engine thread: plain numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ReverbDesign {
    /// Each comb's feedback.
    feedback: f32,
    /// The comb loops' low-pass coefficient (0: no damping).
    damping: f32,
    pre_delay_frames: u32,
    /// What each side takes of its own network and of the other side's (the width).
    direct: f32,
    cross: f32,
}

impl Default for ReverbDesign {
    fn default() -> Self {
        ReverbSettings::default().design()
    }
}

/// A feedback comb filter with a one-pole low-pass in its loop.
struct Comb {
    line: Vec<f32>,
    index: usize,
    /// The low-pass filter's state.
    store: f32,
}

impl Comb {
    fn new(length: usize) -> Self {
        Self {
            line: vec![0.0; length.max(1)],
            index: 0,
            store: 0.0,
        }
    }

    /// One sample through the comb; true in the second value when it wrote anything but zero.
    #[inline]
    fn run(&mut self, input: f32, feedback: f32, damping: f32) -> (f32, bool) {
        let output = self.line[self.index];
        self.store = quiet(output * (1.0 - damping) + self.store * damping);
        let written = quiet(input + self.store * feedback);
        self.line[self.index] = written;
        self.index += 1;
        if self.index == self.line.len() {
            self.index = 0;
        }
        (output, written != 0.0 || self.store != 0.0)
    }

    fn clear(&mut self) {
        self.line.fill(0.0);
        self.store = 0.0;
    }
}

/// A Schroeder all-pass filter.
struct AllPass {
    line: Vec<f32>,
    index: usize,
}

impl AllPass {
    fn new(length: usize) -> Self {
        Self {
            line: vec![0.0; length.max(1)],
            index: 0,
        }
    }

    #[inline]
    fn run(&mut self, input: f32) -> (f32, bool) {
        let delayed = self.line[self.index];
        let output = delayed - input;
        let written = quiet(input + delayed * ALLPASS_FEEDBACK);
        self.line[self.index] = written;
        self.index += 1;
        if self.index == self.line.len() {
            self.index = 0;
        }
        (output, written != 0.0)
    }

    fn clear(&mut self) {
        self.line.fill(0.0);
    }
}

/// One side's network: eight combs in parallel, then four all-passes in series.
struct Side {
    combs: Vec<Comb>,
    allpasses: Vec<AllPass>,
}

impl Side {
    fn new(spread: usize) -> Self {
        Self {
            combs: COMB_TUNINGS
                .iter()
                .map(|&length| Comb::new(scaled(length + spread)))
                .collect(),
            allpasses: ALLPASS_TUNINGS
                .iter()
                .map(|&length| AllPass::new(scaled(length + spread)))
                .collect(),
        }
    }

    #[inline]
    fn run(&mut self, input: f32, feedback: f32, damping: f32) -> (f32, bool) {
        let mut sum = 0.0;
        let mut wrote = false;
        for comb in &mut self.combs {
            let (output, written) = comb.run(input, feedback, damping);
            sum += output;
            wrote |= written;
        }
        for allpass in &mut self.allpasses {
            let (output, written) = allpass.run(sum);
            sum = output;
            wrote |= written;
        }
        (sum, wrote)
    }

    /// The length of all its lines together: longer than any value can stay in them.
    fn span(&self) -> usize {
        self.combs
            .iter()
            .map(|comb| comb.line.len())
            .chain(self.allpasses.iter().map(|allpass| allpass.line.len()))
            .sum()
    }

    fn clear(&mut self) {
        self.combs.iter_mut().for_each(Comb::clear);
        self.allpasses.iter_mut().for_each(AllPass::clear);
    }

    fn floats(&self) -> usize {
        self.combs.iter().map(|comb| comb.line.len()).sum::<usize>()
            + self
                .allpasses
                .iter()
                .map(|allpass| allpass.line.len())
                .sum::<usize>()
    }
}

/// The running reverb.
pub(crate) struct Reverb {
    pre_delay: Vec<f32>,
    write: usize,
    /// The pre-delay tap in use, and the one it crossfades from while `fade` runs.
    tap: u32,
    previous_tap: u32,
    fade: u32,
    left: Side,
    right: Side,
    feedback: Glide,
    damping: Glide,
    direct: Glide,
    cross: Glide,
    /// Frames in a row where the input was silent and nothing but zeros was written.
    quiet_frames: usize,
    /// How many such frames leave every line holding zeros.
    settle_frames: usize,
    idle: bool,
}

impl Reverb {
    /// A reverb with `design`, silent and idle, every line allocated at its full size. Allocates:
    /// call it off the engine thread.
    pub(crate) fn new(design: &ReverbDesign) -> Self {
        let pre_delay = vec![0.0; (f64::from(MAX_PRE_DELAY_MS) * 1e-3 * SAMPLE_RATE) as usize + 1];
        let left = Side::new(0);
        let right = Side::new(STEREO_SPREAD);
        // A value can travel through the pre-delay and every line of a side in turn before it
        // is gone: the sum of their lengths is a safe bound.
        let settle_frames = pre_delay.len() + left.span().max(right.span());
        Self {
            pre_delay,
            write: 0,
            tap: design.pre_delay_frames,
            previous_tap: design.pre_delay_frames,
            fade: 0,
            left,
            right,
            feedback: Glide::new(design.feedback),
            damping: Glide::new(design.damping),
            direct: Glide::new(design.direct),
            cross: Glide::new(design.cross),
            quiet_frames: 0,
            settle_frames,
            idle: true,
        }
    }

    /// The memory its lines take, in floats.
    pub(crate) fn floats(&self) -> usize {
        self.pre_delay.len() + self.left.floats() + self.right.floats()
    }

    /// Takes `design`: the numbers glide there and a new pre-delay crossfades in.
    pub(crate) fn apply(&mut self, design: &ReverbDesign) {
        self.feedback.set(design.feedback, GLIDE_FRAMES);
        self.damping.set(design.damping, GLIDE_FRAMES);
        self.direct.set(design.direct, GLIDE_FRAMES);
        self.cross.set(design.cross, GLIDE_FRAMES);
        let tap = design.pre_delay_frames.min(self.pre_delay.len() as u32 - 1);
        if tap != self.tap {
            self.previous_tap = self.current_tap();
            self.tap = tap;
            self.fade = CROSSFADE_FRAMES;
        }
    }

    /// The tap the output is closest to now (the new one once a crossfade passed halfway).
    fn current_tap(&self) -> u32 {
        if self.fade > CROSSFADE_FRAMES / 2 {
            self.previous_tap
        } else {
            self.tap
        }
    }

    /// True while the reverb holds nothing but zeros: its output is silence until a sample comes.
    pub(crate) fn is_idle(&self) -> bool {
        self.idle
    }

    fn read(&self, delay: u32) -> f32 {
        let length = self.pre_delay.len();
        self.pre_delay[(self.write + length - delay as usize) % length]
    }

    /// Runs the mono `input` through the reverb into `output`, stereo interleaved, as many frames
    /// as both hold; `output` is overwritten with the wet signal. Allocation-free.
    pub(crate) fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (&sample, frame) in input.iter().zip(output.chunks_exact_mut(2)) {
            let sample = if sample.is_finite() { sample } else { 0.0 };
            self.pre_delay[self.write] = quiet(sample);
            let delayed = if self.fade > 0 {
                let way = 1.0 - self.fade as f32 / CROSSFADE_FRAMES as f32;
                self.fade -= 1;
                self.read(self.previous_tap) * (1.0 - way) + self.read(self.tap) * way
            } else {
                self.read(self.tap)
            };
            self.write += 1;
            if self.write == self.pre_delay.len() {
                self.write = 0;
            }
            let (feedback, damping) = (self.feedback.next(), self.damping.next());
            let (direct, cross) = (self.direct.next(), self.cross.next());
            let fed = delayed * INPUT_GAIN;
            let (left, wrote_left) = self.left.run(fed, feedback, damping);
            let (right, wrote_right) = self.right.run(fed, feedback, damping);
            frame[0] = OUTPUT_GAIN * (left * direct + right * cross);
            frame[1] = OUTPUT_GAIN * (right * direct + left * cross);
            if sample == 0.0 && !wrote_left && !wrote_right {
                self.quiet_frames = self.quiet_frames.saturating_add(1);
            } else {
                self.quiet_frames = 0;
                self.idle = false;
            }
        }
        if !self.idle && self.quiet_frames >= self.settle_frames {
            // Every line holds zeros by now; clearing makes sure of it.
            self.left.clear();
            self.right.clear();
            self.pre_delay.fill(0.0);
            self.idle = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::{rms, sine};

    fn reverb(settings: ReverbSettings) -> Reverb {
        Reverb::new(&settings.design())
    }

    /// The reverb's stereo output for `input`, in blocks of 256 frames as the stage runs it.
    fn run(reverb: &mut Reverb, input: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; input.len() * 2];
        for (input, output) in input.chunks(256).zip(output.chunks_mut(512)) {
            reverb.process(input, output);
        }
        output
    }

    fn impulse(frames: usize) -> Vec<f32> {
        let mut input = vec![0.0; frames];
        input[0] = 1.0;
        input
    }

    #[test]
    fn an_impulse_rings_out_and_decays_with_no_nan_or_subnormal() {
        let mut reverb = reverb(ReverbSettings::default());
        let output = run(&mut reverb, &impulse(48_000 * 4));
        assert!(output
            .iter()
            .all(|sample| sample.is_finite() && (*sample == 0.0 || sample.is_normal())));
        let second = |n: usize| rms(&output[n * 96_000..(n + 1) * 96_000]);
        assert!(second(0) > 1e-3, "it rings: {}", second(0));
        assert!(
            second(1) < second(0) / 4.0,
            "{} then {}",
            second(0),
            second(1)
        );
        assert!(
            second(3) < second(1) / 4.0,
            "{} then {}",
            second(1),
            second(3)
        );
        // The pre-delay: nothing before 10 ms.
        assert!(output[..2 * 470].iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn a_larger_room_rings_longer_and_damping_darkens_it() {
        let tail = |settings: ReverbSettings| {
            let mut reverb = reverb(settings);
            let output = run(&mut reverb, &impulse(48_000 * 2));
            rms(&output[96_000..])
        };
        let small = tail(ReverbSettings {
            size_percent: 10.0,
            ..ReverbSettings::default()
        });
        let large = tail(ReverbSettings {
            size_percent: 90.0,
            ..ReverbSettings::default()
        });
        assert!(large > small * 4.0, "small {small}, large {large}");

        // The high frequencies' share of the tail falls with damping: compare a first difference
        // (a crude high-pass) with the tail itself.
        let brightness = |damping_percent: f32| {
            let mut reverb = reverb(ReverbSettings {
                damping_percent,
                ..ReverbSettings::default()
            });
            let output = run(&mut reverb, &impulse(48_000));
            let left: Vec<f32> = output[24_000..].iter().step_by(2).copied().collect();
            let differences: Vec<f32> = left.windows(2).map(|pair| pair[1] - pair[0]).collect();
            rms(&differences) / rms(&left)
        };
        assert!(brightness(90.0) < brightness(0.0) * 0.8);
    }

    #[test]
    fn width_moves_the_sides_from_apart_to_the_same() {
        let sides = |width_percent: f32| {
            let mut reverb = reverb(ReverbSettings {
                width_percent,
                ..ReverbSettings::default()
            });
            let output = run(&mut reverb, &impulse(24_000));
            let (left, right): (Vec<f32>, Vec<f32>) = output
                .chunks_exact(2)
                .map(|frame| (frame[0], frame[1]))
                .unzip();
            let difference: Vec<f32> = left.iter().zip(&right).map(|(l, r)| l - r).collect();
            rms(&difference) / rms(&left)
        };
        assert!(sides(100.0) > 0.5, "wide: {}", sides(100.0));
        assert!(sides(0.0) < 1e-6, "mono: {}", sides(0.0));
    }

    #[test]
    fn silence_in_gives_exact_silence_once_the_tail_died_and_the_reverb_goes_idle() {
        let mut reverb = reverb(ReverbSettings {
            size_percent: 100.0,
            damping_percent: 0.0,
            ..ReverbSettings::default()
        });
        assert!(reverb.is_idle(), "a new reverb holds nothing");
        let mut input = sine(440.0, 0.8, 4_800);
        input.resize(48_000 * 60, 0.0);
        let output = run(&mut reverb, &input);
        assert!(!output[..9_600].iter().all(|s| *s == 0.0));
        assert!(reverb.is_idle(), "the tail died within a minute");
        let last = &output[output.len() - 96_000..];
        assert!(
            last.iter().all(|sample| sample.to_bits() == 0),
            "exact zeros"
        );
        // Idle, it gives zeros and stays idle.
        let more = run(&mut reverb, &vec![0.0; 1_024]);
        assert!(more.iter().all(|sample| sample.to_bits() == 0));
        assert!(reverb.is_idle());
        // A sample wakes it.
        let mut woken = vec![0.0; 1_024];
        woken[0] = 0.5;
        run(&mut reverb, &woken);
        assert!(!reverb.is_idle());
    }

    #[test]
    fn changing_a_setting_mid_tail_does_not_click() {
        // A steady tone through the reverb; the size, damping, width and pre-delay all change at
        // once. No frame may jump more than the tone's own steepest frame-to-frame step allows.
        let tone = sine(220.0, 0.5, 48_000 * 2);
        let steepest = |output: &[f32]| {
            output
                .chunks_exact(2)
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| (pair[1][0] - pair[0][0]).abs())
                .fold(0.0f32, f32::max)
        };
        let mut steady = reverb(ReverbSettings::default());
        let reference = run(&mut steady, &tone);
        let mut changed = reverb(ReverbSettings::default());
        let mut output = run(&mut changed, &tone[..48_000]);
        changed.apply(
            &ReverbSettings {
                size_percent: 95.0,
                damping_percent: 80.0,
                pre_delay_ms: 150.0,
                width_percent: 20.0,
            }
            .design(),
        );
        output.extend(run(&mut changed, &tone[48_000..]));
        // Against the steepest step of the steady reverb before and of the changed one after.
        let bound =
            3.0 * steepest(&reference[2 * 20_000..2 * 47_000]).max(steepest(&output[2 * 60_000..]));
        let around = &output[2 * 47_000..2 * 52_000];
        assert!(
            steepest(around) <= bound,
            "a jump of {} against {}",
            steepest(around),
            bound
        );
    }

    #[test]
    fn settings_hold_their_ranges_and_round_trip_through_json() {
        let wild = ReverbSettings {
            size_percent: 400.0,
            damping_percent: f32::NAN,
            pre_delay_ms: -5.0,
            width_percent: 50.0,
        };
        assert_eq!(
            wild.clamped(),
            ReverbSettings {
                size_percent: 100.0,
                damping_percent: 40.0,
                pre_delay_ms: 0.0,
                width_percent: 50.0,
            }
        );
        let json = serde_json::to_string(&ReverbSettings::default()).unwrap();
        assert!(json.contains("\"preDelayMs\""), "{json}");
        let partial: ReverbSettings = serde_json::from_str(r#"{"sizePercent":80}"#).unwrap();
        assert_eq!(partial.size_percent, 80.0);
        assert_eq!(partial.width_percent, 100.0);
    }

    #[test]
    fn running_and_applying_never_allocate_and_the_lines_are_sized_once() {
        let mut reverb = reverb(ReverbSettings::default());
        // About 37 thousand floats (150 KB): the pre-delay and both sides' lines.
        assert!(
            (30_000..45_000).contains(&reverb.floats()),
            "{}",
            reverb.floats()
        );
        let input = sine(330.0, 0.5, 256);
        let mut output = vec![0.0; 512];
        let designs = [
            ReverbSettings::default().design(),
            ReverbSettings {
                pre_delay_ms: 120.0,
                ..ReverbSettings::default()
            }
            .design(),
        ];
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for design in &designs {
                reverb.apply(design);
                for _ in 0..8 {
                    reverb.process(&input, &mut output);
                }
            }
        });
        assert_eq!((allocations, frees), (0, 0));
    }
}
