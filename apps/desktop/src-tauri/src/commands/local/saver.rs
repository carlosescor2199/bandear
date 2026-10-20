//! Keeps the engineer's local mix in the session without writing it under the engine lock.
//!
//! A fader moved on the Local mix tab changes the local mix many times a second. Each change is
//! applied by the engine at once, under the engine lock, and only marked here ([LocalMixSaver::
//! mark]): no disk I/O. A thread of its own writes the latest mix marked, at most once per
//! interval ([SAVE_INTERVAL]), so a drag costs a write every few hundred milliseconds and always
//! ends with its last value kept. Stopping the engine, quitting the app and dropping the saver
//! write what is still waiting ([LocalMixSaver::flush]).
//!
//! A write that fails keeps its mix waiting (unless a newer one came) and is tried again at the
//! next interval. The failure is logged once, and again only when it changes, so a session that
//! cannot be written does not fill the log on every tick.
//!
//! Writes are one at a time: whoever takes the waiting mix writes it before anyone else can take
//! the next, so an older mix never lands after a newer one. A start or a loaded session pauses
//! the saver ([LocalMixSaver::pause]) to read or replace the session while no write is in flight.
//!
//! The saver keeps anything the desk changes many times a second the same way ([Saver], over what
//! it keeps, [Kept]): the local mix, and each input's channel processing (see
//! [crate::commands::processing]).

use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use personal_monitoring::{log_error, log_warn};

use crate::session::SessionMix;

/// The most often the local mix is written while it keeps changing.
pub(crate) const SAVE_INTERVAL: Duration = Duration::from_millis(400);

/// What a [Saver] keeps in the session: its name in the lines it logs, and its thread's name.
pub(crate) trait Kept: Clone + Send + 'static {
    /// What is kept, as a log line names it: "the local mix".
    const WHAT: &'static str;
    /// The name of the saver's thread.
    const THREAD: &'static str;
}

impl Kept for SessionMix {
    const WHAT: &'static str = "the local mix";
    const THREAD: &'static str = "mixlink-local-mix-saver";
}

/// The saver of the engineer's local mix.
pub(crate) type LocalMixSaver = Saver<SessionMix>;

type Write<T> = dyn Fn(&T) -> Result<(), String> + Send + Sync;
type Log = dyn Fn(&str) + Send + Sync;

#[cfg(test)]
thread_local! {
    /// Whether savers started on this thread write only when flushed (see [manual_savers]).
    static MANUAL_SAVERS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Until the guard is dropped, every saver started on this thread has no thread of its own and
/// writes only when flushed or dropped, like [LocalMixSaver::manual]. For tests that build a whole
/// `DesktopState` and must not race its saver's interval.
#[cfg(test)]
pub(crate) fn manual_savers() -> ManualSavers {
    MANUAL_SAVERS.with(|manual| manual.set(true));
    ManualSavers(())
}

/// See [manual_savers].
#[cfg(test)]
pub(crate) struct ManualSavers(());

#[cfg(test)]
impl Drop for ManualSavers {
    fn drop(&mut self) {
        MANUAL_SAVERS.with(|manual| manual.set(false));
    }
}

/// Writes the latest value marked, off the engine lock (see the module documentation).
pub(crate) struct Saver<T: Kept> {
    inner: Arc<Inner<T>>,
    thread: Option<JoinHandle<()>>,
}

struct Inner<T> {
    write: Box<Write<T>>,
    log: Box<Log>,
    interval: Duration,
    state: Mutex<State<T>>,
    wake: Condvar,
    /// Held across taking the waiting value and writing it, so writes land in the order taken.
    writing: Mutex<()>,
}

struct State<T> {
    /// The latest value not written yet.
    pending: Option<T>,
    stop: bool,
    /// When the last write was tried, for the interval.
    last_write: Option<Instant>,
    /// The failure said last; a failure is said again only once it changes.
    failure: Option<String>,
}

impl<T> Default for State<T> {
    fn default() -> Self {
        Self {
            pending: None,
            stop: false,
            last_write: None,
            failure: None,
        }
    }
}

impl<T: Kept> Saver<T> {
    /// A saver that writes with `write` on a thread of its own, at most once per `interval`, and
    /// says what goes wrong with `log`.
    pub(crate) fn start(
        write: impl Fn(&T) -> Result<(), String> + Send + Sync + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
        interval: Duration,
    ) -> Self {
        #[cfg(test)]
        if MANUAL_SAVERS.with(std::cell::Cell::get) {
            return Self::manual(write, log);
        }
        let inner = Arc::new(Inner::new(Box::new(write), Box::new(log), interval));
        let thread = {
            let inner = Arc::clone(&inner);
            thread::Builder::new()
                .name(T::THREAD.to_owned())
                .spawn(move || inner.run())
                .map_err(|error| {
                    log_warn!(
                        "local",
                        "the saver of {what} did not start ({error}); {what} is kept when the \
                         engine stops",
                        what = T::WHAT
                    )
                })
                .ok()
        };
        Self { inner, thread }
    }

    /// A saver with no thread of its own: it writes only when flushed. For the tests.
    #[cfg(test)]
    pub(crate) fn manual(
        write: impl Fn(&T) -> Result<(), String> + Send + Sync + 'static,
        log: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Arc::new(Inner::new(Box::new(write), Box::new(log), SAVE_INTERVAL)),
            thread: None,
        }
    }

    /// Keeps `value` to be written: it replaces any value still waiting. Never touches the disk,
    /// so it may be called under the engine lock.
    pub(crate) fn mark(&self, value: T) {
        self.inner.lock_state().pending = Some(value);
        self.inner.wake.notify_all();
    }

    /// Writes the mix still waiting now, on the caller's thread: when the engine stops and when
    /// the app quits. Never call it under the engine lock.
    pub(crate) fn flush(&self) {
        self.inner.write_pending();
    }

    /// Holds every write off until the pause is dropped, after any write in flight has landed, so
    /// the session and the mix still waiting can be read or replaced together.
    pub(crate) fn pause(&self) -> Paused<'_, T> {
        Paused {
            inner: &self.inner,
            _writing: self
                .inner
                .writing
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        }
    }
}

impl<T: Kept> Drop for Saver<T> {
    fn drop(&mut self) {
        self.inner.lock_state().stop = true;
        self.inner.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log_error!("local", "the saver of {} panicked", T::WHAT);
            }
        }
        self.inner.write_pending();
    }
}

/// The saver held still (see [Saver::pause]).
pub(crate) struct Paused<'a, T: Kept> {
    inner: &'a Inner<T>,
    _writing: MutexGuard<'a, ()>,
}

impl<T: Kept> Paused<'_, T> {
    /// The value marked and not written yet.
    pub(crate) fn pending(&self) -> Option<T> {
        self.inner.lock_state().pending.clone()
    }

    /// Forgets the value waiting: the session now holds it, or a loaded one replaced it.
    pub(crate) fn clear(&self) {
        self.inner.lock_state().pending = None;
    }
}

impl<T: Kept> Inner<T> {
    fn new(write: Box<Write<T>>, log: Box<Log>, interval: Duration) -> Self {
        Self {
            write,
            log,
            interval,
            state: Mutex::new(State::default()),
            wake: Condvar::new(),
            writing: Mutex::new(()),
        }
    }

    // Nothing in the state can be left half changed, so a poisoned lock is used as it is.
    fn lock_state(&self) -> MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The saver's thread: waits for a mix, waits out the interval since the last write, writes.
    fn run(&self) {
        loop {
            {
                let mut state = self.lock_state();
                while state.pending.is_none() && !state.stop {
                    state = self
                        .wake
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
                if let Some(last) = state.last_write {
                    let due = last + self.interval;
                    loop {
                        let now = Instant::now();
                        if state.stop || now >= due {
                            break;
                        }
                        state = self
                            .wake
                            .wait_timeout(state, due - now)
                            .map_or_else(|poisoned| poisoned.into_inner().0, |(state, _)| state);
                    }
                }
                if state.stop {
                    // Dropping the saver writes what is left.
                    return;
                }
            }
            self.write_pending();
        }
    }

    /// Takes the value waiting and writes it; a failed write keeps it waiting unless a newer one
    /// came, and is said once until the failure changes.
    fn write_pending(&self) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(value) = self.lock_state().pending.take() else {
            return;
        };
        let result = (self.write)(&value);
        let mut state = self.lock_state();
        state.last_write = Some(Instant::now());
        match result {
            Ok(()) => {
                if state.failure.take().is_some() {
                    (self.log)(&format!("{} is kept in the session again", T::WHAT));
                }
            }
            Err(error) => {
                if state.failure.as_deref() != Some(error.as_str()) {
                    (self.log)(&format!(
                        "{} plays but was not kept in the session: {error}",
                        T::WHAT
                    ));
                    state.failure = Some(error);
                }
                if state.pending.is_none() {
                    state.pending = Some(value);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn mix(volume_percent: u8) -> SessionMix {
        SessionMix {
            channels: Vec::new(),
            groups: Default::default(),
            volume_percent,
            muted: false,
        }
    }

    type Book = Arc<Mutex<Vec<u8>>>;

    /// A write that records each mix's volume.
    fn recorder() -> (
        Book,
        impl Fn(&SessionMix) -> Result<(), String> + Send + Sync,
    ) {
        let written: Book = Arc::default();
        let book = Arc::clone(&written);
        (written, move |mix: &SessionMix| {
            book.lock().expect("book").push(mix.volume_percent);
            Ok(())
        })
    }

    fn wait_until(done: impl Fn() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(5));
        }
        done()
    }

    #[test]
    fn a_drag_of_many_changes_is_written_a_few_times_and_ends_on_its_last_value() {
        let (written, write) = recorder();
        let saver = LocalMixSaver::start(write, |_| {}, Duration::from_millis(200));

        let started = Instant::now();
        for volume in 0..=100 {
            saver.mark(mix(volume));
            thread::sleep(Duration::from_millis(2));
        }
        let elapsed = started.elapsed();

        assert!(wait_until(|| written
            .lock()
            .expect("book")
            .last()
            .is_some_and(|&last| last == 100)));
        let writes = written.lock().expect("book").clone();
        let bound = 2 + (elapsed.as_millis() / 200) as usize;
        assert!(
            writes.len() <= bound,
            "{} writes for 101 changes over {elapsed:?}: {writes:?}",
            writes.len()
        );
        assert_eq!(writes.last(), Some(&100), "the last value is kept");
    }

    #[test]
    fn dropping_or_flushing_the_saver_writes_what_is_waiting() {
        let (written, write) = recorder();
        let saver = LocalMixSaver::manual(write, |_| {});
        saver.mark(mix(10));
        saver.mark(mix(20));
        saver.flush();
        saver.flush();
        assert_eq!(
            *written.lock().expect("book"),
            vec![20],
            "only the latest, once"
        );

        saver.mark(mix(30));
        drop(saver);
        assert_eq!(*written.lock().expect("book"), vec![20, 30]);

        let (written, write) = recorder();
        let threaded = LocalMixSaver::start(write, |_| {}, Duration::from_secs(60));
        threaded.mark(mix(1));
        assert!(wait_until(|| written.lock().expect("book").len() == 1));
        threaded.mark(mix(2));
        drop(threaded);
        assert_eq!(
            *written.lock().expect("book"),
            vec![1, 2],
            "a stop does not wait out the interval"
        );
    }

    #[test]
    fn savers_started_under_the_manual_guard_write_only_when_flushed() {
        let (written, write) = recorder();
        let manual = manual_savers();
        let saver = LocalMixSaver::start(write, |_| {}, Duration::from_millis(1));
        saver.mark(mix(1));
        thread::sleep(Duration::from_millis(30));
        assert!(
            written.lock().expect("book").is_empty(),
            "no thread writes on its own"
        );
        saver.flush();
        assert_eq!(*written.lock().expect("book"), vec![1]);

        drop(manual);
        let (written, write) = recorder();
        let threaded = LocalMixSaver::start(write, |_| {}, Duration::from_millis(1));
        threaded.mark(mix(2));
        assert!(
            wait_until(|| *written.lock().expect("book") == vec![2]),
            "once the guard is dropped, savers write on their own again"
        );
    }

    #[test]
    fn a_failure_is_said_once_until_it_changes_and_the_mix_is_tried_again() {
        let outcome: Arc<Mutex<Result<(), String>>> =
            Arc::new(Mutex::new(Err("the disk is full".to_owned())));
        let tried: Book = Arc::default();
        let said: Arc<Mutex<Vec<String>>> = Arc::default();
        let saver = {
            let (outcome, tried, said) =
                (Arc::clone(&outcome), Arc::clone(&tried), Arc::clone(&said));
            LocalMixSaver::manual(
                move |mix: &SessionMix| {
                    tried.lock().expect("tried").push(mix.volume_percent);
                    outcome.lock().expect("outcome").clone()
                },
                move |line| said.lock().expect("said").push(line.to_owned()),
            )
        };

        saver.mark(mix(5));
        for _ in 0..4 {
            saver.flush();
        }
        assert_eq!(
            *tried.lock().expect("tried"),
            vec![5, 5, 5, 5],
            "kept and tried again"
        );
        let lines = said.lock().expect("said").clone();
        assert_eq!(lines.len(), 1, "said once: {lines:?}");

        *outcome.lock().expect("outcome") =
            Err("the session was written by a newer MixLink".to_owned());
        saver.flush();
        let lines = said.lock().expect("said").clone();
        assert_eq!(lines.len(), 2, "a new failure is said: {lines:?}");

        saver.mark(mix(6));
        *outcome.lock().expect("outcome") = Ok(());
        saver.flush();
        saver.flush();
        let said = said.lock().expect("said").clone();
        assert_eq!(said.len(), 3, "{said:?}");
        assert!(said[2].contains("again"), "{said:?}");
        assert_eq!(
            *tried.lock().expect("tried"),
            vec![5, 5, 5, 5, 5, 6],
            "the newer mix wins"
        );
    }

    #[test]
    fn a_pause_waits_for_the_write_in_flight_and_holds_the_next_one() {
        let writing = Arc::new(AtomicBool::new(false));
        let release = Arc::new(AtomicBool::new(false));
        let (written, record) = recorder();
        let saver = {
            let (writing, release) = (Arc::clone(&writing), Arc::clone(&release));
            LocalMixSaver::start(
                move |mix: &SessionMix| {
                    writing.store(true, Ordering::SeqCst);
                    while !release.load(Ordering::SeqCst) {
                        thread::sleep(Duration::from_millis(1));
                    }
                    record(mix)
                },
                |_| {},
                Duration::from_millis(1),
            )
        };
        saver.mark(mix(1));
        assert!(wait_until(|| writing.load(Ordering::SeqCst)));

        let pauser = thread::spawn({
            let release = Arc::clone(&release);
            move || {
                thread::sleep(Duration::from_millis(50));
                release.store(true, Ordering::SeqCst);
            }
        });
        let paused = saver.pause();
        assert_eq!(
            *written.lock().expect("book"),
            vec![1],
            "the pause began once the write in flight had landed"
        );
        saver.mark(mix(2));
        thread::sleep(Duration::from_millis(30));
        assert_eq!(paused.pending(), Some(mix(2)));
        assert_eq!(
            written.lock().expect("book").len(),
            1,
            "nothing is written while paused"
        );
        paused.clear();
        drop(paused);
        pauser.join().expect("joined");
        drop(saver);
        assert_eq!(
            *written.lock().expect("book"),
            vec![1],
            "a cleared mix is never written"
        );
    }
}
