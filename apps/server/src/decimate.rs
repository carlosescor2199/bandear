//! Anti-alias decimation of 96 kHz and 192 kHz capture to the engine's 48 kHz.
//!
//! Reducing the rate by keeping every `ratio`-th frame (what capture did before) folds everything
//! between 24 kHz and the input's Nyquist frequency back into the audible band. Here a linear-phase
//! low-pass runs first: a Kaiser-windowed sinc FIR with its cutoff (-6 dB) at 24 kHz, the Nyquist
//! frequency of 48 kHz, a pass band flat to 20 kHz (ripple under 0.01 dB) and at least 80 dB of
//! attenuation from 28 kHz up. Content between 24 and 28 kHz is only partly removed and folds into
//! 20-24 kHz, above hearing; nothing from 28 kHz up comes back louder than -80 dB.
//!
//! Only every `ratio`-th output is computed, so the cost is one multiply-add per tap per output
//! sample: 63 taps at 96 kHz and 123 at 192 kHz. The filter delays the audio by (taps - 1) / 2
//! input frames, about 0.32 ms at either rate. Everything is allocated in [Decimator::new];
//! [Decimator::process] never allocates, locks or panics.

use crate::capture::TARGET_SAMPLE_RATE;

/// The highest frequency the filter passes unchanged.
const PASS_EDGE_HZ: f64 = 20_000.0;

/// The lowest frequency the filter attenuates by [STOP_ATTENUATION_DB].
const STOP_EDGE_HZ: f64 = 28_000.0;

/// The attenuation the Kaiser design targets from [STOP_EDGE_HZ] up.
const STOP_ATTENUATION_DB: f64 = 80.0;

/// The integer factor between a capture rate and 48 kHz, or why the rate cannot be reduced exactly.
pub(crate) fn sample_rate_reduction_ratio(source_sample_rate: u32) -> Result<usize, String> {
    if source_sample_rate < TARGET_SAMPLE_RATE
        || !source_sample_rate.is_multiple_of(TARGET_SAMPLE_RATE)
    {
        return Err(format!(
            "source sample rate {source_sample_rate} Hz cannot be reduced exactly to {TARGET_SAMPLE_RATE} Hz; only 48000 Hz and integer multiples are supported"
        ));
    }

    Ok((source_sample_rate / TARGET_SAMPLE_RATE) as usize)
}

/// Converts interleaved capture at an integer multiple of 48 kHz to 48 kHz float frames.
///
/// At 48 kHz it only converts. Above it, each channel runs through the low-pass described in the
/// module documentation, and an output frame is produced for every `ratio`-th input frame, the
/// first one included, so frame boundaries and channel order are kept.
pub(crate) struct Decimator {
    ratio: usize,
    channels: usize,
    /// The symmetric low-pass, empty at 48 kHz.
    taps: Box<[f32]>,
    /// Each channel's last `taps.len()` inputs, stored twice in a row (`2 * taps.len()` per
    /// channel) so the newest window is always one contiguous slice starting at `position`.
    history: Box<[f32]>,
    /// Where the next input goes in each channel's window.
    position: usize,
    /// Input frames since the last output frame, modulo `ratio`.
    phase: usize,
}

impl Decimator {
    pub(crate) fn new(source_sample_rate: u32, channels: usize) -> Result<Self, String> {
        if channels == 0 {
            return Err("cannot decimate PCM without channels".to_owned());
        }
        let ratio = sample_rate_reduction_ratio(source_sample_rate)?;
        let taps: Box<[f32]> = if ratio == 1 {
            Box::new([])
        } else {
            low_pass_taps(source_sample_rate).into_boxed_slice()
        };
        let history = vec![0.0; channels * 2 * taps.len()].into_boxed_slice();
        Ok(Self {
            ratio,
            channels,
            taps,
            history,
            position: 0,
            phase: 0,
        })
    }

    pub(crate) fn ratio(&self) -> usize {
        self.ratio
    }

    /// Decimates the whole frames of `input`, converting each sample with `convert`, into
    /// `output`, and returns the number of output frames written.
    ///
    /// `output` should hold `input_frames.div_ceil(ratio)` frames; input whose output frame would
    /// not fit is left unread rather than written out of bounds. A trailing partial frame is
    /// ignored.
    pub(crate) fn process<T: Copy>(
        &mut self,
        input: &[T],
        convert: impl Fn(T) -> f32,
        output: &mut [f32],
    ) -> usize {
        let channels = self.channels;
        let mut slots = output.chunks_exact_mut(channels);
        let mut written = 0;

        if self.ratio == 1 {
            for (frame, slot) in input.chunks_exact(channels).zip(slots) {
                for (out, sample) in slot.iter_mut().zip(frame) {
                    *out = convert(*sample);
                }
                written += 1;
            }
            return written;
        }

        let length = self.taps.len();
        for frame in input.chunks_exact(channels) {
            let slot = if self.phase == 0 {
                match slots.next() {
                    Some(slot) => Some(slot),
                    None => break,
                }
            } else {
                None
            };

            let position = self.position;
            for (window, sample) in self.history.chunks_exact_mut(2 * length).zip(frame) {
                let value = convert(*sample);
                window[position] = value;
                window[position + length] = value;
            }
            self.position = if position + 1 == length {
                0
            } else {
                position + 1
            };

            if let Some(slot) = slot {
                let start = self.position;
                for (out, window) in slot.iter_mut().zip(self.history.chunks_exact(2 * length)) {
                    *out = dot(&self.taps, &window[start..start + length]);
                }
                written += 1;
            }
            self.phase = if self.phase + 1 == self.ratio {
                0
            } else {
                self.phase + 1
            };
        }
        written
    }
}

/// `sum(taps[i] * window[i])` with four running sums, so the adds do not wait on each other.
fn dot(taps: &[f32], window: &[f32]) -> f32 {
    let mut sums = [0.0f32; 4];
    let mut tap_quads = taps.chunks_exact(4);
    let mut window_quads = window.chunks_exact(4);
    for (tap, sample) in (&mut tap_quads).zip(&mut window_quads) {
        sums[0] += tap[0] * sample[0];
        sums[1] += tap[1] * sample[1];
        sums[2] += tap[2] * sample[2];
        sums[3] += tap[3] * sample[3];
    }
    let tail: f32 = tap_quads
        .remainder()
        .iter()
        .zip(window_quads.remainder())
        .map(|(tap, sample)| tap * sample)
        .sum();
    (sums[0] + sums[1]) + (sums[2] + sums[3]) + tail
}

/// The Kaiser-windowed sinc low-pass for `source_sample_rate`, normalized to unity gain at DC.
///
/// The order comes from Kaiser's formula for the transition from [PASS_EDGE_HZ] to [STOP_EDGE_HZ]
/// at [STOP_ATTENUATION_DB], rounded up to an even order so the taps are odd in number and the
/// delay is a whole number of input frames.
fn low_pass_taps(source_sample_rate: u32) -> Vec<f32> {
    let rate = f64::from(source_sample_rate);
    let cutoff = (PASS_EDGE_HZ + STOP_EDGE_HZ) / 2.0 / rate;
    let transition = 2.0 * std::f64::consts::PI * (STOP_EDGE_HZ - PASS_EDGE_HZ) / rate;
    let order = ((STOP_ATTENUATION_DB - 8.0) / (2.285 * transition)).ceil() as usize;
    let order = order + order % 2;
    let beta = 0.1102 * (STOP_ATTENUATION_DB - 8.7);
    let middle = order as f64 / 2.0;
    let window_scale = bessel_i0(beta);

    let taps: Vec<f64> = (0..=order)
        .map(|index| {
            let offset = index as f64 - middle;
            let sinc = if offset == 0.0 {
                2.0 * cutoff
            } else {
                (2.0 * std::f64::consts::PI * cutoff * offset).sin()
                    / (std::f64::consts::PI * offset)
            };
            let edge = offset / middle;
            let window = bessel_i0(beta * (1.0 - edge * edge).max(0.0).sqrt()) / window_scale;
            sinc * window
        })
        .collect();
    let gain: f64 = taps.iter().sum();
    taps.iter().map(|tap| (tap / gain) as f32).collect()
}

/// The zeroth-order modified Bessel function of the first kind, by its power series.
fn bessel_i0(x: f64) -> f64 {
    let half = x / 2.0;
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..64 {
        let factor = half / k as f64;
        term *= factor * factor;
        sum += term;
        if term < sum * 1e-17 {
            break;
        }
    }
    sum
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(sample: f32) -> f32 {
        sample
    }

    /// `frames` of a sine at `frequency` Hz sampled at `rate`, mono.
    fn tone(rate: u32, frequency: f64, frames: usize, amplitude: f64) -> Vec<f32> {
        (0..frames)
            .map(|frame| {
                let time = frame as f64 / f64::from(rate);
                (amplitude * (2.0 * std::f64::consts::PI * frequency * time).sin()) as f32
            })
            .collect()
    }

    fn rms(samples: &[f32]) -> f64 {
        let energy: f64 = samples
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum();
        (energy / samples.len() as f64).sqrt()
    }

    /// The level change, in dB, of a tone at `frequency` decimated from `rate` to 48 kHz, measured
    /// after the filter has settled.
    fn gain_db(rate: u32, frequency: f64) -> f64 {
        let ratio = sample_rate_reduction_ratio(rate).unwrap();
        let input = tone(rate, frequency, 48_000 * ratio / 5, 0.5);
        let mut decimator = Decimator::new(rate, 1).unwrap();
        let mut output = vec![0.0; input.len().div_ceil(ratio)];
        let written = decimator.process(&input, identity, &mut output);
        let settled = &output[256..written];
        20.0 * (rms(settled) / (0.5 / 2f64.sqrt())).log10()
    }

    #[test]
    fn a_48_khz_input_is_only_converted() {
        let mut decimator = Decimator::new(48_000, 2).unwrap();
        let mut output = [0.0; 4];

        let written = decimator.process(&[1i16, 2, 3, 4], f32::from, &mut output);

        assert_eq!(written, 2);
        assert_eq!(output, [1.0, 2.0, 3.0, 4.0]);
    }

    /// Measured: -83.8 dB or lower from 28 kHz up at either rate. Frame dropping alone (the
    /// previous decimation) folds every one of these tones back at 0 dB.
    #[test]
    fn content_above_24_khz_is_attenuated_before_decimation() {
        for frequency in [28_000.0, 30_000.0, 36_000.0, 47_000.0] {
            let gain = gain_db(96_000, frequency);
            assert!(
                gain < -80.0,
                "{frequency} Hz at 96 kHz folds back at {gain:.1} dB"
            );
        }
        for frequency in [28_000.0, 30_000.0, 50_000.0, 70_000.0, 95_000.0] {
            let gain = gain_db(192_000, frequency);
            assert!(
                gain < -80.0,
                "{frequency} Hz at 192 kHz folds back at {gain:.1} dB"
            );
        }
    }

    /// Measured: within 0.002 dB at 1, 10 and 19 kHz.
    #[test]
    fn the_pass_band_is_flat_to_20_khz() {
        for rate in [96_000, 192_000] {
            for frequency in [1_000.0, 10_000.0, 19_000.0] {
                let gain = gain_db(rate, frequency);
                assert!(
                    gain.abs() < 0.05,
                    "{frequency} Hz at {rate} Hz changes by {gain:.3} dB"
                );
            }
        }
    }

    #[test]
    fn a_steady_level_passes_at_unity_gain_with_full_precision() {
        // The largest 24-bit value, as an ASIO driver hands it over in a 32-bit word.
        let level = 8_388_607.0f32 / 8_388_608.0;
        let mut decimator = Decimator::new(192_000, 1).unwrap();
        let mut output = [0.0; 256];

        let written = decimator.process(&[level; 1_024], identity, &mut output);

        assert_eq!(written, 256);
        for sample in &output[64..] {
            assert!((sample - level).abs() < 1e-6, "{sample} != {level}");
        }
    }

    #[test]
    fn decimates_192_khz_to_48_khz_by_complete_frames_keeping_channels_apart() {
        let mut decimator = Decimator::new(192_000, 2).unwrap();
        // 400 frames: the left channel steady at 0.5, the right at -0.25.
        let input: Vec<f32> = (0..400).flat_map(|_| [0.5, -0.25]).collect();
        let mut output = vec![0.0; 2 * 100];

        let written = decimator.process(&input, identity, &mut output);

        assert_eq!(written, 100);
        for frame in output[2 * 64..].chunks_exact(2) {
            assert!((frame[0] - 0.5).abs() < 1e-6);
            assert!((frame[1] + 0.25).abs() < 1e-6);
        }
    }

    #[test]
    fn a_stream_split_anywhere_decimates_like_one_buffer() {
        let input = tone(96_000, 5_000.0, 1_000, 0.5);
        let mut whole = Decimator::new(96_000, 1).unwrap();
        let mut expected = vec![0.0; 500];
        let expected_frames = whole.process(&input, identity, &mut expected);

        let mut pieces = Decimator::new(96_000, 1).unwrap();
        let mut output = vec![0.0; 500];
        let mut written = 0;
        for piece in input.chunks(37) {
            written += pieces.process(piece, identity, &mut output[written..]);
        }

        assert_eq!(written, expected_frames);
        assert_eq!(output, expected);
    }

    #[test]
    fn a_short_output_never_overflows_and_a_partial_frame_is_ignored() {
        let mut decimator = Decimator::new(96_000, 2).unwrap();
        let mut output = [0.0; 4];

        // Eight whole frames would make four outputs; there is room for two. One stray sample.
        let written = decimator.process(&[0.1f32; 17], identity, &mut output);

        assert_eq!(written, 2);
    }

    #[test]
    fn rejects_non_integer_reduction_ratio() {
        let error = sample_rate_reduction_ratio(44_100).unwrap_err();

        assert!(error.contains("cannot be reduced exactly"));
        assert!(Decimator::new(44_100, 2).is_err());
        assert!(Decimator::new(48_000, 0).is_err());
    }

    #[test]
    fn the_filter_lengths_match_the_documented_design() {
        assert_eq!(Decimator::new(96_000, 1).unwrap().taps.len(), 63);
        assert_eq!(Decimator::new(192_000, 1).unwrap().taps.len(), 123);
        assert!(Decimator::new(48_000, 1).unwrap().taps.is_empty());
    }
}
