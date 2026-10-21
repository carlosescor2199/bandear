//! A bench for the engine thread's cost, for the `engine_load` example: the engine's own sinks
//! (the phones' network sink, the local output's and the recorder's) fed by a generated source
//! through a capture ring, as the engine thread runs them, with each block timed.
//!
//! Hidden from the documentation: it is a measuring tool, not part of the engine's interface.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::capture::TARGET_SAMPLE_RATE;
use crate::engine::{LevelMeter, OutputMeter};
use crate::load::EngineLoadStatus;
use crate::mix::{GroupLayout, MixState};
use crate::network::{NetworkSink, PacketStats};
use crate::output::local_output_sink;
use crate::pipeline::{CaptureLinks, CaptureStats, CaptureWriter, Pipeline, SourceFormat};
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
}

impl Default for LoadBenchOptions {
    fn default() -> Self {
        Self {
            inputs: 32,
            targets: 16,
            seconds: 10.0,
            block_frames: 256,
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

/// Runs the engine's sinks on a generated source for `options.seconds` of audio, paced in real
/// time, and times every block. The phones are sockets bound on the loopback interface, one
/// address each (127.0.0.1, 127.0.0.2, ...): nothing leaves the machine.
pub fn run_engine_load(options: &LoadBenchOptions) -> Result<LoadBenchReport, String> {
    let inputs = options.inputs.max(1);
    let block_frames = options
        .block_frames
        .clamp(1, crate::pipeline::MAX_BLOCK_FRAMES);
    let phones = (0..options.targets)
        .map(|index| {
            let address = Ipv4Addr::new(127, 0, (index / 250) as u8, (index % 250 + 1) as u8);
            UdpSocket::bind((address, 0))
                .map_err(|error| format!("could not bind a phone on {address}: {error}"))
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
    let (local, _output_control) =
        local_output_sink(Arc::clone(&local_mix), OutputMeter::default());
    let (recorder, _recorder_lines) = recorder_sink(Arc::clone(&local_mix));
    let levels = Arc::new(LevelMeter::new());
    let mut pipeline = Pipeline::new(
        vec![Box::new(network), Box::new(local), Box::new(recorder)],
        Arc::clone(&levels),
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

    let period = Duration::from_secs_f64(block_frames as f64 / f64::from(TARGET_SAMPLE_RATE));
    let blocks = ((options.seconds.max(0.0) / period.as_secs_f64()).ceil() as usize).max(1);
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
