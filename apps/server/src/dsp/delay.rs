//! The shared delay: one stereo echo fed by a mono send bus, returned to every mix that asks for it
//! (see [crate::effects]).
//!
//! **Network.** Two delay lines, left and right, read [DelayDesign] frames behind where they are
//! written. What comes out goes back in, scaled by the feedback and through a one-pole low-pass
//! (the high cut: each repeat is darker than the last, as tape and analogue echoes are). Straight,
//! each side feeds itself and both repeat together in the middle; ping-pong, the input goes into
//! the left line only and each side feeds the other, so the repeats bounce left, right, left.
//!
//! **Settings.** The time, 1 to [MAX_DELAY_MS] ms, or a note value at a tempo (a quarter, a dotted
//! eighth...; [DelaySettings::time_ms_in_force]); the feedback (0-90%), the high cut and
//! ping-pong. Plain serde data that persists with a session; [DelaySettings::design] works the
//! numbers out off the engine thread.
//!
//! **Real time.** Both lines are allocated in [Delay::new] for the longest time; processing never
//! allocates. The feedback and the high cut glide over [GLIDE_FRAMES]; a new time crossfades from
//! the old read position to the new one over [CROSSFADE_FRAMES](super::reverb::CROSSFADE_FRAMES)
//! (20 ms), so a change neither clicks nor sweeps the pitch; a time that comes during a crossfade
//! follows once it ends ([MovingTap]), so a time dragged quickly never clicks either.
//!
//! **Silence.** As the reverb's: values below [SILENCE_FLOOR] are written as zero, and once the
//! lines hold nothing else the delay is [idle](Delay::is_idle), its output exact silence.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use super::reverb::{MovingTap, Taps, SILENCE_FLOOR};
use super::{clamp_to, Glide, GLIDE_FRAMES, SAMPLE_RATE};

/// The longest delay time, in milliseconds.
pub const MAX_DELAY_MS: f32 = 2_000.0;

/// The ranges [DelaySettings::clamped] holds the settings to.
pub const DELAY_TIME_RANGE: RangeInclusive<f32> = 1.0..=MAX_DELAY_MS;
/// A tempo of 0 means none: the time is in milliseconds.
pub const DELAY_TEMPO_RANGE: RangeInclusive<f32> = 30.0..=300.0;
pub const DELAY_FEEDBACK_RANGE: RangeInclusive<f32> = 0.0..=90.0;
pub const DELAY_HIGH_CUT_RANGE: RangeInclusive<f32> = 1_000.0..=20_000.0;

/// The frames the lines hold: the longest time and one more.
fn line_frames() -> usize {
    (f64::from(MAX_DELAY_MS) * 1e-3 * SAMPLE_RATE) as usize + 1
}

/// `value`, or zero when it is too small to hear.
#[inline]
fn quiet(value: f32) -> f32 {
    if value.abs() < SILENCE_FLOOR {
        0.0
    } else {
        value
    }
}

/// A note value the delay time follows at a tempo.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DelayNote {
    Half,
    #[default]
    Quarter,
    DottedEighth,
    QuarterTriplet,
    Eighth,
    EighthTriplet,
    Sixteenth,
}

impl DelayNote {
    /// The note's length in quarter notes (beats).
    pub fn beats(self) -> f32 {
        match self {
            Self::Half => 2.0,
            Self::Quarter => 1.0,
            Self::DottedEighth => 0.75,
            Self::QuarterTriplet => 2.0 / 3.0,
            Self::Eighth => 0.5,
            Self::EighthTriplet => 1.0 / 3.0,
            Self::Sixteenth => 0.25,
        }
    }
}

/// The delay's settings.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DelaySettings {
    /// The time between repeats, 1-2,000 ms, while no tempo is set.
    pub time_ms: f32,
    /// A tempo in beats per minute (30-300) the time follows on `note`; 0 for none.
    pub tempo_bpm: f32,
    pub note: DelayNote,
    /// How much of each repeat comes back, 0-90%.
    pub feedback_percent: f32,
    /// The high cut on the repeats, 1-20 kHz.
    pub high_cut_hz: f32,
    /// Repeats bounce between the sides instead of sitting in the middle.
    pub ping_pong: bool,
}

impl Default for DelaySettings {
    fn default() -> Self {
        Self {
            time_ms: 375.0,
            tempo_bpm: 0.0,
            note: DelayNote::Quarter,
            feedback_percent: 35.0,
            high_cut_hz: 6_000.0,
            ping_pong: false,
        }
    }
}

impl DelaySettings {
    /// These settings with every number held in its range (a NaN takes the default; a tempo
    /// under the range is none).
    pub fn clamped(&self) -> Self {
        let default = Self::default();
        let tempo_bpm = if self.tempo_bpm.is_nan() || self.tempo_bpm < *DELAY_TEMPO_RANGE.start() {
            0.0
        } else {
            self.tempo_bpm.min(*DELAY_TEMPO_RANGE.end())
        };
        Self {
            time_ms: clamp_to(self.time_ms, &DELAY_TIME_RANGE, default.time_ms),
            tempo_bpm,
            note: self.note,
            feedback_percent: clamp_to(
                self.feedback_percent,
                &DELAY_FEEDBACK_RANGE,
                default.feedback_percent,
            ),
            high_cut_hz: clamp_to(self.high_cut_hz, &DELAY_HIGH_CUT_RANGE, default.high_cut_hz),
            ping_pong: self.ping_pong,
        }
    }

    /// The time the delay repeats at: the note at the tempo when one is set, else `time_ms`; held
    /// to 1-2,000 ms (a half note at 30 bpm would be 4 s).
    pub fn time_ms_in_force(&self) -> f32 {
        let held = self.clamped();
        let time = if held.tempo_bpm > 0.0 {
            60_000.0 / held.tempo_bpm * held.note.beats()
        } else {
            held.time_ms
        };
        time.clamp(*DELAY_TIME_RANGE.start(), *DELAY_TIME_RANGE.end())
    }

    /// The delay worked out for the engine thread.
    pub(crate) fn design(&self) -> DelayDesign {
        let held = self.clamped();
        let frames = (f64::from(self.time_ms_in_force()) * 1e-3 * SAMPLE_RATE).round() as u32;
        DelayDesign {
            frames: frames.clamp(1, line_frames() as u32 - 1),
            feedback: held.feedback_percent / 100.0,
            high_cut: (-std::f64::consts::TAU * f64::from(held.high_cut_hz) / SAMPLE_RATE).exp()
                as f32,
            ping_pong: held.ping_pong,
        }
    }
}

/// A delay's settings worked out for the engine thread: plain numbers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DelayDesign {
    /// The read position behind the write position.
    frames: u32,
    feedback: f32,
    /// The one-pole low-pass coefficient on the repeats.
    high_cut: f32,
    ping_pong: bool,
}

impl Default for DelayDesign {
    fn default() -> Self {
        DelaySettings::default().design()
    }
}

/// The running delay.
pub(crate) struct Delay {
    left: Vec<f32>,
    right: Vec<f32>,
    write: usize,
    /// The lines' read position, `frames` behind the write position.
    tap: MovingTap,
    feedback: Glide,
    high_cut: Glide,
    ping_pong: bool,
    /// The high cut's state on each side.
    low_left: f32,
    low_right: f32,
    quiet_frames: usize,
    idle: bool,
}

impl Delay {
    /// A delay with `design`, silent and idle, both lines allocated for the longest time.
    /// Allocates: call it off the engine thread.
    pub(crate) fn new(design: &DelayDesign) -> Self {
        Self {
            left: vec![0.0; line_frames()],
            right: vec![0.0; line_frames()],
            write: 0,
            tap: MovingTap::new(design.frames),
            feedback: Glide::new(design.feedback),
            high_cut: Glide::new(design.high_cut),
            ping_pong: design.ping_pong,
            low_left: 0.0,
            low_right: 0.0,
            quiet_frames: 0,
            idle: true,
        }
    }

    /// The memory its lines take, in floats.
    pub(crate) fn floats(&self) -> usize {
        self.left.len() + self.right.len()
    }

    /// Takes `design`: the feedback and the high cut glide there, a new time crossfades in, and
    /// ping-pong changes the routing of what comes in from now (the repeats already in the lines
    /// carry on as they were).
    pub(crate) fn apply(&mut self, design: &DelayDesign) {
        self.feedback.set(design.feedback, GLIDE_FRAMES);
        self.high_cut.set(design.high_cut, GLIDE_FRAMES);
        self.ping_pong = design.ping_pong;
        self.tap.set(design.frames);
    }

    /// True while both lines hold nothing but zeros.
    pub(crate) fn is_idle(&self) -> bool {
        self.idle
    }

    fn read(line: &[f32], write: usize, frames: u32) -> f32 {
        line[(write + line.len() - frames as usize) % line.len()]
    }

    /// Runs the mono `input` through the delay into `output`, stereo interleaved, as many frames
    /// as both hold; `output` is overwritten with the repeats. Allocation-free.
    pub(crate) fn process(&mut self, input: &[f32], output: &mut [f32]) {
        for (&sample, frame) in input.iter().zip(output.chunks_exact_mut(2)) {
            let sample = if sample.is_finite() { sample } else { 0.0 };
            let (left, right) = match self.tap.next() {
                Taps::One(frames) => (
                    Self::read(&self.left, self.write, frames),
                    Self::read(&self.right, self.write, frames),
                ),
                Taps::Two { from, to, way } => {
                    let old = (
                        Self::read(&self.left, self.write, from),
                        Self::read(&self.right, self.write, from),
                    );
                    let new = (
                        Self::read(&self.left, self.write, to),
                        Self::read(&self.right, self.write, to),
                    );
                    (
                        old.0 * (1.0 - way) + new.0 * way,
                        old.1 * (1.0 - way) + new.1 * way,
                    )
                }
            };
            let (feedback, high_cut) = (self.feedback.next(), self.high_cut.next());
            self.low_left = quiet(left * (1.0 - high_cut) + self.low_left * high_cut);
            self.low_right = quiet(right * (1.0 - high_cut) + self.low_right * high_cut);
            let (into_left, into_right) = if self.ping_pong {
                (sample + self.low_right * feedback, self.low_left * feedback)
            } else {
                (
                    sample + self.low_left * feedback,
                    sample + self.low_right * feedback,
                )
            };
            let (into_left, into_right) = (quiet(into_left), quiet(into_right));
            self.left[self.write] = into_left;
            self.right[self.write] = into_right;
            self.write += 1;
            if self.write == self.left.len() {
                self.write = 0;
            }
            frame[0] = left;
            frame[1] = right;
            let wrote = into_left != 0.0
                || into_right != 0.0
                || self.low_left != 0.0
                || self.low_right != 0.0;
            if sample == 0.0 && !wrote {
                self.quiet_frames = self.quiet_frames.saturating_add(1);
            } else {
                self.quiet_frames = 0;
                self.idle = false;
            }
        }
        if !self.idle && self.quiet_frames >= self.left.len() {
            self.left.fill(0.0);
            self.right.fill(0.0);
            self.idle = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::tests::{rms, sine};

    fn delay(settings: DelaySettings) -> Delay {
        Delay::new(&settings.design())
    }

    fn run(delay: &mut Delay, input: &[f32]) -> Vec<f32> {
        let mut output = vec![0.0; input.len() * 2];
        for (input, output) in input.chunks(256).zip(output.chunks_mut(512)) {
            delay.process(input, output);
        }
        output
    }

    fn impulse(frames: usize) -> Vec<f32> {
        let mut input = vec![0.0; frames];
        input[0] = 1.0;
        input
    }

    /// The frames where `side` (0 left, 1 right) of `output` peaks above `floor`, in order.
    fn repeats(output: &[f32], side: usize, floor: f32) -> Vec<usize> {
        let samples: Vec<f32> = output.iter().skip(side).step_by(2).copied().collect();
        (1..samples.len() - 1)
            .filter(|&n| {
                samples[n].abs() > floor
                    && samples[n].abs() >= samples[n - 1].abs()
                    && samples[n].abs() > samples[n + 1].abs()
            })
            .collect()
    }

    #[test]
    fn an_impulse_repeats_at_the_time_each_repeat_quieter_and_darker() {
        let mut delay = delay(DelaySettings {
            time_ms: 100.0,
            feedback_percent: 50.0,
            high_cut_hz: 20_000.0,
            ..DelaySettings::default()
        });
        let output = run(&mut delay, &impulse(48_000));
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            output[..2 * 4_800].iter().all(|sample| *sample == 0.0),
            "nothing before 100 ms"
        );
        let peaks = repeats(&output, 0, 1e-3);
        assert!(peaks.len() >= 4, "{peaks:?}");
        for pair in peaks.windows(2) {
            let gap = pair[1] - pair[0];
            assert!((4_799..=4_802).contains(&gap), "every 100 ms: {peaks:?}");
        }
        let at = |n: usize| output[2 * n].abs();
        assert!(at(peaks[1]) < at(peaks[0]) * 0.6, "quieter each time");
        assert!(at(peaks[2]) < at(peaks[1]) * 0.6);
        // Straight: both sides repeat together.
        assert_eq!(repeats(&output, 1, 1e-3), peaks);
    }

    #[test]
    fn ping_pong_bounces_the_repeats_between_the_sides() {
        let mut delay = delay(DelaySettings {
            time_ms: 50.0,
            feedback_percent: 60.0,
            ping_pong: true,
            ..DelaySettings::default()
        });
        let output = run(&mut delay, &impulse(24_000));
        let left = repeats(&output, 0, 1e-3);
        let right = repeats(&output, 1, 1e-3);
        assert!(left.len() >= 2 && right.len() >= 2, "{left:?} {right:?}");
        // Left at 50, 150, 250 ms...; right at 100, 200 ms...
        assert!(left[0] < right[0] && right[0] < left[1] && left[1] < right[1]);
        assert!((left[0] as i64 - 2_400).abs() <= 2, "{left:?}");
        assert!((right[0] as i64 - 4_800).abs() <= 3, "{right:?}");
    }

    #[test]
    fn a_tempo_sets_the_time_from_the_note() {
        let at = |tempo_bpm: f32, note: DelayNote| {
            DelaySettings {
                tempo_bpm,
                note,
                ..DelaySettings::default()
            }
            .time_ms_in_force()
        };
        assert_eq!(at(120.0, DelayNote::Quarter), 500.0);
        assert_eq!(at(120.0, DelayNote::DottedEighth), 375.0);
        assert_eq!(at(120.0, DelayNote::Sixteenth), 125.0);
        assert!((at(90.0, DelayNote::EighthTriplet) - 222.222).abs() < 0.01);
        assert_eq!(
            at(30.0, DelayNote::Half),
            2_000.0,
            "held to the longest time"
        );
        assert_eq!(at(0.0, DelayNote::Half), 375.0, "no tempo: the time in ms");
        assert_eq!(
            at(10.0, DelayNote::Half),
            375.0,
            "a tempo under the range is none"
        );
    }

    #[test]
    fn silence_in_gives_exact_silence_once_the_repeats_died_and_the_delay_goes_idle() {
        let mut delay = delay(DelaySettings {
            time_ms: 500.0,
            feedback_percent: 90.0,
            high_cut_hz: 20_000.0,
            ..DelaySettings::default()
        });
        assert!(delay.is_idle());
        let mut input = sine(500.0, 0.9, 4_800);
        input.resize(48_000 * 150, 0.0);
        let output = run(&mut delay, &input);
        assert!(output.iter().all(|sample| sample.is_finite()));
        assert!(
            output[2 * 24_000..2 * 30_000]
                .iter()
                .any(|sample| *sample != 0.0),
            "it repeated first"
        );
        assert!(delay.is_idle(), "the repeats died out");
        assert!(output[output.len() - 192_000..]
            .iter()
            .all(|sample| sample.to_bits() == 0));
        assert!(
            output
                .iter()
                .all(|sample| *sample == 0.0 || sample.is_normal()),
            "no subnormal on the way"
        );
    }

    #[test]
    fn a_new_time_crossfades_instead_of_clicking() {
        let tone = sine(330.0, 0.5, 48_000 * 2);
        let steepest = |output: &[f32]| {
            output
                .chunks_exact(2)
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| (pair[1][0] - pair[0][0]).abs())
                .fold(0.0f32, f32::max)
        };
        let mut delay = delay(DelaySettings {
            time_ms: 300.0,
            feedback_percent: 0.0,
            ..DelaySettings::default()
        });
        let mut output = run(&mut delay, &tone[..48_000]);
        delay.apply(
            &DelaySettings {
                time_ms: 737.0,
                feedback_percent: 70.0,
                high_cut_hz: 2_000.0,
                ..DelaySettings::default()
            }
            .design(),
        );
        output.extend(run(&mut delay, &tone[48_000..]));
        let before = steepest(&output[2 * 20_000..2 * 47_000]);
        let around = steepest(&output[2 * 47_000..2 * 50_000]);
        // The tone's own steepest step, with some room for the crossfade's sum of two phases.
        assert!(
            around <= before * 2.0,
            "a jump of {around} against {before}"
        );
        assert!(rms(&output[2 * 60_000..]) > 0.05, "still repeating");
    }

    /// Times dragged quickly: four come within one crossfade (every 5 ms), between two taps that
    /// read the tone in opposite phases. The output never jumps, and it ends on the last time
    /// asked for.
    #[test]
    fn times_that_come_during_a_crossfade_never_click() {
        // 220 Hz: 375 ms is 82.5 periods, 100 ms is 22, so the two taps read opposite phases.
        let tone = sine(220.0, 0.5, 48_000 * 2);
        let steepest = |output: &[f32]| {
            output
                .chunks_exact(2)
                .collect::<Vec<_>>()
                .windows(2)
                .map(|pair| (pair[1][0] - pair[0][0]).abs())
                .fold(0.0f32, f32::max)
        };
        let at = |time_ms: f32| {
            DelaySettings {
                time_ms,
                feedback_percent: 0.0,
                high_cut_hz: 20_000.0,
                ..DelaySettings::default()
            }
            .design()
        };
        let mut delay = Delay::new(&at(375.0));
        let mut output = run(&mut delay, &tone[..48_000]);
        let mut from = 48_000;
        for time_ms in [100.0, 375.0, 100.0, 375.0, 100.0] {
            delay.apply(&at(time_ms));
            output.extend(run(&mut delay, &tone[from..from + 240]));
            from += 240;
        }
        output.extend(run(&mut delay, &tone[from..]));
        let before = steepest(&output[2 * 20_000..2 * 47_000]);
        let around = steepest(&output[2 * 47_000..2 * 56_000]);
        assert!(
            around <= before * 1.5,
            "a jump of {around} against {before}"
        );
        // Settled on 100 ms: the output is the tone 4,800 frames late.
        let settled = &output[2 * 70_000..2 * 70_100];
        let expected = &tone[70_000 - 4_800..70_100 - 4_800];
        assert!(settled
            .chunks_exact(2)
            .zip(expected)
            .all(|(frame, want)| (frame[0] - want).abs() < 0.02));
    }

    #[test]
    fn settings_hold_their_ranges_and_round_trip_through_json() {
        let wild = DelaySettings {
            time_ms: 9_000.0,
            tempo_bpm: 999.0,
            note: DelayNote::Eighth,
            feedback_percent: 150.0,
            high_cut_hz: f32::NAN,
            ping_pong: true,
        };
        let held = wild.clamped();
        assert_eq!(held.time_ms, 2_000.0);
        assert_eq!(held.tempo_bpm, 300.0);
        assert_eq!(held.feedback_percent, 90.0);
        assert_eq!(held.high_cut_hz, 6_000.0);
        let json = serde_json::to_string(&wild).unwrap();
        assert!(
            json.contains("\"note\":\"eighth\"") && json.contains("\"pingPong\":true"),
            "{json}"
        );
        let partial: DelaySettings =
            serde_json::from_str(r#"{"tempoBpm":120,"note":"dottedEighth"}"#).unwrap();
        assert_eq!(partial.time_ms_in_force(), 375.0);
        assert_eq!(partial.feedback_percent, 35.0);
    }

    #[test]
    fn running_and_applying_never_allocate_and_the_lines_are_sized_once() {
        let mut delay = delay(DelaySettings::default());
        assert_eq!(delay.floats(), 2 * 96_001, "two seconds a side (750 KB)");
        let input = sine(330.0, 0.5, 256);
        let mut output = vec![0.0; 512];
        let designs = [
            DelaySettings::default().design(),
            DelaySettings {
                time_ms: 1_200.0,
                ping_pong: true,
                ..DelaySettings::default()
            }
            .design(),
        ];
        let (allocations, frees) = crate::pipeline::tests::allocations::count(|| {
            for design in &designs {
                delay.apply(design);
                for _ in 0..8 {
                    delay.process(&input, &mut output);
                }
            }
        });
        assert_eq!((allocations, frees), (0, 0));
    }
}
