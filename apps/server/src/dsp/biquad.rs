//! Second-order filters (biquads), designed from the public formulas of Robert Bristow-Johnson's
//! "Audio EQ Cookbook": a 12 dB/octave high-pass, a low shelf, a peak and a high shelf.
//!
//! A design is worked out in f64 off the engine thread ([BiquadCoefficients::design]); the filter
//! runs in f32 as a transposed direct form II, which keeps its state small and well scaled. New
//! coefficients are reached in a straight line over [GLIDE_FRAMES]: each step is a mix of the old
//! and the new filter's coefficients, and as the stable region of a biquad's feedback coefficients
//! (the stability triangle) is convex, every step on the way is stable too. A filter set to nothing
//! glides to the identity, then lets the samples through untouched.

use super::{channel_frames, Glide, DENORMAL_FLOOR, GLIDE_FRAMES, MAX_CHANNELS, SAMPLE_RATE};

/// The kind of a biquad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FilterShape {
    /// 12 dB per octave below the frequency; `q` shapes the knee (0.707 is maximally flat).
    HighPass,
    /// `gain_db` below the frequency, half of it at the frequency.
    LowShelf,
    /// `gain_db` at the frequency, over a band `q` wide.
    Peak,
    /// `gain_db` above the frequency, half of it at the frequency.
    HighShelf,
}

/// A biquad's coefficients normalised so that a0 is 1:
/// `y[n] = b0 x[n] + b1 x[n-1] + b2 x[n-2] - a1 y[n-1] - a2 y[n-2]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct BiquadCoefficients {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoefficients {
    /// The filter that changes nothing.
    pub(crate) const IDENTITY: Self = Self {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// The cookbook filter of `shape` at `frequency_hz` with quality `q` and, for the shelves and
    /// the peak, `gain_db`, at the engine's 48 kHz. The caller keeps the numbers in range (see
    /// [super::eq]); the frequency must be below the Nyquist frequency and `q` above zero.
    pub(crate) fn design(shape: FilterShape, frequency_hz: f64, q: f64, gain_db: f64) -> Self {
        let w0 = std::f64::consts::TAU * frequency_hz / SAMPLE_RATE;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * q);
        let a = 10f64.powf(gain_db / 40.0);
        let (b0, b1, b2, a0, a1, a2) = match shape {
            FilterShape::HighPass => (
                (1.0 + cos) / 2.0,
                -(1.0 + cos),
                (1.0 + cos) / 2.0,
                1.0 + alpha,
                -2.0 * cos,
                1.0 - alpha,
            ),
            FilterShape::Peak => (
                1.0 + alpha * a,
                -2.0 * cos,
                1.0 - alpha * a,
                1.0 + alpha / a,
                -2.0 * cos,
                1.0 - alpha / a,
            ),
            FilterShape::LowShelf => {
                let shelf = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) - (a - 1.0) * cos + shelf),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * cos),
                    a * ((a + 1.0) - (a - 1.0) * cos - shelf),
                    (a + 1.0) + (a - 1.0) * cos + shelf,
                    -2.0 * ((a - 1.0) + (a + 1.0) * cos),
                    (a + 1.0) + (a - 1.0) * cos - shelf,
                )
            }
            FilterShape::HighShelf => {
                let shelf = 2.0 * a.sqrt() * alpha;
                (
                    a * ((a + 1.0) + (a - 1.0) * cos + shelf),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * cos),
                    a * ((a + 1.0) + (a - 1.0) * cos - shelf),
                    (a + 1.0) - (a - 1.0) * cos + shelf,
                    2.0 * ((a - 1.0) - (a + 1.0) * cos),
                    (a + 1.0) - (a - 1.0) * cos - shelf,
                )
            }
        };
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    /// The filter's gain at `frequency_hz`, in dB, from its transfer function on the unit circle.
    pub(crate) fn magnitude_db(&self, frequency_hz: f64) -> f64 {
        let w = std::f64::consts::TAU * frequency_hz / SAMPLE_RATE;
        let (sin1, cos1) = w.sin_cos();
        let (sin2, cos2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * cos1 + self.b2 * cos2;
        let num_im = self.b1 * sin1 + self.b2 * sin2;
        let den_re = 1.0 + self.a1 * cos1 + self.a2 * cos2;
        let den_im = self.a1 * sin1 + self.a2 * sin2;
        let power = (num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im);
        10.0 * power.max(1e-30).log10()
    }

    /// The coefficients the engine runs: `[b0, b1, b2, a1, a2]` in f32.
    pub(crate) fn to_f32(self) -> [f32; 5] {
        [
            self.b0 as f32,
            self.b1 as f32,
            self.b2 as f32,
            self.a1 as f32,
            self.a2 as f32,
        ]
    }
}

const IDENTITY: [f32; 5] = [1.0, 0.0, 0.0, 0.0, 0.0];

/// One biquad running on one channel or a linked pair (the same filter on each, with its own
/// state). Starts as the identity, untouched samples.
#[derive(Clone, Debug)]
pub(crate) struct Biquad {
    coefficients: [Glide; 5],
    /// Per channel, the two state values of the transposed direct form II.
    state: [[f32; 2]; MAX_CHANNELS],
    channels: usize,
    /// False while the filter is the identity and holds no state: the samples are not touched.
    active: bool,
}

impl Biquad {
    /// An identity filter for `channels` channels (1, or 2 for a linked pair).
    pub(crate) fn new(channels: usize) -> Self {
        Self {
            coefficients: IDENTITY.map(Glide::new),
            state: [[0.0; 2]; MAX_CHANNELS],
            channels: channels.clamp(1, MAX_CHANNELS),
            active: false,
        }
    }

    /// Glides to `coefficients`, or to the identity for `None`, over [GLIDE_FRAMES].
    pub(crate) fn set(&mut self, coefficients: Option<&BiquadCoefficients>) {
        let target = coefficients.map_or(IDENTITY, |coefficients| coefficients.to_f32());
        if !self.active && target == IDENTITY {
            return;
        }
        self.active = true;
        for (glide, target) in self.coefficients.iter_mut().zip(target) {
            glide.set(target, GLIDE_FRAMES);
        }
    }

    /// Jumps to the identity with no state, untouched samples from the next block.
    pub(crate) fn reset(&mut self) {
        *self = Self::new(self.channels);
    }

    /// False when the samples go through untouched.
    pub(crate) fn is_active(&self) -> bool {
        self.active
    }

    /// Filters channels `first..first + channels` of each `stride`-wide frame of `block`.
    pub(crate) fn process(&mut self, block: &mut [f32], stride: usize, first: usize) {
        if !self.active {
            return;
        }
        let channels = self.channels;
        let gliding = self.coefficients.iter().any(|glide| !glide.is_settled());
        let mut current = self.coefficients.map(|glide| glide.value());
        for frame in channel_frames(block, stride, first, channels) {
            if gliding {
                for (value, glide) in current.iter_mut().zip(self.coefficients.iter_mut()) {
                    *value = glide.next();
                }
            }
            let [b0, b1, b2, a1, a2] = current;
            for (sample, state) in frame.iter_mut().zip(self.state.iter_mut()) {
                let x = *sample;
                let y = b0 * x + state[0];
                let s1 = b1 * x - a1 * y + state[1];
                let s2 = b2 * x - a2 * y;
                // Both values at once: zeroing one alone upsets the pair, and a resonant filter
                // then rings on at the floor instead of decaying.
                *state = if s1.abs() < DENORMAL_FLOOR && s2.abs() < DENORMAL_FLOOR {
                    [0.0; 2]
                } else {
                    [s1, s2]
                };
                *sample = y;
            }
        }
        let settled = self.coefficients.iter().all(Glide::is_settled);
        let identity = self.coefficients.map(|glide| glide.target()) == IDENTITY;
        let silent = self.state.iter().flatten().all(|&value| value == 0.0);
        if settled && identity && silent {
            self.active = false;
        }
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> &[[f32; 2]] {
        &self.state[..self.channels]
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{awkward_signal, rms, same_bits, sine};
    use super::*;

    const SHAPES: [FilterShape; 4] = [
        FilterShape::HighPass,
        FilterShape::LowShelf,
        FilterShape::Peak,
        FilterShape::HighShelf,
    ];

    /// The gain in dB a running filter gives a sine at `frequency` (a whole number of cycles in
    /// the second it is measured over), after a second to settle.
    fn measured_db(coefficients: &BiquadCoefficients, frequency: f64) -> f64 {
        let mut filter = Biquad::new(1);
        filter.set(Some(coefficients));
        let input = sine(frequency, 0.25, 96_000);
        let mut output = input.clone();
        for block in output.chunks_mut(256) {
            filter.process(block, 1, 0);
        }
        let tail = 48_000..96_000;
        20.0 * (rms(&output[tail.clone()]) / rms(&input[tail])).log10()
    }

    #[test]
    fn a_filter_with_nothing_set_leaves_the_samples_bit_for_bit() {
        let input = awkward_signal(4_096);
        let mut output = input.clone();
        let mut filter = Biquad::new(2);
        filter.process(&mut output, 2, 0);
        filter.set(None);
        filter.process(&mut output, 2, 0);
        assert!(!filter.is_active());
        assert!(same_bits(&input, &output));
    }

    #[test]
    fn a_filter_set_back_to_nothing_glides_out_then_leaves_the_samples_alone() {
        let mut filter = Biquad::new(1);
        filter.set(Some(&BiquadCoefficients::design(
            FilterShape::Peak,
            1_000.0,
            1.0,
            12.0,
        )));
        let mut warm = sine(1_000.0, 0.3, 4_800);
        filter.process(&mut warm, 1, 0);
        filter.set(None);
        let mut gliding = sine(1_000.0, 0.3, 960);
        filter.process(&mut gliding, 1, 0);
        assert!(!filter.is_active());
        let input = awkward_signal(1_024);
        let mut output = input.clone();
        filter.process(&mut output, 1, 0);
        assert!(same_bits(&input, &output));
    }

    #[test]
    fn the_cookbook_shapes_have_their_textbook_response() {
        let close = |actual: f64, expected: f64| (actual - expected).abs() < 0.1;
        let high_pass = BiquadCoefficients::design(FilterShape::HighPass, 1_000.0, 0.7071, 0.0);
        assert!(close(high_pass.magnitude_db(1_000.0), -3.01));
        assert!(close(high_pass.magnitude_db(16_000.0), 0.0));
        // 12 dB per octave well below the corner: about -24 dB two octaves down.
        assert!(close(high_pass.magnitude_db(250.0), -24.1));
        let peak = BiquadCoefficients::design(FilterShape::Peak, 1_000.0, 2.0, 9.0);
        assert!(close(peak.magnitude_db(1_000.0), 9.0));
        assert!(close(peak.magnitude_db(30.0), 0.0));
        assert!(close(peak.magnitude_db(20_000.0), 0.0));
        let low = BiquadCoefficients::design(FilterShape::LowShelf, 500.0, 0.7071, -12.0);
        assert!(close(low.magnitude_db(20.0), -12.0));
        assert!(close(low.magnitude_db(500.0), -6.0));
        assert!(close(low.magnitude_db(15_000.0), 0.0));
        let high = BiquadCoefficients::design(FilterShape::HighShelf, 4_000.0, 0.7071, 6.0);
        assert!(close(high.magnitude_db(20_000.0), 6.0));
        assert!(close(high.magnitude_db(4_000.0), 3.0));
        assert!(close(high.magnitude_db(40.0), 0.0));
    }

    #[test]
    fn a_running_filter_gives_the_response_it_was_designed_for() {
        let designs = [
            BiquadCoefficients::design(FilterShape::HighPass, 120.0, 0.7071, 0.0),
            BiquadCoefficients::design(FilterShape::LowShelf, 200.0, 0.7071, 9.0),
            BiquadCoefficients::design(FilterShape::Peak, 1_000.0, 1.5, -10.0),
            BiquadCoefficients::design(FilterShape::HighShelf, 6_000.0, 0.7071, 12.0),
        ];
        for coefficients in &designs {
            for frequency in [100.0, 1_000.0, 8_000.0] {
                let measured = measured_db(coefficients, frequency);
                let designed = coefficients.magnitude_db(frequency);
                assert!(
                    (measured - designed).abs() < 0.1,
                    "{coefficients:?} at {frequency} Hz: measured {measured:.3} dB, designed \
                     {designed:.3} dB"
                );
            }
        }
    }

    #[test]
    fn the_high_pass_cuts_below_its_corner() {
        let high_pass = BiquadCoefficients::design(FilterShape::HighPass, 100.0, 0.7071, 0.0);
        assert!(measured_db(&high_pass, 25.0) < -23.0);
        assert!(measured_db(&high_pass, 50.0) < -11.5);
        assert!(measured_db(&high_pass, 1_000.0).abs() < 0.1);
    }

    #[test]
    fn every_shape_is_stable_in_f32_across_the_whole_range() {
        let frequencies = [
            20.0, 31.5, 50.0, 80.0, 125.0, 200.0, 315.0, 500.0, 800.0, 1_250.0, 2_000.0, 3_150.0,
            5_000.0, 8_000.0, 12_500.0, 16_000.0, 20_000.0,
        ];
        for shape in SHAPES {
            for frequency in frequencies {
                for q in [0.3, 0.5, 0.7071, 1.0, 2.0, 4.0, 10.0] {
                    for gain in [-18.0, -9.0, 0.0, 9.0, 18.0] {
                        let [_, _, _, a1, a2] =
                            BiquadCoefficients::design(shape, frequency, q, gain).to_f32();
                        // The stability triangle: both poles inside the unit circle.
                        assert!(
                            a2.abs() < 1.0 && a1.abs() < 1.0 + a2,
                            "{shape:?} {frequency} Hz q {q} {gain} dB: a1 {a1} a2 {a2}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_extreme_filters_ring_down_to_silence() {
        for (shape, frequency, q, gain) in [
            (FilterShape::Peak, 20.0, 10.0, 18.0),
            (FilterShape::Peak, 20.0, 10.0, -18.0),
            (FilterShape::LowShelf, 20.0, 10.0, 18.0),
            (FilterShape::HighPass, 20.0, 10.0, 0.0),
            (FilterShape::HighShelf, 20_000.0, 0.3, 18.0),
            (FilterShape::Peak, 20_000.0, 10.0, 18.0),
        ] {
            let mut filter = Biquad::new(1);
            filter.set(Some(&BiquadCoefficients::design(shape, frequency, q, gain)));
            let mut signal = vec![0.0f32; 48_000 * 4];
            signal[1_000] = 1.0;
            filter.process(&mut signal, 1, 0);
            let tail = signal[signal.len() - 4_800..]
                .iter()
                .fold(0.0f32, |peak, &s| peak.max(s.abs()));
            assert!(tail < 1e-6, "{shape:?} {frequency} Hz: tail {tail}");
        }
    }

    #[test]
    fn a_decaying_tail_never_leaves_subnormals_behind() {
        for coefficients in [
            BiquadCoefficients::design(FilterShape::HighPass, 20.0, 0.7071, 0.0),
            BiquadCoefficients::design(FilterShape::Peak, 30.0, 3.0, 12.0),
        ] {
            let mut filter = Biquad::new(2);
            filter.set(Some(&coefficients));
            let mut signal = sine(40.0, 0.5, 4_800)
                .into_iter()
                .flat_map(|s| [s, -s])
                .collect::<Vec<_>>();
            signal.resize(2 * 48_000 * 8, 0.0);
            for block in signal.chunks_mut(512) {
                filter.process(block, 2, 0);
                assert!(block.iter().all(|s| *s == 0.0 || s.is_normal()));
                assert!(filter
                    .state()
                    .iter()
                    .flatten()
                    .all(|s| *s == 0.0 || s.is_normal()));
            }
            assert!(filter.state().iter().flatten().all(|s| *s == 0.0));
        }
    }

    #[test]
    fn a_new_setting_glides_instead_of_jumping() {
        // A low shelf thrown from -18 dB to +18 dB under a 100 Hz tone: while it moves, the output
        // never steps further between two samples than the tone does once it is at the new gain.
        let shelf = |gain| BiquadCoefficients::design(FilterShape::LowShelf, 400.0, 0.7071, gain);
        let mut filter = Biquad::new(1);
        filter.set(Some(&shelf(-18.0)));
        let mut signal = sine(100.0, 0.1, 48_000);
        // The change lands on a crest of the tone, where a jump would be largest.
        filter.process(&mut signal[..24_120], 1, 0);
        filter.set(Some(&shelf(18.0)));
        filter.process(&mut signal[24_120..], 1, 0);
        let largest_step = |samples: &[f32]| {
            samples
                .windows(2)
                .fold(0.0f32, |step, pair| step.max((pair[1] - pair[0]).abs()))
        };
        let moving = largest_step(&signal[23_000..30_000]);
        let settled = largest_step(&signal[40_000..]);
        assert!(
            moving <= settled * 1.05,
            "moving {moving}, settled {settled}"
        );
    }
}
