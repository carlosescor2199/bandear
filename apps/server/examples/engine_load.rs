//! How long the engine thread takes per block for a given session size, without an interface or a
//! network: a generated source feeds the engine's own sinks (every musician's mix and UDP send, the
//! local mix, the recorder) through a capture ring, paced in real time, and each block is timed
//! from its pop to the return of the last sink.
//!
//! Run with `cargo run -p personal-monitoring --example engine_load`, optionally with
//! `--inputs 32 --targets 16 --seconds 10 --block 256` (the defaults; inputs 1 to 32, musicians at
//! most 64, seconds at most 600). `--strips` runs a typical channel strip on every input (a
//! high-pass filter, two EQ bands and a compressor), to measure the channel processing too. `--fx`
//! sends every input to the shared reverb and delay and returns both in every mix. The
//! phones are sockets on the loopback interface, so nothing leaves the machine. Each musician needs its own loopback address (127.0.0.1, 127.0.0.2, ...):
//! on a host that only answers on 127.0.0.1 (macOS by default) run with `--targets 1`, or add the
//! addresses as loopback aliases. A debug build's numbers are pessimistic,
//! often several times a release build's: measure the cost of a session with `--release`.

use std::time::Duration;

use personal_monitoring::bench::{run_engine_load, LoadBenchOptions};

fn main() {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "usage: engine_load [--inputs N] [--targets N] [--seconds S] [--block FRAMES] [--strips] [--fx]"
            );
            std::process::exit(2);
        }
    };
    let profile = if cfg!(debug_assertions) {
        "debug (pessimistic)"
    } else {
        "release"
    };
    println!(
        "{} inputs, {} musicians, {}-frame blocks, {} s, channel strips {}, effects {}, {profile} build",
        options.inputs,
        options.targets,
        options.block_frames,
        options.seconds,
        if options.strips { "on" } else { "off" },
        if options.fx { "on" } else { "off" },
    );
    let report = match run_engine_load(&options) {
        Ok(report) => report,
        Err(message) => {
            eprintln!("the bench could not run: {message}");
            std::process::exit(1);
        }
    };
    let share = |time: Duration| time.as_secs_f64() / report.block_period.as_secs_f64() * 100.0;
    let micros = |time: Duration| time.as_secs_f64() * 1e6;
    println!(
        "{} blocks of {:.0} µs each",
        report.blocks,
        micros(report.block_period)
    );
    for (label, time) in [
        ("mean", report.mean),
        ("p99", report.p99),
        ("max", report.max),
    ] {
        println!(
            "  {label:<4} {:>9.1} µs  {:>6.1}% of the block",
            micros(time),
            share(time)
        );
    }
    println!(
        "engine load meter: last {:.1}%, average {:.1}%, peak {:.1}%, {} late blocks",
        report.load.percent,
        report.load.average_percent,
        report.load.peak_percent,
        report.load.late_blocks
    );
    println!(
        "packets sent {} / discarded {}",
        report.packets_sent, report.packets_discarded
    );
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<LoadBenchOptions, String> {
    let mut options = LoadBenchOptions::default();
    while let Some(flag) = args.next() {
        if flag == "--strips" {
            options.strips = true;
            continue;
        }
        if flag == "--fx" {
            options.fx = true;
            continue;
        }
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let whole = |value: &str| {
            value
                .parse::<usize>()
                .map_err(|_| format!("{flag} takes a whole number, not {value}"))
        };
        match flag.as_str() {
            "--inputs" => options.inputs = whole(&value)?,
            "--targets" => options.targets = whole(&value)?,
            "--seconds" => {
                options.seconds = value
                    .parse::<f64>()
                    .map_err(|_| format!("{flag} takes a number of seconds, not {value}"))?;
            }
            "--block" => options.block_frames = whole(&value)?,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    // 1 to 32 inputs, at most 64 musicians, at most 600 s: a usage error past them.
    options.validate()?;
    Ok(options)
}
