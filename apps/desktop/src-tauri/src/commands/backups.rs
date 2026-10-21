//! The musicians' mixes backed up as they change, and put back from a backup: the
//! `list_mix_backups` and `restore_mix_backup` commands (see [crate::backups] for the files).
//!
//! The desk's session keeps no musician's mix (a phone keeps its own and sends it when it
//! connects), and a named session holds the mixes only when the engineer saves one. So that a
//! crash never costs more than a few seconds of the engineer's work on the musicians' mixes, every
//! change of one is noted ([MixBackups::note_change], by the event pump as it announces it) and a
//! backup of every musician's mix is taken once the mixes have been still for [BACKUP_QUIET], or
//! at the latest [BACKUP_MAX_WAIT] after the first change while they keep changing ([Debounce]).
//! Stopping the engine and quitting the app take one too.
//!
//! A backup is read from the engine (in memory, under the engine lock, as a status is) and then
//! written by a thread of its own: no disk I/O on the engine thread, under the engine lock or on
//! the event pump. One with no musician, or with the same musicians and mixes as the last one
//! written, is not written. A write that fails is logged, once until the failure changes.
//!
//! Restoring is the engineer's choice: a phone's own mix when it connects is never overridden on
//! its own. A restore pushes each backed-up mix through the desk's path (as a loaded session does)
//! to the musician the engine lists under the backup's key, so their phone is sent it at once.
//! A musician whose phone is not connected, or who is not here, is skipped and said, not queued:
//! when their phone connects its own mix wins, as always.

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use personal_monitoring::{log_debug, log_error, log_info, log_warn};

use super::{
    phone_id, push_saved_mixes, row_key, session_mix_of, DeskReader, DeskView, DesktopState,
    SessionEngine,
};
use crate::backups::{BackedUpMusician, BackupStore, MixBackup, MixBackupSummary, BACKUP_VERSION};
use crate::dto::MixRestoredDto;
use crate::session::{Session, SessionMix, SessionStore};

/// How long the mixes stay still before a backup is taken.
pub(crate) const BACKUP_QUIET: Duration = Duration::from_secs(3);
/// The longest a change waits for its backup while the mixes keep changing.
pub(crate) const BACKUP_MAX_WAIT: Duration = Duration::from_secs(20);

/// When a backup is due, given when the mixes changed: the clock is passed in, so it is tested
/// without waiting.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Debounce {
    quiet: Duration,
    max_wait: Duration,
    /// The first change not backed up yet, and the last one.
    first: Option<Instant>,
    last: Option<Instant>,
}

impl Debounce {
    pub(crate) fn new(quiet: Duration, max_wait: Duration) -> Self {
        Self {
            quiet,
            max_wait,
            first: None,
            last: None,
        }
    }

    /// A mix changed at `now`.
    pub(crate) fn note(&mut self, now: Instant) {
        self.first.get_or_insert(now);
        self.last = Some(now);
    }

    /// Whether a backup is due at `now`: the mixes changed and have been still for the quiet time,
    /// or the first change has waited the longest it may.
    pub(crate) fn due(&self, now: Instant) -> bool {
        match (self.first, self.last) {
            (Some(first), Some(last)) => {
                now.saturating_duration_since(last) >= self.quiet
                    || now.saturating_duration_since(first) >= self.max_wait
            }
            _ => false,
        }
    }

    /// The changes waiting are being backed up.
    pub(crate) fn clear(&mut self) {
        self.first = None;
        self.last = None;
    }
}

/// The mix backups: when the next is due, and the thread that writes them (see the module
/// documentation).
pub(crate) struct MixBackups {
    store: Arc<BackupStore>,
    debounce: Mutex<Debounce>,
    inner: Arc<Writer>,
    thread: Option<JoinHandle<()>>,
}

struct Writer {
    store: Arc<BackupStore>,
    state: Mutex<WriterState>,
    wake: Condvar,
    /// Held across taking the backup waiting and writing it, so backups land in the order taken.
    writing: Mutex<()>,
}

#[derive(Default)]
struct WriterState {
    /// The latest backup taken and not written yet.
    pending: Option<MixBackup>,
    stop: bool,
    /// The musicians of the last backup written, so an unchanged one is not written again.
    last: Option<Vec<BackedUpMusician>>,
    /// The failure said last; said again only once it changes.
    failure: Option<String>,
    /// Why the old backups could not be dropped, said last; said again only once it changes.
    prune_failure: Option<String>,
}

impl MixBackups {
    /// Backups kept in `store`, written by a thread of its own.
    pub(crate) fn start(store: BackupStore) -> Self {
        let mut backups = Self::manual(store);
        let inner = Arc::clone(&backups.inner);
        backups.thread = thread::Builder::new()
            .name("bandear-mix-backups".to_owned())
            .spawn(move || inner.run())
            .map_err(|error| {
                log_warn!(
                    "backup",
                    "the mix backup writer did not start ({error}); the mixes are backed up when \
                     the engine stops and when the app quits"
                )
            })
            .ok();
        backups
    }

    /// Backups kept in `store` with no thread of their own: written only when flushed. For the
    /// tests, and the fallback when the thread does not start.
    pub(crate) fn manual(store: BackupStore) -> Self {
        let store = Arc::new(store);
        Self {
            inner: Arc::new(Writer {
                store: Arc::clone(&store),
                state: Mutex::new(WriterState::default()),
                wake: Condvar::new(),
                writing: Mutex::new(()),
            }),
            store,
            debounce: Mutex::new(Debounce::new(BACKUP_QUIET, BACKUP_MAX_WAIT)),
            thread: None,
        }
    }

    /// A musician's mix changed at `now`. No disk I/O.
    pub(crate) fn note_change(&self, now: Instant) {
        self.lock_debounce().note(now);
    }

    /// Whether a backup is due at `now`; when it is, the changes waiting are taken by it.
    pub(crate) fn take_due(&self, now: Instant) -> bool {
        let mut debounce = self.lock_debounce();
        let due = debounce.due(now);
        if due {
            debounce.clear();
        }
        due
    }

    /// Hands `backup` to the writer, in place of one still waiting; the changes waiting are in it.
    /// No disk I/O, so it may be called under the engine lock.
    pub(crate) fn submit(&self, backup: MixBackup) {
        self.lock_debounce().clear();
        self.inner.lock_state().pending = Some(backup);
        self.inner.wake.notify_all();
    }

    /// Writes the backup still waiting now, on the caller's thread: when the app quits. Never
    /// under the engine lock.
    pub(crate) fn flush(&self) {
        self.inner.write_pending();
    }

    pub(crate) fn list(&self) -> Result<Vec<MixBackupSummary>, String> {
        self.store.list()
    }

    pub(crate) fn read(&self, stamp: &str) -> Result<MixBackup, String> {
        self.store.read(stamp)
    }

    // Nothing in the schedule can be left half changed, so a poisoned lock is used as it is.
    fn lock_debounce(&self) -> MutexGuard<'_, Debounce> {
        self.debounce.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for MixBackups {
    fn drop(&mut self) {
        self.inner.lock_state().stop = true;
        self.inner.wake.notify_all();
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log_error!("backup", "the mix backup writer panicked");
            }
        }
        self.inner.write_pending();
    }
}

impl Writer {
    fn lock_state(&self) -> MutexGuard<'_, WriterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The writer's thread: waits for a backup and writes it.
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
                if state.stop {
                    // Dropping the backups writes what is left.
                    return;
                }
            }
            self.write_pending();
        }
    }

    /// Takes the backup waiting and writes it, unless it holds what the last one written held.
    fn write_pending(&self) {
        let _writing = self.writing.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(backup) = self.lock_state().pending.take() else {
            return;
        };
        if self.lock_state().last.as_ref() == Some(&backup.musicians) {
            return;
        }
        let result = self.store.write(&backup, SystemTime::now());
        let mut state = self.lock_state();
        match result {
            Ok(written) => {
                log_debug!(
                    "backup",
                    "backed up {} musicians' mixes as {}",
                    backup.musicians.len(),
                    written.stamp
                );
                if state.failure.take().is_some() {
                    log_info!("backup", "the musicians' mixes are backed up again");
                }
                match written.prune_problem {
                    Some(problem) => {
                        if state.prune_failure.as_deref() != Some(problem.as_str()) {
                            log_warn!("backup", "{problem}; tried again with the next backup");
                            state.prune_failure = Some(problem);
                        }
                    }
                    None => {
                        if state.prune_failure.take().is_some() {
                            log_info!("backup", "the old mix backups are dropped again");
                        }
                    }
                }
                state.last = Some(backup.musicians);
            }
            Err(error) => {
                if state.failure.as_deref() != Some(error.as_str()) {
                    log_warn!("backup", "{error}");
                    state.failure = Some(error);
                }
            }
        }
    }
}

/// The backup of every musician `desk` lists, as the desk shows them: their key, the name the
/// window shows (their phone's, else the engineer's label in `session`), their client id and
/// address, the engineer's limit and locks for them, and their whole mix. `None` with nobody to
/// back up.
pub(super) fn backup_of(
    desk: &DeskView,
    session: &SessionStore,
    saved_at: u64,
) -> Option<MixBackup> {
    if desk.musicians.is_empty() {
        return None;
    }
    let musicians = desk
        .musicians
        .iter()
        .map(|row| {
            let address = row.address.to_string();
            let client_id = phone_id(row.client_id.as_deref());
            BackedUpMusician {
                key: row_key(row),
                name: row
                    .name
                    .clone()
                    .or_else(|| session.label_for(client_id, &address)),
                client_id: client_id.map(str::to_owned),
                engineer: session.engineer_for(client_id, &address),
                address: Some(address),
                mix: session_mix_of(&row.mix, desk),
            }
        })
        .collect();
    Some(MixBackup {
        version: BACKUP_VERSION,
        saved_at,
        musicians,
    })
}

/// The backup of the running engine's musicians now, read under the engine lock and handed back
/// after it is released; `None` with no engine, or nobody to back up.
fn current_backup(state: &DesktopState) -> Option<MixBackup> {
    let desk = state.engine.lock().ok()?.as_ref().map(DeskReader::desk)?;
    backup_of(&desk, &state.session, unix_millis_now())
}

fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

/// The event pump's turn, on each pass: `changed` says whether a musician's mix change was just
/// announced. When a backup is due it is read from the engine and handed to the writer.
pub fn tick(state: &DesktopState, changed: bool, now: Instant) {
    if changed {
        state.mix_backups.note_change(now);
    }
    if state.mix_backups.take_due(now) {
        if let Some(backup) = current_backup(state) {
            state.mix_backups.submit(backup);
        }
    }
}

/// Takes a backup of the mixes in `desk`, the engine about to stop (read under the engine lock by
/// the caller), for the writer: when the engine stops.
pub(super) fn back_up_desk(state: &DesktopState, desk: &DeskView) {
    if let Some(backup) = backup_of(desk, &state.session, unix_millis_now()) {
        state.mix_backups.submit(backup);
    }
}

/// Takes a backup of the running engine's mixes and writes it now, with any still waiting: when
/// the app quits. Never under the engine lock.
pub fn flush_mix_backup(state: &DesktopState) {
    if let Some(backup) = current_backup(state) {
        state.mix_backups.submit(backup);
    }
    state.mix_backups.flush();
}

/// The mix backups, the newest first, each with why it cannot be restored when it cannot (see
/// [crate::backups]).
pub fn list_mix_backups(state: &DesktopState) -> Result<Vec<MixBackupSummary>, String> {
    state.mix_backups.list()
}

/// Puts the mixes of the backup `stamp` back on the musicians who are connected now, through the
/// desk's path, so each phone is sent its mix at once, and answers who got theirs and who was
/// skipped and why ([MixRestoredDto]). A stamp that names no backup in the backups folder, a
/// backup that cannot be read, or no engine running is an error, and nothing changed.
pub fn restore_mix_backup(state: &DesktopState, stamp: String) -> Result<MixRestoredDto, String> {
    let backup = state.mix_backups.read(&stamp)?;
    let outcome = restore_with(&state.engine, &state.session.current(), &stamp, backup)?;
    log_info!(
        "desk",
        "restore_mix_backup: {stamp} restored={} skipped={}",
        outcome.restored.len(),
        outcome.skipped.len()
    );
    Ok(outcome)
}

/// [restore_mix_backup] of `backup`, read as `stamp`, over any engine slot. A skipped mix is named
/// by the label `session` keeps for the musician, else by the name the backup kept.
pub(super) fn restore_with<E: SessionEngine>(
    engine: &Mutex<Option<E>>,
    session: &Session,
    stamp: &str,
    backup: MixBackup,
) -> Result<MixRestoredDto, String> {
    let mut guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard.as_mut().ok_or_else(|| {
        "the engine is not running: a backup is put back on the musicians who are connected"
            .to_owned()
    })?;
    let mixes: BTreeMap<String, SessionMix> = backup
        .musicians
        .iter()
        .map(|musician| (musician.key.clone(), musician.mix.clone()))
        .collect();
    let desk = engine.desk();
    let (restored, mut skipped) = push_saved_mixes(engine, &desk, session, &mixes);
    drop(guard);
    // Named as the backup knew them when the desk's session has no label for them.
    for mix in &mut skipped {
        if mix.label.is_none() {
            mix.label = backup
                .musicians
                .iter()
                .find(|musician| musician.key == mix.key)
                .and_then(|musician| musician.name.clone());
        }
    }
    Ok(MixRestoredDto {
        backup: MixBackupSummary::of(stamp, &backup),
        restored,
        skipped,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Duration = Duration::from_secs(3);
    const MAX_WAIT: Duration = Duration::from_secs(20);

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    #[test]
    fn a_backup_is_due_once_the_mixes_have_been_still_for_the_quiet_time() {
        let start = Instant::now();
        let mut debounce = Debounce::new(QUIET, MAX_WAIT);
        assert!(!debounce.due(start), "nothing changed, nothing is due");

        debounce.note(start);
        assert!(!debounce.due(start + seconds(1)));
        // A second change restarts the quiet time.
        debounce.note(start + seconds(2));
        assert!(!debounce.due(start + seconds(4)));
        assert!(debounce.due(start + seconds(5)));

        debounce.clear();
        assert!(!debounce.due(start + seconds(60)), "taken: nothing waits");
    }

    #[test]
    fn under_constant_change_a_backup_is_due_at_most_the_longest_wait_after_the_first() {
        let start = Instant::now();
        let mut debounce = Debounce::new(QUIET, MAX_WAIT);
        let mut due_at = None;
        for second in 0..60 {
            let now = start + seconds(second);
            debounce.note(now);
            if debounce.due(now) {
                due_at = Some(second);
                break;
            }
        }
        assert_eq!(
            due_at,
            Some(20),
            "never still for 3 s, so the longest wait decides"
        );
    }

    #[test]
    fn take_due_hands_the_changes_to_one_backup_and_a_submit_clears_them_too() {
        let store = BackupStore::new(std::env::temp_dir().join("bandear-backup-due"), 10);
        let backups = MixBackups::manual(store);
        let start = Instant::now();
        backups.note_change(start);
        assert!(!backups.take_due(start + seconds(1)));
        assert!(backups.take_due(start + seconds(3)));
        assert!(!backups.take_due(start + seconds(10)), "taken once");

        backups.note_change(start + seconds(11));
        backups.submit(MixBackup {
            version: BACKUP_VERSION,
            saved_at: 0,
            musicians: Vec::new(),
        });
        assert!(
            !backups.take_due(start + seconds(30)),
            "the backup submitted holds that change"
        );
    }

    fn backed_up(volume_percent: u8) -> MixBackup {
        MixBackup {
            version: BACKUP_VERSION,
            saved_at: 1,
            musicians: vec![BackedUpMusician {
                key: "address:192.168.1.40:50000".to_owned(),
                name: Some("Rack".to_owned()),
                client_id: None,
                address: Some("192.168.1.40:50000".to_owned()),
                engineer: None,
                mix: crate::session::SessionMix {
                    channels: Vec::new(),
                    groups: Default::default(),
                    volume_percent,
                    muted: false,
                },
            }],
        }
    }

    #[test]
    fn the_writer_writes_the_latest_backup_once_and_skips_an_unchanged_one() {
        let directory = std::env::temp_dir()
            .join("bandear-backup-tests")
            .join("writer");
        let _ = std::fs::remove_dir_all(&directory);
        let backups = MixBackups::manual(BackupStore::new(directory.clone(), 10));

        backups.submit(backed_up(10));
        backups.submit(backed_up(20));
        backups.flush();
        backups.flush();
        let listed = backups.list().expect("listed");
        assert_eq!(listed.len(), 1, "only the latest, once");
        let stamp = listed[0].stamp.clone();
        assert_eq!(
            backups.read(&stamp).expect("readable").musicians[0]
                .mix
                .volume_percent,
            20
        );

        backups.submit(backed_up(20));
        backups.flush();
        assert_eq!(
            backups.list().expect("listed").len(),
            1,
            "unchanged: not again"
        );

        backups.submit(backed_up(30));
        drop(backups);
        let reopened = MixBackups::manual(BackupStore::new(directory, 10));
        assert_eq!(
            reopened.list().expect("listed").len(),
            2,
            "dropping the backups writes the one waiting"
        );
    }

    #[test]
    fn a_backup_whose_old_ones_cannot_be_dropped_still_counts_as_written() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let directory = std::env::temp_dir()
            .join("bandear-backup-tests")
            .join("writer-prune-fails");
        let _ = std::fs::remove_dir_all(&directory);
        let held = Arc::new(AtomicBool::new(true));
        let store = crate::backups::tests::store_with_held_files(directory, 1, &held);
        let backups = MixBackups::manual(store);

        backups.submit(backed_up(10));
        backups.flush();
        backups.submit(backed_up(20));
        backups.flush();
        assert_eq!(
            backups.list().expect("listed").len(),
            2,
            "the old one stays"
        );

        // The same mixes again: written already, so not again, though the old one still stays.
        backups.submit(backed_up(20));
        backups.flush();
        backups.submit(backed_up(20));
        backups.flush();
        assert_eq!(
            backups.list().expect("listed").len(),
            2,
            "an unchanged backup is not written again while pruning fails"
        );

        held.store(false, Ordering::SeqCst);
        backups.submit(backed_up(30));
        backups.flush();
        assert_eq!(
            backups.list().expect("listed").len(),
            1,
            "pruned again once it can be"
        );
    }

    #[test]
    fn the_writer_thread_writes_on_its_own() {
        let directory = std::env::temp_dir()
            .join("bandear-backup-tests")
            .join("writer-thread");
        let _ = std::fs::remove_dir_all(&directory);
        let backups = MixBackups::start(BackupStore::new(directory, 10));
        backups.submit(backed_up(40));
        let deadline = Instant::now() + seconds(3);
        while backups.list().map_or(0, |listed| listed.len()) == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(backups.list().expect("listed").len(), 1);
    }
}
