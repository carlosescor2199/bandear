//! Mix backups: the musicians' live mixes, kept on their own as they change, so a crash never loses
//! more than the last few seconds of them (see [crate::commands::backups] for when one is taken).
//!
//! Each backup is one JSON file in the `backups` folder of the app's config directory,
//! `backups/mixes-<stamp>.json`, written atomically as the session is (a temporary file flushed to
//! disk, then renamed). The stamp is the UTC time it was taken, to the millisecond:
//! `20261018T190405123Z`. Only the newest [KEPT_BACKUPS] are kept; writing one more drops the
//! oldest. An old one that cannot be dropped (held open) does not stop the new one; the next
//! write tries again. A file holds:
//!
//! ```json
//! {
//!   "version": 1,
//!   "savedAt": 1760814245123,
//!   "musicians": [
//!     {
//!       "key": "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b",
//!       "name": "Ana",
//!       "clientId": "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b",
//!       "address": "192.168.1.30:50000",
//!       "engineer": { "limitPercent": 60, "lockedChannels": [1, 2] },
//!       "mix": {
//!         "channels": [{ "gain": 80, "pan": 50, "muted": false, "solo": false }],
//!         "groups": { "g3f9a0c1d2e4b5a6": 70 },
//!         "volumePercent": 85,
//!         "muted": false
//!       }
//!     }
//!   ]
//! }
//! ```
//!
//! - `key` is the key the desk keeps the musician under (`client:<client_id>` for a phone that
//!   announced one, else `address:<ip:port>`), and a restore finds the musician by it, as a loaded
//!   session does. `mix` is a whole mix as a saved session keeps it ([SessionMix]): never the
//!   phone's own level cap.
//! - `name` is what the desk showed for the musician (their phone's name, else the engineer's
//!   label), `engineer` the engineer's limit and locks for them when they had any. Both are for
//!   the engineer to read: the limit and locks live in the desk's session, which keeps them across
//!   a crash, so a restore puts back the mixes only.
//! - A file that is not a backup this build can read (damaged, or written by a newer BandEar) is
//!   listed with its problem and cannot be restored; it is never changed, and it ages out like any
//!   other.
//! - A stamp is only ever read as exactly the form above, and the file it names must be directly
//!   in the folder (checked on the disk, links resolved), so a stamp never reaches outside it.

use std::fs;
use std::io;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use personal_monitoring::logging::{format_line, Level};
use serde::{Deserialize, Serialize};

use crate::session::{write_atomically, SessionEngineer, SessionMix};

/// The folder of the app config directory the mix backups live in.
pub const BACKUPS_DIRECTORY: &str = "backups";
/// How many backups are kept; writing one more drops the oldest.
pub const KEPT_BACKUPS: usize = 10;
/// The version of the backup files this build writes, and the newest it reads.
pub const BACKUP_VERSION: u32 = 1;

const FILE_PREFIX: &str = "mixes-";
const FILE_EXTENSION: &str = ".json";

/// One backup of every musician's mix.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MixBackup {
    pub version: u32,
    /// When it was taken, in unix milliseconds.
    pub saved_at: u64,
    pub musicians: Vec<BackedUpMusician>,
}

/// One musician in a backup (see the module documentation).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackedUpMusician {
    pub key: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub client_id: Option<String>,
    #[serde(default)]
    pub address: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub engineer: Option<SessionEngineer>,
    pub mix: SessionMix,
}

impl BackedUpMusician {
    /// What the window calls them: their name, else their address, else "a phone".
    pub fn shown_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.address.clone())
            .unwrap_or_else(|| "a phone".to_owned())
    }
}

/// One backup as the window lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MixBackupSummary {
    /// The backup's stamp, what a restore names it by.
    pub stamp: String,
    /// When it was taken, in unix milliseconds; null for a file that cannot be read.
    pub saved_at: Option<u64>,
    /// How many musicians' mixes it keeps.
    pub musicians: usize,
    /// What the window calls each of them, in the order kept.
    pub names: Vec<String>,
    /// Why it cannot be restored (it cannot be read, or a newer BandEar wrote it); null when it can.
    pub problem: Option<String>,
}

impl MixBackupSummary {
    pub(crate) fn of(stamp: &str, backup: &MixBackup) -> Self {
        Self {
            stamp: stamp.to_owned(),
            saved_at: Some(backup.saved_at),
            musicians: backup.musicians.len(),
            names: backup
                .musicians
                .iter()
                .map(BackedUpMusician::shown_name)
                .collect(),
            problem: None,
        }
    }
}

/// A backup written: its stamp, and why the oldest past the kept number could not be dropped when
/// they could not (the backup is written all the same; the next write tries again).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
    pub stamp: String,
    pub prune_problem: Option<String>,
}

type Remover = Box<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// The mix backups in one folder.
pub struct BackupStore {
    directory: PathBuf,
    keep: usize,
    /// How an old backup is dropped: [fs::remove_file], or a failing one in the tests.
    remove: Remover,
    /// Held across every write, so two writes never take one stamp or prune each other's file.
    files: Mutex<()>,
}

impl BackupStore {
    /// The backups kept in `directory` (created with the first one), the newest `keep` of them.
    pub fn new(directory: PathBuf, keep: usize) -> Self {
        Self {
            directory,
            keep,
            remove: Box::new(|path| fs::remove_file(path)),
            files: Mutex::new(()),
        }
    }

    /// The same store, dropping old backups with `remove`: for the tests.
    #[cfg(test)]
    pub(crate) fn with_remover(
        mut self,
        remove: impl Fn(&Path) -> io::Result<()> + Send + Sync + 'static,
    ) -> Self {
        self.remove = Box::new(remove);
        self
    }

    /// Writes `backup` as the one taken at `time` and drops the oldest past the kept number;
    /// answers its stamp. A stamp already taken (two in one millisecond) moves on by a
    /// millisecond. A write that fails leaves no file and drops nothing; old backups that cannot be
    /// dropped leave the written one written, with why in [Written::prune_problem].
    pub fn write(&self, backup: &MixBackup, time: SystemTime) -> Result<Written, String> {
        let _files = self.lock();
        let mut taken_at = time;
        let mut stamp = stamp_of(taken_at);
        while self.path_of(&stamp).exists() {
            taken_at += Duration::from_millis(1);
            stamp = stamp_of(taken_at);
        }
        write_atomically(&self.path_of(&stamp), backup)
            .map_err(|error| format!("the mixes were not backed up: {error}"))?;
        // The backup is on the disk whatever happens to the old ones: a backup that could not drop
        // them still counts as written, and the next write tries them again.
        let prune_problem = self.prune().err();
        Ok(Written {
            stamp,
            prune_problem,
        })
    }

    /// Every backup, the newest first; one that cannot be restored is listed with its problem.
    /// No folder yet is no backup.
    pub fn list(&self) -> Result<Vec<MixBackupSummary>, String> {
        let _files = self.lock();
        let mut stamps = self.stamps()?;
        stamps.reverse();
        Ok(stamps
            .iter()
            .map(|stamp| match self.parse(stamp) {
                Ok(backup) => MixBackupSummary::of(stamp, &backup),
                Err(problem) => MixBackupSummary {
                    stamp: stamp.clone(),
                    saved_at: None,
                    musicians: 0,
                    names: Vec::new(),
                    problem: Some(problem),
                },
            })
            .collect())
    }

    /// Reads the backup `stamp` for a restore: only a stamp of the right form naming a file
    /// directly in the folder. One that cannot be read, or that a newer BandEar wrote, is an
    /// error saying so.
    pub fn read(&self, stamp: &str) -> Result<MixBackup, String> {
        let _files = self.lock();
        let refused = || {
            format!(
                "`{stamp}` is not a mix backup in {}",
                self.directory.display()
            )
        };
        if !is_stamp(stamp) {
            return Err(refused());
        }
        // Checked on the disk too: the file is directly in the folder, whatever links lie between.
        let folder = fs::canonicalize(&self.directory).map_err(|_| refused())?;
        let found = fs::canonicalize(self.path_of(stamp)).map_err(|_| refused())?;
        if found.parent() != Some(folder.as_path()) || !found.is_file() {
            return Err(refused());
        }
        self.parse(stamp)
            .map_err(|problem| format!("the mix backup `{stamp}` cannot be restored: {problem}"))
    }

    /// The stamps of the backups in the folder, the oldest first.
    fn stamps(&self) -> Result<Vec<String>, String> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "the mix backups in {} could not be listed: {error}",
                    self.directory.display()
                ))
            }
        };
        let mut stamps: Vec<String> = entries
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let stamp = name
                    .strip_prefix(FILE_PREFIX)?
                    .strip_suffix(FILE_EXTENSION)?;
                is_stamp(stamp).then(|| stamp.to_owned())
            })
            .collect();
        // The stamp's form sorts as the time it names.
        stamps.sort();
        Ok(stamps)
    }

    /// Drops the oldest backups past the kept number.
    fn prune(&self) -> Result<(), String> {
        let stamps = self.stamps()?;
        let surplus = stamps.len().saturating_sub(self.keep);
        for stamp in &stamps[..surplus] {
            match (self.remove)(&self.path_of(stamp)) {
                Err(error) if error.kind() != ErrorKind::NotFound => {
                    return Err(format!(
                        "the old mix backup {stamp} could not be dropped: {error}"
                    ))
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The backup `stamp` as written, or why it cannot be read.
    fn parse(&self, stamp: &str) -> Result<MixBackup, String> {
        let bytes = fs::read(self.path_of(stamp)).map_err(|error| match error.kind() {
            ErrorKind::NotFound => "there is no such backup".to_owned(),
            _ => format!("it could not be read: {error}"),
        })?;
        let backup: MixBackup = serde_json::from_slice(&bytes)
            .map_err(|error| format!("it is damaged and cannot be read ({error})"))?;
        if backup.version > BACKUP_VERSION {
            return Err(format!(
                "a newer BandEar wrote it (version {}; this one reads up to {BACKUP_VERSION})",
                backup.version
            ));
        }
        Ok(backup)
    }

    fn path_of(&self, stamp: &str) -> PathBuf {
        self.directory
            .join(format!("{FILE_PREFIX}{stamp}{FILE_EXTENSION}"))
    }

    // Nothing is kept behind the lock, so a poisoned one guards nothing broken.
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The stamp of a backup taken at `time`: UTC to the millisecond, `20261018T190405123Z`.
pub fn stamp_of(time: SystemTime) -> String {
    // The log's own clock text, `2026-10-18T19:04:05.123Z`, with its separators left out.
    let line = format_line(time, Level::Info, "", format_args!(""));
    line.split(' ')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect()
}

/// Whether `text` is a stamp as [stamp_of] writes one: 8 digits, `T`, 9 digits, `Z`. Nothing else
/// is ever joined to the folder's path.
fn is_stamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 19
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..18].iter().all(u8::is_ascii_digit)
        && bytes[18] == b'Z'
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::time::UNIX_EPOCH;

    fn scratch(test: &str) -> PathBuf {
        let directory = std::env::temp_dir().join("bandear-backup-tests").join(test);
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    fn at(millis: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(millis)
    }

    fn mix(volume_percent: u8) -> SessionMix {
        SessionMix {
            channels: Vec::new(),
            groups: BTreeMap::new(),
            volume_percent,
            muted: false,
        }
    }

    fn backup(saved_at: u64, volumes: &[(&str, u8)]) -> MixBackup {
        MixBackup {
            version: BACKUP_VERSION,
            saved_at,
            musicians: volumes
                .iter()
                .map(|(name, volume)| BackedUpMusician {
                    key: format!("address:{name}"),
                    name: Some((*name).to_owned()),
                    client_id: None,
                    address: None,
                    engineer: None,
                    mix: mix(*volume),
                })
                .collect(),
        }
    }

    #[test]
    fn a_stamp_is_the_utc_time_to_the_millisecond() {
        assert_eq!(stamp_of(UNIX_EPOCH), "19700101T000000000Z");
        // 2026-10-18 19:04:05.123 UTC.
        assert_eq!(stamp_of(at(1_792_350_245_123)), "20261018T190405123Z");
    }

    #[test]
    fn a_backup_is_written_atomically_and_read_back_whole() {
        let directory = scratch("atomic");
        let store = BackupStore::new(directory.clone(), KEPT_BACKUPS);
        let kept = MixBackup {
            musicians: vec![BackedUpMusician {
                key: "client:0b6f".to_owned(),
                name: Some("Ana".to_owned()),
                client_id: Some("0b6f".to_owned()),
                address: Some("192.168.1.30:50000".to_owned()),
                engineer: Some(SessionEngineer {
                    limit_percent: 60,
                    locked_channels: vec![1, 2],
                }),
                mix: mix(85),
            }],
            ..backup(1_000, &[])
        };

        let stamp = store.write(&kept, at(1_000)).expect("written").stamp;

        assert_eq!(stamp, "19700101T000001000Z");
        let names: Vec<String> = fs::read_dir(&directory)
            .expect("folder")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            names,
            vec!["mixes-19700101T000001000Z.json".to_owned()],
            "the file alone: no temporary file is left"
        );
        assert_eq!(store.read(&stamp).expect("readable"), kept);
    }

    #[test]
    fn a_write_that_fails_leaves_nothing() {
        let directory = scratch("blocked");
        fs::create_dir_all(directory.parent().expect("parent")).expect("parent folder");
        fs::write(&directory, b"a file where the folder goes").expect("blocking file");
        let store = BackupStore::new(directory.clone(), KEPT_BACKUPS);

        assert!(store.write(&backup(1, &[("Ana", 50)]), at(1)).is_err());
        assert!(directory.is_file(), "nothing was made in its place");
        let _ = fs::remove_file(&directory);
    }

    #[test]
    fn only_the_newest_ten_are_kept_and_listed_newest_first() {
        let store = BackupStore::new(scratch("rotation"), KEPT_BACKUPS);
        for index in 0..12u64 {
            let taken = 1_000 * (index + 1);
            store
                .write(&backup(taken, &[("Ana", index as u8)]), at(taken))
                .expect("written");
        }

        let listed = store.list().expect("listed");

        assert_eq!(listed.len(), KEPT_BACKUPS);
        assert_eq!(listed[0].saved_at, Some(12_000), "the newest first");
        assert_eq!(listed[9].saved_at, Some(3_000), "the two oldest are gone");
        assert_eq!(listed[0].names, vec!["Ana".to_owned()]);
        assert_eq!(listed[0].musicians, 1);
        assert!(store.read(&stamp_of(at(1_000))).is_err());
    }

    /// A store keeping `keep` whose old backups cannot be dropped while `held` is set.
    pub(crate) fn store_with_held_files(
        directory: PathBuf,
        keep: usize,
        held: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> BackupStore {
        let held = std::sync::Arc::clone(held);
        BackupStore::new(directory, keep).with_remover(move |path| {
            if held.load(std::sync::atomic::Ordering::SeqCst) {
                Err(io::Error::new(ErrorKind::PermissionDenied, "held open"))
            } else {
                fs::remove_file(path)
            }
        })
    }

    #[test]
    fn a_backup_is_written_when_the_old_ones_cannot_be_dropped_and_they_are_dropped_later() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let held = std::sync::Arc::new(AtomicBool::new(true));
        let store = store_with_held_files(scratch("prune-fails"), 2, &held);

        for index in 0..4u64 {
            let taken = 1_000 * (index + 1);
            let written = store
                .write(&backup(taken, &[("Ana", index as u8)]), at(taken))
                .expect("written, though the old ones stay");
            assert_eq!(written.stamp, stamp_of(at(taken)));
            assert_eq!(
                written.prune_problem.is_some(),
                index >= 2,
                "only a write past the kept number has old ones to drop"
            );
        }
        assert_eq!(
            store.list().expect("listed").len(),
            4,
            "nothing dropped yet"
        );

        held.store(false, Ordering::SeqCst);
        let written = store
            .write(&backup(5_000, &[("Ana", 5)]), at(5_000))
            .expect("written");

        assert_eq!(written.prune_problem, None);
        let listed = store.list().expect("listed");
        assert_eq!(
            listed.len(),
            2,
            "the next write drops every one past the kept number"
        );
        assert_eq!(listed[0].saved_at, Some(5_000));
        assert_eq!(listed[1].saved_at, Some(4_000));
    }

    #[test]
    fn two_backups_in_one_millisecond_both_keep_a_stamp_of_their_own() {
        let store = BackupStore::new(scratch("same-millisecond"), KEPT_BACKUPS);
        let first = store
            .write(&backup(5, &[("Ana", 1)]), at(5))
            .expect("written")
            .stamp;
        let second = store
            .write(&backup(5, &[("Ana", 2)]), at(5))
            .expect("written")
            .stamp;

        assert_ne!(first, second);
        assert_eq!(
            store.read(&second).expect("readable").musicians[0]
                .mix
                .volume_percent,
            2
        );
    }

    #[test]
    fn a_damaged_or_newer_backup_is_listed_with_its_problem_and_never_restored() {
        let directory = scratch("damaged");
        let store = BackupStore::new(directory.clone(), KEPT_BACKUPS);
        let good = store
            .write(&backup(2_000, &[("Ana", 60)]), at(2_000))
            .expect("written")
            .stamp;
        fs::write(
            directory.join("mixes-19700101T000003000Z.json"),
            b"{ \"version\": 1, \"musicians\": [ trunc",
        )
        .expect("damaged file");
        let newer = MixBackup {
            version: BACKUP_VERSION + 1,
            ..backup(4_000, &[("Ana", 70)])
        };
        fs::write(
            directory.join("mixes-19700101T000004000Z.json"),
            serde_json::to_vec(&newer).expect("serializes"),
        )
        .expect("newer file");
        // Not a backup's name: never listed.
        fs::write(directory.join("notes.json"), b"{}").expect("other file");
        fs::write(directory.join("mixes-yesterday.json"), b"{}").expect("other file");

        let listed = store.list().expect("the list does not fail");

        let stamps: Vec<&str> = listed.iter().map(|row| row.stamp.as_str()).collect();
        assert_eq!(
            stamps,
            vec!["19700101T000004000Z", "19700101T000003000Z", good.as_str()]
        );
        assert!(listed[0]
            .problem
            .as_deref()
            .is_some_and(|why| why.contains("newer")));
        assert!(listed[1].problem.is_some());
        assert_eq!(listed[1].musicians, 0);
        assert_eq!(listed[1].saved_at, None);
        assert_eq!(listed[2].problem, None);
        assert!(store.read("19700101T000003000Z").is_err());
        assert!(store.read("19700101T000004000Z").is_err());
        assert!(store.read(&good).is_ok());
    }

    #[test]
    fn a_stamp_outside_the_backups_folder_is_refused() {
        let directory = scratch("outside");
        let store = BackupStore::new(directory.clone(), KEPT_BACKUPS);
        store
            .write(&backup(1_000, &[("Ana", 60)]), at(1_000))
            .expect("written");
        // A backup-shaped file one folder up.
        fs::write(
            directory
                .parent()
                .expect("parent")
                .join("mixes-19700101T000001000Z.json"),
            serde_json::to_vec(&backup(1_000, &[("Eve", 1)])).expect("serializes"),
        )
        .expect("outside file");

        for stamp in [
            "../mixes-19700101T000001000Z",
            "..\\19700101T000001000Z",
            "19700101T000001000Z/../../x",
            "",
            "19700101T000001000",
            "1970-01-01T00:00:01.000Z",
            "C:\\backups\\19700101T000001000Z",
            "/19700101T000001000Z",
        ] {
            let refused = store.read(stamp).expect_err(stamp);
            assert!(refused.contains("not a mix backup"), "{stamp}: {refused}");
        }
        assert!(
            store.read("19700101T000009000Z").is_err(),
            "a stamp with no file"
        );
        assert_eq!(
            store
                .read("19700101T000001000Z")
                .expect("its own")
                .musicians[0]
                .name
                .as_deref(),
            Some("Ana")
        );
    }
}
