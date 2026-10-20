//! Channel processing: the filters and dynamics of a console's channel strip, for one input or a
//! linked stereo pair, at the engine's 48 kHz.
//!
//! Every processor follows the engine thread's rules:
//!
//! - **Nothing allocated on the audio path.** Its state is fixed-size and made in `new`; settings
//!   are turned into a design (coefficients, linear thresholds, time constants) off the engine
//!   thread, and applying a design copies plain numbers.
//! - **In place, on interleaved frames.** `process(block, stride, first)` works on the channels
//!   `first` (and `first + 1` for a linked pair) of a block whose frames are `stride` samples wide,
//!   so a stage can process one input of a whole capture block without copying it out.
//! - **An exact bypass.** A processor with neutral settings, or turned off, does not touch the
//!   samples at all: its output is the input bit for bit. One that is turned off while working first
//!   glides back to neutral, then stops touching them.
//! - **No clicks.** A filter's coefficients glide to new ones over [GLIDE_FRAMES]; a gain that is
//!   set (makeup) glides the same way, and gains a detector moves follow its attack and release.
//! - **No denormals.** Feedback state that decays towards zero is flushed to zero below
//!   [DENORMAL_FLOOR] (about -400 dBFS), so a silent tail never falls into the slow subnormal range.

use std::ops::RangeInclusive;

use crate::capture::TARGET_SAMPLE_RATE;

pub(crate) mod biquad;
pub(crate) mod eq;

/// The rate every processor runs at.
pub(crate) const SAMPLE_RATE: f64 = TARGET_SAMPLE_RATE as f64;

/// A processor handles one channel or a linked stereo pair.
pub(crate) const MAX_CHANNELS: usize = 2;

/// How long a coefficient or a set gain takes to reach a new value: 10 ms at 48 kHz.
pub(crate) const GLIDE_FRAMES: u32 = 480;

/// Feedback state below this magnitude is set to zero.
pub(crate) const DENORMAL_FLOOR: f32 = 1e-20;

/// `value`, or zero when it is too small to matter and would decay into a subnormal.
#[inline]
pub(crate) fn flush_denormal(value: f32) -> f32 {
    if value.abs() < DENORMAL_FLOOR {
        0.0
    } else {
        value
    }
}

/// The linear gain of `db` decibels.
#[inline]
pub(crate) fn db_to_gain(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// The level of a linear amplitude in decibels, -200 dB for silence.
#[inline]
pub(crate) fn gain_to_db(gain: f32) -> f32 {
    20.0 * gain.max(1e-10).log10()
}

/// The one-pole coefficient that covers about 63% of the way to a target in `ms` milliseconds;
/// zero (immediate) for no time.
pub(crate) fn time_coefficient(ms: f32) -> f32 {
    let frames = f64::from(ms) * 1e-3 * SAMPLE_RATE;
    if frames <= 0.0 || !frames.is_finite() {
        0.0
    } else {
        (-1.0 / frames).exp() as f32
    }
}

/// `value` held inside `range`; `fallback` when it is not a number.
pub(crate) fn clamp_to(value: f32, range: &RangeInclusive<f32>, fallback: f32) -> f32 {
    if value.is_nan() {
        fallback
    } else {
        value.clamp(*range.start(), *range.end())
    }
}

/// The samples of channels `first..first + channels` of each frame of `block`, whose frames are
/// `stride` samples wide. Nothing when those channels do not fit in a frame.
#[inline]
pub(crate) fn channel_frames(
    block: &mut [f32],
    stride: usize,
    first: usize,
    channels: usize,
) -> impl Iterator<Item = &mut [f32]> + '_ {
    let fits = stride > 0 && channels > 0 && first + channels <= stride;
    let frames = if fits { block.len() / stride } else { 0 };
    block
        .chunks_exact_mut(stride.max(1))
        .take(frames)
        .map(move |frame| &mut frame[first..first + channels])
}

/// A value that moves to a new target in a straight line over a number of frames, then holds it
/// exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Glide {
    value: f32,
    target: f32,
    step: f32,
    remaining: u32,
}

impl Glide {
    pub(crate) const fn new(value: f32) -> Self {
        Self {
            value,
            target: value,
            step: 0.0,
            remaining: 0,
        }
    }

    /// Starts moving from the current value to `target` over `frames` frames.
    pub(crate) fn set(&mut self, target: f32, frames: u32) {
        if target == self.target {
            return;
        }
        self.target = target;
        if frames == 0 {
            self.value = target;
            self.remaining = 0;
        } else {
            self.step = (target - self.value) / frames as f32;
            self.remaining = frames;
        }
    }

    /// Jumps to `value` with no glide.
    pub(crate) fn jump(&mut self, value: f32) {
        *self = Self::new(value);
    }

    /// Advances one frame and returns the value for it; the last frame lands on the target.
    #[inline]
    pub(crate) fn next(&mut self) -> f32 {
        if self.remaining > 0 {
            self.remaining -= 1;
            self.value = if self.remaining == 0 {
                self.target
            } else {
                self.value + self.step
            };
        }
        self.value
    }

    pub(crate) fn value(&self) -> f32 {
        self.value
    }

    pub(crate) fn target(&self) -> f32 {
        self.target
    }

    /// True once the value is the target.
    pub(crate) fn is_settled(&self) -> bool {
        self.remaining == 0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A sine of `amplitude` at `frequency` Hz, `frames` long.
    pub(crate) fn sine(frequency: f64, amplitude: f32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                let phase = std::f64::consts::TAU * frequency * n as f64 / SAMPLE_RATE;
                amplitude * phase.sin() as f32
            })
            .collect()
    }

    /// Root mean square of `samples`.
    pub(crate) fn rms(samples: &[f32]) -> f64 {
        let sum: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
        (sum / samples.len() as f64).sqrt()
    }

    /// A signal that covers the awkward values: zeros of both signs, tiny and large samples, a
    /// tone and noise, for exact-bypass checks.
    pub(crate) fn awkward_signal(frames: usize) -> Vec<f32> {
        let mut seed = 0x2545_f491_u32;
        let mut signal = sine(997.0, 0.6, frames);
        for (index, sample) in signal.iter_mut().enumerate() {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 - 0.5) * 0.4;
            *sample = match index % 97 {
                0 => -0.0,
                1 => 0.0,
                2 => 1e-30,
                3 => -1.5,
                4 => f32::MIN_POSITIVE,
                _ => *sample + noise,
            };
        }
        signal
    }

    /// True when `a` and `b` hold the same bits, sample for sample.
    pub(crate) fn same_bits(a: &[f32], b: &[f32]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.to_bits() == y.to_bits())
    }

    #[test]
    fn a_glide_moves_in_a_line_and_lands_on_its_target() {
        let mut glide = Glide::new(0.0);
        glide.set(1.0, 4);
        let values: Vec<f32> = (0..6).map(|_| glide.next()).collect();
        assert_eq!(values, [0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
        assert!(glide.is_settled());
        glide.set(0.0, 0);
        assert_eq!(glide.next(), 0.0);
    }

    #[test]
    fn channel_frames_reach_the_chosen_channels_and_nothing_when_they_do_not_fit() {
        let mut block = [0.0f32, 1.0, 2.0, 3.0, 4.0, 5.0];
        let seen: Vec<Vec<f32>> = channel_frames(&mut block, 3, 1, 2)
            .map(|frame| frame.to_vec())
            .collect();
        assert_eq!(seen, [vec![1.0, 2.0], vec![4.0, 5.0]]);
        assert_eq!(channel_frames(&mut block, 3, 2, 2).count(), 0);
        assert_eq!(channel_frames(&mut block, 0, 0, 1).count(), 0);
    }

    #[test]
    fn decibels_and_times_convert() {
        assert!((db_to_gain(-6.0206) - 0.5).abs() < 1e-5);
        assert!((gain_to_db(0.5) + 6.0206).abs() < 1e-3);
        assert_eq!(gain_to_db(0.0), -200.0);
        assert_eq!(time_coefficient(0.0), 0.0);
        let coefficient = time_coefficient(10.0);
        // After one time constant (480 frames), 1/e of the distance is left.
        let left = f64::from(coefficient).powi(480);
        assert!((left - (-1.0f64).exp()).abs() < 1e-4);
        assert_eq!(clamp_to(f32::NAN, &(1.0..=2.0), 1.5), 1.5);
        assert_eq!(clamp_to(f32::INFINITY, &(1.0..=2.0), 1.5), 2.0);
    }
}
