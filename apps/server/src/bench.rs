//! A bench for the engine thread's cost, for the `engine_load` example: the engine's own sinks
//! (the phones' network sink, the local output's and the recorder's) fed by a generated source
//! through a capture ring and the channel processing stage, as the engine thread runs them, with
//! each block timed.
//!
//! Hidden from the documentation: it is a measuring tool, not part of the engine's interface.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::dsp::strip::StripSettings;
use crate::effects::EffectLevels;
use crate::engine::{ChannelMeta, LevelMeter, OutputMeter};
use crate::load::EngineLoadStatus;
use crate::mix::{GroupLayout, MixState, MAX_MIX_CHANNELS};
use crate::network::{NetworkSink, PacketStats};
use crate::output::local_output_sink;
use crate::pipeline::{
    CaptureLinks, CaptureStats, CaptureWriter, Pipeline, SourceFormat, MAX_BLOCK_FRAMES,
};
use crate::processing::{processing_stage, Processing};
use crate::recorder::recorder_sink;
use crate::targets::TargetRegistry;

/// What the bench runs.
#[derive(Clone, Copy, Debug)]
pub struct LoadBenchOptions {
    /// Input channels of the generated source.
    pub inputs: usize,
    /// Musicians, each with a full mix of every input and a phone on the loopback interface.
    pub targets: usize,
    /// Audio time to run, paced in real time as a capture would deliver it.
    pub seconds: f64,
    /// Frames per block at 48 kHz.
    pub block_frames: usize,
    /// A typical channel strip working on every input ([typical_strip]), so the cost of the
    /// channel processing is in the measure; off, every strip is neutral and out of the path.
    pub strips: bool,
    /// The shared reverb and delay running: every input sends to both and every mix (each
    /// musician's and the local one) returns both ([crate::effects]), so their cost is in the measure.
    pub fx: bool,
}

/// The most musicians the bench runs: each is a socket on its own loopback address.
pub const MAX_BENCH_TARGETS: usize = 64;
/// The most audio time the bench runs, paced in real time.
pub const MAX_BENCH_SECONDS: f64 = 600.0;

impl LoadBenchOptions {
    /// Checks the options against the bench's bounds: 1 to 32 inputs (the mixer's channels), 0 to
    /// [MAX_BENCH_TARGETS] musicians, more than 0 and at most [MAX_BENCH_SECONDS] seconds, and 1 to
    /// the engine's largest block of frames. The message names the option out of bounds.
    pub fn validate(&self) -> Result<(), String> {
        if self.inputs == 0 || self.inputs > MAX_MIX_CHANNELS {
            return Err(format!(
                "--inputs takes 1 to {MAX_MIX_CHANNELS} channels, not {}",
                self.inputs
            ));
        }
        if self.targets > MAX_BENCH_TARGETS {
            return Err(format!(
                "--targets takes at most {MAX_BENCH_TARGETS} musicians, not {}",
                self.targets
            ));
        }
        if !(self.seconds > 0.0 && self.seconds <= MAX_BENCH_SECONDS) {
            return Err(format!(
                "--seconds takes more than 0 and at most {MAX_BENCH_SECONDS} seconds, not {}",
                self.seconds
            ));
        }
        if self.block_frames == 0 || self.block_frames > MAX_BLOCK_FRAMES {
            return Err(format!(
                "--block takes 1 to {MAX_BLOCK_FRAMES} frames, not {}",
                self.block_frames
            ));
        }
        Ok(())
    }
}

impl Default for LoadBenchOptions {
    fn default() -> Self {
        Self {
            inputs: 32,
            targets: 16,
            seconds: 10.0,
            block_frames: 256,
            strips: false,
            fx: false,
        }
    }
}

/// What the bench measured.
#[derive(Clone, Debug)]
pub struct LoadBenchReport {
    pub blocks: usize,
    /// The time one block lasts.
    pub block_period: Duration,
    /// Each block's time from its pop to the return of the last sink, as the bench timed it.
    pub mean: Duration,
    pub p99: Duration,
    pub max: Duration,
    /// The engine thread's own load meter at the end (see [crate::EngineStatus::load]).
    pub load: EngineLoadStatus,
    pub packets_sent: u64,
    pub packets_discarded: u64,
}

/// The strip the bench runs on every input with `--strips`: a high-pass filter at 80 Hz, two of
/// the four bands (-3 dB at 400 Hz, +2 dB at 8 kHz) and a compressor at -30 dBFS, 3:1, which the
/// bench's quiet tones (about -26 dBFS) drive into a few dB of reduction.
pub fn typical_strip() -> StripSettings {
    let mut settings = StripSettings::default();
    settings.eq.high_pass.enabled = true;
    settings.eq.bands[1].gain_db = -3.0;
    settings.eq.bands[3].gain_db = 2.0;
    settings.compressor.enabled = true;
    settings.compressor.threshold_db = -30.0;
    settings
}

/// Runs the engine's sinks on a generated source for `options.seconds` of audio, paced in real
/// time, and times every block. Options outside [LoadBenchOptions::validate]'s bounds are an error.
///
/// The phones are sockets bound on the loopback interface, one address each (127.0.0.1,
/// 127.0.0.2, ...): nothing leaves the machine. Musicians are told apart by their IP, one target
/// per IP as on a real network, so they cannot share 127.0.0.1 on different ports: on a host that
/// only answers on 127.0.0.1 (macOS by default) a second musician is an error that says so, and a
/// single musician still runs.
pub fn run_engine_load(options: &LoadBenchOptions) -> Result<LoadBenchReport, String> {
    options.validate()?;
    let inputs = options.inputs;
    let block_frames = options.block_frames;
    let phones = (0..options.targets)
        .map(|index| {
            // At most MAX_BENCH_TARGETS (64) musicians: 127.0.0.1 to 127.0.0.64.
            let address = Ipv4Addr::new(127, 0, 0, index as u8 + 1);
            UdpSocket::bind((address, 0)).map_err(|error| {
                if index == 0 {
                    format!("could not bind a phone on {address}: {error}")
                } else {
                    format!(
                        "could not bind a phone on {address}: {error}. Each musician needs its \
                         own loopback address; on a host that only answers on 127.0.0.1 run with \
                         --targets 1, or add the address (macOS: ifconfig lo0 alias {address})"
                    )
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let addresses = phones
        .iter()
        .map(|phone| phone.local_addr().map_err(|error| error.to_string()))
        .collect::<Result<Vec<SocketAddr>, _>>()?;

    let targets = Arc::new(TargetRegistry::new(&addresses, &GroupLayout::default())?);
    let packet_stats = Arc::new(PacketStats {
        sent: AtomicU64::new(0),
        discarded: AtomicU64::new(0),
    });
    let network = NetworkSink::new(Arc::clone(&targets), Arc::clone(&packet_stats))
        .map_err(|error| format!("could not open the network sink: {error}"))?;
    let local_mix = Arc::new(MixState::default());
    let (local, _output_control) = local_output_sink(
        Arc::clone(&local_mix),
        OutputMeter::default(),
        Arc::default(),
    );
    let (recorder, _recorder_lines) = recorder_sink(Arc::clone(&local_mix));
    let levels = Arc::new(LevelMeter::new());
    let (stage, processing_control) = processing_stage();
    let mut pipeline = Pipeline::with_processing(
        vec![Box::new(network), Box::new(local), Box::new(recorder)],
        Arc::clone(&levels),
        stage,
    );
    let load = pipeline.load();
    let links = CaptureLinks {
        levels,
        stats: Arc::new(CaptureStats::default()),
        wake: None,
    };
    let (mut writer, reader) = CaptureWriter::new(
        TARGET_SAMPLE_RATE,
        SourceFormat {
            channels: inputs,
            block_frames,
        },
        links,
    )?;
    pipeline.set_source(reader);
    // One strip per input, as the engine lays them out; typical ones when asked, else neutral.
    let mut processing = Processing::new(processing_control);
    processing.follow(&vec![ChannelMeta::default(); inputs])?;
    if options.strips || options.fx {
        let mut strip = if options.strips {
            typical_strip()
        } else {
            StripSettings::default()
        };
        if options.fx {
            strip.sends = EffectLevels {
                reverb: 30,
                delay: 20,
            };
        }
        processing.replace(&vec![strip; inputs])?;
    }
    if options.fx {
        let returns = EffectLevels {
            reverb: 40,
            delay: 30,
        };
        for entry in targets.read().iter() {
            entry.mix_state.set_fx_returns(returns);
        }
        local_mix.set_fx_returns(returns);
    }

    let period = Duration::from_secs_f64(block_frames as f64 / f64::from(TARGET_SAMPLE_RATE));
    let blocks = ((options.seconds / period.as_secs_f64()).ceil() as usize).max(1);
    let mut source = vec![0.0f32; block_frames * inputs];
    let mut times = Vec::with_capacity(blocks);
    let mut phase = 0u64;
    let start = Instant::now();
    for index in 0..blocks {
        // Each input a quiet tone of its own; written as a capture callback would, untimed.
        for frame in source.chunks_exact_mut(inputs) {
            for (channel, sample) in frame.iter_mut().enumerate() {
                let step = (phase * (channel as u64 + 1) * 7) % 4_800;
                *sample = 0.05 * (step as f32 / 4_800.0 * std::f32::consts::TAU).sin();
            }
            phase += 1;
        }
        let due = start + period.mul_f64(index as f64);
        let now = Instant::now();
        if due > now {
            std::thread::sleep(due - now);
        }
        writer.write(&source);
        let started = Instant::now();
        pipeline.pump_at(started);
        times.push(started.elapsed());
        pipeline.tick(Instant::now());
    }
    drop(phones);
    drop(processing);

    times.sort_unstable();
    let total: Duration = times.iter().sum();
    let at = |share: f64| times[((times.len() - 1) as f64 * share).round() as usize];
    Ok(LoadBenchReport {
        blocks,
        block_period: period,
        mean: total / blocks as u32,
        p99: at(0.99),
        max: at(1.0),
        load: load.status(),
        packets_sent: packet_stats.sent.load(Ordering::Relaxed),
        packets_discarded: packet_stats.discarded.load(Ordering::Relaxed),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(change: fn(&mut LoadBenchOptions)) -> Result<(), String> {
        let mut options = LoadBenchOptions::default();
        change(&mut options);
        options.validate()
    }

    #[test]
    fn the_bench_refuses_options_outside_its_bounds() {
        assert_eq!(LoadBenchOptions::default().validate(), Ok(()));
        assert!(with(|o| o.inputs = 0).unwrap_err().contains("--inputs"));
        assert_eq!(with(|o| o.inputs = MAX_MIX_CHANNELS), Ok(()));
        assert!(with(|o| o.inputs = MAX_MIX_CHANNELS + 1)
            .unwrap_err()
            .contains("--inputs"));
        assert_eq!(with(|o| o.targets = 0), Ok(()));
        assert_eq!(with(|o| o.targets = MAX_BENCH_TARGETS), Ok(()));
        assert!(with(|o| o.targets = usize::MAX)
            .unwrap_err()
            .contains("--targets"));
        assert_eq!(with(|o| o.seconds = MAX_BENCH_SECONDS), Ok(()));
        assert!(with(|o| o.seconds = 1e300)
            .unwrap_err()
            .contains("--seconds"));
        assert!(with(|o| o.seconds = f64::NAN).is_err());
        assert!(with(|o| o.seconds = f64::INFINITY).is_err());
        assert!(with(|o| o.seconds = 0.0).is_err());
        assert!(with(|o| o.block_frames = 0)
            .unwrap_err()
            .contains("--block"));
        assert!(with(|o| o.block_frames = MAX_BLOCK_FRAMES + 1)
            .unwrap_err()
            .contains("--block"));
    }

    #[test]
    fn a_run_out_of_bounds_is_an_error_before_it_allocates() {
        // Before the bounds, these sizes overflowed a buffer's capacity and aborted the process.
        for options in [
            LoadBenchOptions {
                inputs: usize::MAX,
                ..LoadBenchOptions::default()
            },
            LoadBenchOptions {
                seconds: 1e300,
                ..LoadBenchOptions::default()
            },
        ] {
            assert!(run_engine_load(&options).is_err());
        }
    }
}
