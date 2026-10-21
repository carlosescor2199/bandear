//! How long the engine thread takes per block for a given session size, without an interface or a
//! network: a generated source feeds the engine's own sinks (every musician's mix and UDP send, the
//! local mix, the recorder) through a capture ring, paced in real time, and each block is timed
//! from its pop to the return of the last sink.
//!
//! Run with `cargo run -p personal-monitoring --example engine_load`, optionally with
//! `--inputs 32 --targets 16 --seconds 10 --block 256` (the defaults). The phones are sockets on
//! the loopback interface, so nothing leaves the machine. A debug build's numbers are pessimistic,
//! often several times a release build's: measure the cost of a session with `--release`.

use std::time::Duration;

use personal_monitoring::bench::{run_engine_load, LoadBenchOptions};

fn main() {
    let options = match parse(std::env::args().skip(1)) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "usage: engine_load [--inputs N] [--targets N] [--seconds S] [--block FRAMES]"
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
        "{} inputs, {} musicians, {}-frame blocks, {} s, {profile} build",
        options.inputs, options.targets, options.block_frames, options.seconds
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
        let value = args.next().ok_or_else(|| format!("{flag} needs a value"))?;
        let number = |value: &str| {
            value
                .parse::<f64>()
                .ok()
                .filter(|number| number.is_finite() && *number > 0.0)
                .ok_or_else(|| format!("{flag} takes a positive number, not {value}"))
        };
        match flag.as_str() {
            "--inputs" => options.inputs = number(&value)? as usize,
            "--targets" => options.targets = number(&value)? as usize,
            "--seconds" => options.seconds = number(&value)?,
            "--block" => options.block_frames = number(&value)? as usize,
            _ => return Err(format!("unknown option {flag}")),
        }
    }
    Ok(options)
}
