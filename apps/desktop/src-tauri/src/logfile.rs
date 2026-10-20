//! The desktop's log file: every engine and desk line, kept where a release build can still be read.
//!
//! A release build has no console (`windows_subsystem = "windows"`), so stderr is lost. At startup
//! [install] hands the engine's log facade ([personal_monitoring::logging]) a sink that appends to
//! `<app log dir>/mixlink.log`:
//!
//! - **Rotation by size**: a line that would take the file past [MAX_FILE_BYTES] first moves it to
//!   `mixlink.log.1` (`.1` to `.2` and so on), keeping [KEPT_FILES] old files; the oldest is
//!   dropped. A single line longer than the limit is still written whole, to a fresh file.
//! - **Created lazily**: nothing is touched on disk until the first line.
//! - **No line lost on exit**: each line is written to the file in one call as it comes, with no
//!   buffer of its own, so a normal exit (which also calls [personal_monitoring::logging::flush]) or
//!   a crash keeps every line already logged.
//! - **Never panics**: a line the file cannot take (a full disk, a folder that cannot be made) goes
//!   to stderr instead, the first failure of a run says why once (a line that reaches the file
//!   again re-arms it), every failure is counted (`log_location` answers the count), and the next
//!   line tries the file again.
//! - **A rotation that fails keeps the log**: when the old files cannot be moved or dropped (one is
//!   held by another process, say), the line still goes to the current file, which keeps growing,
//!   and rotation is tried again only once it has grown by another limit's worth.
//! - **One line at a time**: a mutex around the file keeps lines from several threads whole. The
//!   real-time paths never log (see the engine's logging module), so this lock is never taken there.
//! - **Mirrored to stderr in a development build**, so `tauri dev` reads as before.

use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use personal_monitoring::logging::{self, Level, LogSink};
use serde::Serialize;

/// The log file's name in the app's log directory.
pub const LOG_FILE_NAME: &str = "mixlink.log";
/// The size a log file is rotated at.
pub const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// How many rotated files are kept beside the current one (`mixlink.log.1` to `.5`).
pub const KEPT_FILES: usize = 5;

/// The two file operations a rotation is made of, so a test can make one fail while the folder
/// stays writable (as a file held open by another process does on Windows).
#[derive(Clone, Copy)]
struct FileOps {
    remove: fn(&Path) -> io::Result<()>,
    rename: fn(&Path, &Path) -> io::Result<()>,
}

impl Default for FileOps {
    fn default() -> Self {
        Self {
            remove: |path| fs::remove_file(path),
            rename: |from, to| fs::rename(from, to),
        }
    }
}

/// A log file that rotates by size; see the module docs.
pub struct RotatingFile {
    directory: PathBuf,
    name: String,
    max_bytes: u64,
    keep: usize,
    state: Mutex<FileState>,
    failures: Arc<AtomicU64>,
    ops: FileOps,
}

#[derive(Default)]
struct FileState {
    file: Option<File>,
    /// Bytes in the current file, what was there when it was opened included.
    written: u64,
    /// Whether the current run of failures has been explained on stderr; cleared by the next line
    /// that reaches the file, so a later failure is explained again.
    warned: bool,
    /// After a rotation failed, the size the current file must grow past before the next try
    /// (another [RotatingFile::max_bytes] on top of what it held then); 0 when none failed.
    retry_rotation_at: u64,
}

impl RotatingFile {
    /// A file `name` in `directory`, rotated at `max_bytes` with `keep` old files. Touches nothing
    /// on disk.
    pub fn new(directory: PathBuf, name: &str, max_bytes: u64, keep: usize) -> Self {
        Self {
            directory,
            name: name.to_owned(),
            max_bytes,
            keep,
            state: Mutex::new(FileState::default()),
            failures: Arc::new(AtomicU64::new(0)),
            ops: FileOps::default(),
        }
    }

    /// The same file with its rotation done through `ops`: a test's seam.
    #[cfg(test)]
    fn with_ops(mut self, ops: FileOps) -> Self {
        self.ops = ops;
        self
    }

    /// The current file's path.
    pub fn path(&self) -> PathBuf {
        self.directory.join(&self.name)
    }

    /// The lines the file could not take (they went to stderr).
    pub fn failures(&self) -> u64 {
        self.failures.load(Ordering::Relaxed)
    }

    /// The live failure count, shared: what [location] reads after the file went to the sink.
    fn failure_counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.failures)
    }

    /// Appends `line` (newline included); answers whether it reached the file. A line that did not
    /// went to stderr.
    pub fn write_line(&self, line: &str) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match self.append(&mut state, line) {
            Ok(()) => {
                // The file works again: a later failure is explained again.
                state.warned = false;
                true
            }
            Err(error) => {
                // Closed, so the next line opens the file again: a disk that had no room may have
                // some by then.
                state.file = None;
                self.failures.fetch_add(1, Ordering::Relaxed);
                let mut stderr = io::stderr().lock();
                if !state.warned {
                    state.warned = true;
                    let _ = writeln!(
                        stderr,
                        "the log file \"{}\" cannot be written ({error}); its lines go to stderr",
                        self.path().display()
                    );
                }
                let _ = stderr.write_all(line.as_bytes());
                false
            }
        }
    }

    /// Pushes what the file holds to the system.
    pub fn flush(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(file) = state.file.as_mut() {
            let _ = file.flush();
        }
    }

    fn append(&self, state: &mut FileState, line: &str) -> io::Result<()> {
        let length = line.len() as u64;
        if state.file.is_none() {
            state.file = Some(self.open(&mut state.written)?);
        }
        let after = state.written.saturating_add(length);
        if state.written > 0 && after > self.max_bytes && after > state.retry_rotation_at {
            // Closed before it is renamed: Windows does not rename an open file.
            state.file = None;
            match self.rotate() {
                Ok(()) => state.retry_rotation_at = 0,
                Err(error) => {
                    // A lasting reason (another process holds a file, an old file is read-only)
                    // would fail on every line: the line goes to the current file anyway, and the
                    // next try waits until it has grown by another limit's worth.
                    state.retry_rotation_at = state.written.saturating_add(self.max_bytes);
                    let _ = writeln!(
                        io::stderr().lock(),
                        "the log file \"{}\" cannot be rotated ({error}); it keeps growing until the next try",
                        self.path().display()
                    );
                }
            }
            state.file = Some(self.open(&mut state.written)?);
        }
        let Some(file) = state.file.as_mut() else {
            return Err(io::Error::other("the log file is not open"));
        };
        file.write_all(line.as_bytes())?;
        state.written += length;
        Ok(())
    }

    fn open(&self, written: &mut u64) -> io::Result<File> {
        fs::create_dir_all(&self.directory)?;
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())?;
        *written = file.metadata()?.len();
        Ok(file)
    }

    fn rotated(&self, index: usize) -> PathBuf {
        self.directory.join(format!("{}.{index}", self.name))
    }

    /// `name.keep` is dropped, every `name.N` becomes `name.N+1` and the current file `name.1`.
    fn rotate(&self) -> io::Result<()> {
        if self.keep == 0 {
            return self.remove_if_present(&self.path());
        }
        self.remove_if_present(&self.rotated(self.keep))?;
        for index in (1..self.keep).rev() {
            self.rename_if_present(&self.rotated(index), &self.rotated(index + 1))?;
        }
        self.rename_if_present(&self.path(), &self.rotated(1))
    }

    fn remove_if_present(&self, path: &Path) -> io::Result<()> {
        match (self.ops.remove)(path) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    fn rename_if_present(&self, from: &Path, to: &Path) -> io::Result<()> {
        match (self.ops.rename)(from, to) {
            Err(error) if error.kind() != ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// The sink the desktop installs: the rotating file, and stderr too in a development build.
pub struct DesktopSink {
    file: RotatingFile,
    mirror_to_stderr: bool,
}

impl DesktopSink {
    pub fn new(file: RotatingFile, mirror_to_stderr: bool) -> Self {
        Self {
            file,
            mirror_to_stderr,
        }
    }
}

impl LogSink for DesktopSink {
    fn write_line(&self, _level: Level, line: &str) {
        // A line the file could not take is on stderr already.
        if self.file.write_line(line) && self.mirror_to_stderr {
            let _ = io::stderr().lock().write_all(line.as_bytes());
        }
    }

    fn flush(&self) {
        self.file.flush();
    }
}

/// Where the log is: what the window's `log_location` command answers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLocation {
    /// The folder the log files are in.
    pub directory: String,
    /// The current log file.
    pub file: String,
    /// The lines the file could not take since the app started (they went to stderr): 0 while the
    /// log is whole.
    pub failed_lines: u64,
}

/// The installed log file: where it is, and its failure count, read live.
struct Installed {
    directory: String,
    file: String,
    failures: Arc<AtomicU64>,
}

impl Installed {
    fn location(&self) -> LogLocation {
        LogLocation {
            directory: self.directory.clone(),
            file: self.file.clone(),
            failed_lines: self.failures.load(Ordering::Relaxed),
        }
    }
}

static INSTALLED: OnceLock<Installed> = OnceLock::new();

/// Sends every later engine and desk line to `<directory>/mixlink.log`; answers the file's path.
/// Called once at startup; a second call changes nothing.
pub fn install(directory: PathBuf) -> PathBuf {
    let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, MAX_FILE_BYTES, KEPT_FILES);
    let path = file.path();
    let installed = Installed {
        directory: directory.display().to_string(),
        file: path.display().to_string(),
        failures: file.failure_counter(),
    };
    if logging::set_sink(Box::new(DesktopSink::new(file, cfg!(debug_assertions)))).is_ok() {
        let _ = INSTALLED.set(installed);
    }
    path
}

/// Where the log is, and how many lines it missed, once [install] ran.
pub fn location() -> Option<LogLocation> {
    INSTALLED.get().map(Installed::location)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn fresh(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-logfile-tests")
            .join(name);
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn nothing_is_created_before_the_first_line() {
        let directory = fresh("lazy");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 1_000, 2);
        assert!(!directory.exists(), "a new log touches nothing");
        assert!(file.write_line("first\n"));
        assert_eq!(read(&file.path()), "first\n");
    }

    #[test]
    fn a_file_reopened_appends_and_counts_what_it_holds() {
        let directory = fresh("reopen");
        let first = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 20, 2);
        assert!(first.write_line("0123456789\n"));
        drop(first);
        // A restart: the 11 bytes already there count toward the limit.
        let second = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 20, 2);
        assert!(second.write_line("abcdefghij\n"));
        assert_eq!(read(&second.path()), "abcdefghij\n");
        assert_eq!(read(&directory.join("mixlink.log.1")), "0123456789\n");
    }

    #[test]
    fn a_file_rotates_at_its_size_and_keeps_only_the_newest() {
        let directory = fresh("rotate");
        // Each line is 10 bytes and a file takes at most 25: two lines per file.
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 25, 3);
        for index in 0..12 {
            assert!(file.write_line(&format!("line {index:03}\n")));
        }
        assert_eq!(read(&file.path()), "line 010\nline 011\n");
        assert_eq!(
            read(&directory.join("mixlink.log.1")),
            "line 008\nline 009\n"
        );
        assert_eq!(
            read(&directory.join("mixlink.log.2")),
            "line 006\nline 007\n"
        );
        assert_eq!(
            read(&directory.join("mixlink.log.3")),
            "line 004\nline 005\n"
        );
        assert!(
            !directory.join("mixlink.log.4").exists(),
            "only three old files are kept; the oldest lines are gone"
        );
        for kept in fs::read_dir(&directory).expect("log folder").flatten() {
            let size = kept.metadata().expect("size").len();
            assert!(size <= 25, "{:?} is {size} bytes", kept.file_name());
        }
    }

    #[test]
    fn a_line_longer_than_the_limit_is_written_whole() {
        let directory = fresh("long");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 8, 1);
        assert!(file.write_line("short\n"));
        assert!(file.write_line("a line far past the limit\n"));
        assert_eq!(read(&file.path()), "a line far past the limit\n");
        assert_eq!(read(&directory.join("mixlink.log.1")), "short\n");
    }

    #[test]
    fn a_file_that_cannot_be_written_falls_back_without_panicking() {
        let directory = fresh("unwritable");
        fs::create_dir_all(directory.parent().expect("parent")).expect("parent folder");
        // A file where the log folder should be: the folder can never be made.
        fs::write(&directory, b"not a folder").expect("blocking file");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 1_000, 2);
        assert!(!file.write_line("lost to stderr\n"));
        assert!(!file.write_line("and this one\n"));
        assert_eq!(
            file.failures(),
            2,
            "every line the file did not take is counted"
        );
        file.flush();
        // The sink built on it carries on too.
        let sink = DesktopSink::new(file, true);
        sink.write_line(Level::Error, "still no panic\n");
        sink.flush();
        assert_eq!(sink.file.failures(), 3);
        let _ = fs::remove_file(&directory);
    }

    /// A rotation that fails for a lasting reason (here the current file cannot be renamed, as when
    /// a second instance holds it open on Windows) while the folder is writable: every line still
    /// reaches the current file, and the rotation is not tried again on each line but only once the
    /// file has grown by another limit's worth.
    #[test]
    fn a_rotation_that_fails_keeps_writing_and_backs_off() {
        static ATTEMPTS: AtomicU64 = AtomicU64::new(0);
        static HELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
        let directory = fresh("rotation-fails");
        // Each line is 10 bytes and a file takes at most 25.
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 25, 2).with_ops(FileOps {
            remove: |path| {
                ATTEMPTS.fetch_add(1, Ordering::Relaxed);
                fs::remove_file(path)
            },
            rename: |from, to| {
                if HELD.load(Ordering::Relaxed) {
                    Err(io::Error::new(ErrorKind::PermissionDenied, "held open"))
                } else {
                    fs::rename(from, to)
                }
            },
        });
        for index in 0..4 {
            assert!(
                file.write_line(&format!("line {index:03}\n")),
                "line {index} reaches the file"
            );
        }
        assert_eq!(
            read(&file.path()),
            "line 000\nline 001\nline 002\nline 003\n",
            "the file keeps growing past its limit"
        );
        assert_eq!(file.failures(), 0, "no line was lost");
        assert_eq!(
            ATTEMPTS.load(Ordering::Relaxed),
            1,
            "one try at the limit, none on the lines right after"
        );

        // The hold is gone; the file has grown by another limit's worth since the failure (20 then,
        // 50 with this line), so the next line tries again and rotates.
        HELD.store(false, Ordering::Relaxed);
        assert!(file.write_line("line 004\n"));
        assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 2);
        assert_eq!(read(&file.path()), "line 004\n");
        assert_eq!(
            read(&directory.join("mixlink.log.1")),
            "line 000\nline 001\nline 002\nline 003\n"
        );
        // Rotation is back to normal: the limit, not the backoff, decides the next one.
        assert!(file.write_line("line 005\n"));
        assert!(file.write_line("line 006\n"));
        assert_eq!(ATTEMPTS.load(Ordering::Relaxed), 3);
        assert_eq!(read(&file.path()), "line 006\n");
    }

    /// The oldest kept file cannot be removed: the same holds, nothing is lost.
    #[test]
    fn an_oldest_file_that_cannot_be_removed_loses_no_line() {
        let directory = fresh("remove-fails");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 25, 1).with_ops(FileOps {
            remove: |_| Err(io::Error::new(ErrorKind::PermissionDenied, "in use")),
            rename: |from, to| fs::rename(from, to),
        });
        for index in 0..6 {
            assert!(file.write_line(&format!("line {index:03}\n")));
        }
        assert_eq!(file.failures(), 0);
        assert_eq!(read(&file.path()).lines().count(), 6);
    }

    /// The explanation on stderr is given once per run of failures: a line that reaches the file
    /// again re-arms it, so a later failure says why again.
    #[test]
    fn the_explanation_is_given_again_after_the_file_recovers() {
        let directory = fresh("recovers");
        fs::create_dir_all(directory.parent().expect("parent")).expect("parent folder");
        fs::write(&directory, b"not a folder").expect("blocking file");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 1_000, 2);
        assert!(!file.write_line("lost\n"));
        assert!(file.state.lock().expect("state").warned);
        fs::remove_file(&directory).expect("unblock");
        assert!(file.write_line("back\n"));
        assert!(
            !file.state.lock().expect("state").warned,
            "a line that reached the file re-arms the explanation"
        );
        assert_eq!(read(&file.path()), "back\n");
    }

    /// `log_location` answers the live count of the lines the file missed.
    #[test]
    fn the_location_reports_the_failed_lines_live() {
        let directory = fresh("location");
        fs::create_dir_all(directory.parent().expect("parent")).expect("parent folder");
        fs::write(&directory, b"not a folder").expect("blocking file");
        let file = RotatingFile::new(directory.clone(), LOG_FILE_NAME, 1_000, 2);
        let installed = Installed {
            directory: directory.display().to_string(),
            file: file.path().display().to_string(),
            failures: file.failure_counter(),
        };
        assert_eq!(installed.location().failed_lines, 0);
        assert!(!file.write_line("lost\n"));
        assert!(!file.write_line("lost too\n"));
        let location = installed.location();
        assert_eq!(location.failed_lines, 2);
        assert_eq!(location.file, file.path().display().to_string());
        let json = serde_json::to_value(&location).expect("json");
        assert_eq!(json["failedLines"], 2);
        let _ = fs::remove_file(&directory);
    }

    #[test]
    fn lines_from_many_threads_never_interleave() {
        let directory = fresh("threads");
        let file = Arc::new(RotatingFile::new(
            directory.clone(),
            LOG_FILE_NAME,
            1 << 30,
            1,
        ));
        let writers: Vec<_> = (0..8)
            .map(|writer| {
                let file = Arc::clone(&file);
                thread::spawn(move || {
                    for index in 0..250 {
                        let line = format!("writer {writer} line {index:04} {}\n", "x".repeat(64));
                        assert!(file.write_line(&line));
                    }
                })
            })
            .collect();
        for writer in writers {
            writer.join().expect("writer thread");
        }
        let text = read(&file.path());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 8 * 250);
        let padding = "x".repeat(64);
        for line in lines {
            let mut words = line.split(' ');
            assert_eq!(words.next(), Some("writer"), "{line}");
            assert!(
                words.next().is_some_and(|w| w.parse::<u32>().is_ok()),
                "{line}"
            );
            assert_eq!(words.next(), Some("line"), "{line}");
            assert!(words.next().is_some_and(|i| i.len() == 4), "{line}");
            assert_eq!(words.next(), Some(padding.as_str()), "{line}");
            assert_eq!(words.next(), None, "{line}");
        }
    }

    #[test]
    fn the_desktop_sink_writes_the_engines_lines_to_its_file() {
        let directory = fresh("sink");
        let sink = DesktopSink::new(
            RotatingFile::new(directory.clone(), LOG_FILE_NAME, MAX_FILE_BYTES, KEPT_FILES),
            false,
        );
        let line = logging::format_line(
            std::time::UNIX_EPOCH,
            Level::Info,
            "desk",
            format_args!("start_engine: {} saved groups applied", 2),
        );
        sink.write_line(Level::Info, &line);
        sink.flush();
        assert_eq!(
            read(&directory.join(LOG_FILE_NAME)),
            "1970-01-01T00:00:00.000Z INFO  [desk] start_engine: 2 saved groups applied\n"
        );
    }
}
