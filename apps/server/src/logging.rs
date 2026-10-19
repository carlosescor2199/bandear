//! The engine's diagnostics: one line per event, timestamped, levelled and tagged by area, written
//! to whatever sink the host installs.
//!
//! The engine never decides where its lines go. Until a host calls [set_sink] (the desktop installs
//! a rotating log file at startup), every line goes to stderr, so the CLI, the tests and a
//! development run read exactly as they did before this module existed. No logging crate is used:
//! a small facade is all the engine needs.
//!
//! A line reads `2026-10-18T19:04:05.123Z WARN  [control] control accept error: ...`: the time in
//! UTC as RFC 3339 to the millisecond (UTC, not local time, so lines from a show that crosses a
//! daylight-saving change still sort, and no time-zone lookup is needed), the level padded to five
//! characters, the area in brackets, then the message. A message never spans lines: a line break
//! inside it is written as `\n`, so one event is always one line.
//!
//! **Never log from a real-time path.** The capture and output callbacks and the engine thread's
//! sinks must not allocate, lock or do I/O, and every call here does all three. A problem found
//! there is counted in an atomic and reported off that path instead.

use std::fmt::{self, Write as _};
use std::io::{self, Write as _};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

/// How serious a line is, the most serious first.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
}

impl Level {
    /// The level as it is written in a line.
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
        }
    }

    fn from_u8(value: u8) -> Level {
        match value {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            _ => Level::Debug,
        }
    }
}

/// Where the lines go. A sink is shared by every thread that logs, so it must take whole lines
/// from several threads at once without mixing them.
pub trait LogSink: Send + Sync {
    /// Writes one formatted line (newline included). Never panics: a sink that cannot write falls
    /// back as best it can and carries on.
    fn write_line(&self, level: Level, line: &str);

    /// Pushes out anything still buffered; the host calls [flush] on its way out.
    fn flush(&self) {}
}

/// The sink until a host installs one: standard error, each line written under stderr's lock in
/// one call, so lines from several threads never interleave.
pub struct StderrSink;

impl LogSink for StderrSink {
    fn write_line(&self, _level: Level, line: &str) {
        let _ = io::stderr().lock().write_all(line.as_bytes());
    }
}

static SINK: OnceLock<Box<dyn LogSink>> = OnceLock::new();

/// Debug lines are kept in a development build and dropped in a release one.
const DEFAULT_MAX_LEVEL: Level = if cfg!(debug_assertions) {
    Level::Debug
} else {
    Level::Info
};

static MAX_LEVEL: AtomicU8 = AtomicU8::new(DEFAULT_MAX_LEVEL as u8);

/// Installs the sink every later line goes to. Only the first call wins; a later one hands its
/// sink back.
pub fn set_sink(sink: Box<dyn LogSink>) -> Result<(), Box<dyn LogSink>> {
    SINK.set(sink)
}

/// Whether a host installed a sink; without one, lines go to stderr.
pub fn sink_installed() -> bool {
    SINK.get().is_some()
}

/// Keeps lines up to `level` (included) and drops the less serious ones.
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

/// The least serious level still written.
pub fn max_level() -> Level {
    Level::from_u8(MAX_LEVEL.load(Ordering::Relaxed))
}

/// Writes one line from `area` at `level`; the `log_*!` macros call this.
pub fn log(level: Level, area: &str, message: fmt::Arguments<'_>) {
    if level > max_level() {
        return;
    }
    let line = format_line(SystemTime::now(), level, area, message);
    sink().write_line(level, &line);
}

/// Pushes out whatever the installed sink still buffers.
pub fn flush() {
    sink().flush();
}

fn sink() -> &'static dyn LogSink {
    match SINK.get() {
        Some(sink) => sink.as_ref(),
        None => &StderrSink,
    }
}

/// One line as the sinks write it, newline included (see the module docs for the format).
pub fn format_line(
    time: SystemTime,
    level: Level,
    area: &str,
    message: fmt::Arguments<'_>,
) -> String {
    let mut text = String::new();
    let _ = text.write_fmt(message);
    let mut line = String::with_capacity(text.len() + area.len() + 40);
    let _ = write!(line, "{} {:<5} [{area}] ", timestamp(time), level.label());
    for character in text.chars() {
        match character {
            '\n' => line.push_str("\\n"),
            '\r' => line.push_str("\\r"),
            other => line.push(other),
        }
    }
    line.push('\n');
    line
}

/// `time` in UTC as RFC 3339 to the millisecond: `2026-10-18T19:04:05.123Z`.
fn timestamp(time: SystemTime) -> String {
    let seconds = crate::recorder::rfc3339_utc(time);
    let millis = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.subsec_millis());
    let whole = seconds.strip_suffix('Z').unwrap_or(&seconds);
    format!("{whole}.{millis:03}Z")
}

/// Logs an error line: `log_error!("control", "accept error: {error}")`.
#[macro_export]
macro_rules! log_error {
    ($area:expr, $($message:tt)+) => {
        $crate::logging::log($crate::logging::Level::Error, $area, format_args!($($message)+))
    };
}

/// Logs a warning line: `log_warn!("discovery", "beacon bind error: {error}")`.
#[macro_export]
macro_rules! log_warn {
    ($area:expr, $($message:tt)+) => {
        $crate::logging::log($crate::logging::Level::Warn, $area, format_args!($($message)+))
    };
}

/// Logs an informational line: `log_info!("engine", "Selected input: {name}")`.
#[macro_export]
macro_rules! log_info {
    ($area:expr, $($message:tt)+) => {
        $crate::logging::log($crate::logging::Level::Info, $area, format_args!($($message)+))
    };
}

/// Logs a debug line, kept only in a development build by default.
#[macro_export]
macro_rules! log_debug {
    ($area:expr, $($message:tt)+) => {
        $crate::logging::log($crate::logging::Level::Debug, $area, format_args!($($message)+))
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn at(seconds: u64, millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(seconds) + Duration::from_millis(millis)
    }

    #[test]
    fn a_line_is_utc_time_level_area_and_message() {
        let error = "address in use";
        let line = format_line(
            at(1_700_000_000, 123),
            Level::Warn,
            "control",
            format_args!("control listener bind error: {error}"),
        );
        assert_eq!(
            line,
            "2023-11-14T22:13:20.123Z WARN  [control] control listener bind error: address in use\n"
        );
    }

    #[test]
    fn every_level_has_its_label_padded_to_five() {
        let time = at(951_782_400, 7);
        let lines: Vec<String> = [Level::Error, Level::Warn, Level::Info, Level::Debug]
            .into_iter()
            .map(|level| format_line(time, level, "engine", format_args!("x")))
            .collect();
        assert_eq!(
            lines,
            [
                "2000-02-29T00:00:00.007Z ERROR [engine] x\n",
                "2000-02-29T00:00:00.007Z WARN  [engine] x\n",
                "2000-02-29T00:00:00.007Z INFO  [engine] x\n",
                "2000-02-29T00:00:00.007Z DEBUG [engine] x\n",
            ]
        );
    }

    #[test]
    fn a_message_never_spans_lines() {
        let line = format_line(
            at(0, 0),
            Level::Info,
            "engine",
            format_args!("UDP targets:\n  10.0.0.2:5004\r"),
        );
        assert_eq!(
            line,
            "1970-01-01T00:00:00.000Z INFO  [engine] UDP targets:\\n  10.0.0.2:5004\\r\n"
        );
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn lines_go_to_stderr_until_a_host_installs_a_sink() {
        // No test in this crate installs a sink: the engine's own tests log to stderr, as a
        // development run and the CLI do.
        assert!(!sink_installed());
        crate::log_info!("tests", "a line to stderr: {}", 1);
        flush();
    }

    #[test]
    fn levels_order_from_the_most_serious() {
        assert!(Level::Error < Level::Warn);
        assert!(Level::Warn < Level::Info);
        assert!(Level::Info < Level::Debug);
        assert_eq!(Level::from_u8(Level::Info as u8), Level::Info);
        assert!(DEFAULT_MAX_LEVEL >= Level::Info);
    }

    /// Lines in the production code of the files that hold the real-time paths: the capture
    /// writer and the engine thread (pipeline), the local output callback (output) and every
    /// engine-thread sink (`impl Sink for`).
    fn real_time_sources() -> Vec<(&'static str, String)> {
        fn production(source: &str) -> &str {
            source
                .find("\n#[cfg(test)]\nmod tests")
                .map_or(source, |end| &source[..end])
        }
        fn sink_impls(source: &str) -> String {
            let mut blocks = String::new();
            let mut rest = production(source);
            while let Some(start) = rest.find("\nimpl Sink for ") {
                let block = &rest[start..];
                let end = block[1..].find("\n}\n").map_or(block.len(), |end| end + 4);
                blocks.push_str(&block[..end]);
                rest = &block[end..];
            }
            blocks
        }
        let capture = include_str!("capture.rs").replace("\r\n", "\n");
        let callback = capture
            .find("fn open_typed")
            .map(|start| {
                let body = &capture[start..];
                body[..body.find("\n}\n").unwrap_or(body.len())].to_owned()
            })
            .unwrap_or_default();
        vec![
            (
                "pipeline.rs",
                production(&include_str!("pipeline.rs").replace("\r\n", "\n")).to_owned(),
            ),
            (
                "output.rs (callback)",
                production(&include_str!("output.rs").replace("\r\n", "\n")).to_owned(),
            ),
            (
                "network.rs (sink)",
                sink_impls(&include_str!("network.rs").replace("\r\n", "\n")),
            ),
            (
                "recorder.rs (sink)",
                sink_impls(&include_str!("recorder.rs").replace("\r\n", "\n")),
            ),
            ("capture.rs (open_typed)", callback),
        ]
    }

    #[test]
    fn nothing_logs_on_a_real_time_path() {
        let calls = [
            "log_error!",
            "log_warn!",
            "log_info!",
            "log_debug!",
            "logging::",
        ];
        let prints = ["println!", "eprintln!"];
        for (name, source) in real_time_sources() {
            assert!(
                !source.is_empty(),
                "{name}: the real-time code was not found"
            );
            for call in calls {
                assert!(
                    !source.contains(call),
                    "{name} logs on a real-time path ({call})"
                );
            }
            // The capture error callback keeps its stderr line (see `open_typed`); every other
            // real-time path prints nothing at all.
            if !name.starts_with("capture.rs") {
                for print in prints {
                    assert!(!source.contains(print), "{name} prints on a real-time path");
                }
            }
        }
    }
}
