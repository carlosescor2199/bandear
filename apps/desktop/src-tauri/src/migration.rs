//! Brings a MixLink desk's data over to BandEar on its first start.
//!
//! BandEar was called MixLink, and its Tauri identifier was `com.mixlink.desktop`. Tauri names the
//! folders it gives an app after the app's identifier, so the renamed desk starts with empty
//! folders beside the old ones: `%APPDATA%\com.bandear.desktop` beside
//! `%APPDATA%\com.mixlink.desktop` on Windows, the same under `~/Library/Application Support` on
//! macOS and `~/.config` on Linux. [bring_over] runs at start, before the session is opened:
//!
//! - It runs only when this desk's config folder has no `session.json` and no [MIGRATION_MARKER],
//!   and the old desk's config folder (the sibling named [LEGACY_IDENTIFIER]) exists.
//! - It copies what the old desk kept there: the legacy labels (`musicians.json`), the named
//!   sessions (`sessions/`), the mix backups (`backups/`) and, last, `session.json`. Each file goes
//!   through a temporary file beside it, and a file already in the new folder is never replaced.
//!   Anything else in the old folder (a damaged file set aside, say) stays where it is, and so do
//!   the logs, which are in another folder.
//! - The old folders are only read: nothing in them is changed, moved or removed.
//! - Takes are never copied. When the old desk recorded into its default folder (the Music folder +
//!   `MixLink\Recordings`, or where it fell back to; the session's `recording.folder` is null), that
//!   folder holds takes, and this desk's default folder does not exist yet, the migrated session's
//!   `recording.folder` names the old folder explicitly, so every take stays listed and new takes
//!   go beside them. The engineer can choose another folder at any time.
//! - When it finished without a failure it writes [MIGRATION_MARKER], so it runs once even for a
//!   desk that had no session. After a failure it runs again on the next start and copies what is
//!   still missing, never replacing what it copied before.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use personal_monitoring::{log_info, log_warn};
use serde_json::{Map, Value};

use crate::backups::BACKUPS_DIRECTORY;
use crate::commands::recording::{default_recordings_folder_for, TAKE_FILE};
use crate::session::{LEGACY_LABELS_FILE, SESSION_FILE, SESSION_VERSION};
use crate::sessions::SESSIONS_DIRECTORY;

/// The Tauri identifier the desk had while it was called MixLink.
pub const LEGACY_IDENTIFIER: &str = "com.mixlink.desktop";

/// The folder the MixLink desk recorded into inside the Music folder (`MixLink\Recordings`).
pub const LEGACY_PRODUCT_FOLDER: &str = "MixLink";

/// The file in this desk's config folder that says the migration already ran.
pub const MIGRATION_MARKER: &str = "mixlink-migration.txt";

/// What the old desk kept in its config folder, in the order it is copied (the session last, so a
/// migration cut short runs again).
const KEPT: [&str; 3] = [LEGACY_LABELS_FILE, SESSIONS_DIRECTORY, BACKUPS_DIRECTORY];

/// The folders this desk has, as Tauri resolves them for its identifier.
#[derive(Clone, Debug)]
pub struct DeskFolders {
    /// The app's config folder (where the session lives).
    pub config: PathBuf,
    /// The app's local data folder, when there is one (where takes fall back to).
    pub local_data: Option<PathBuf>,
    /// The user's Music folder, when there is one.
    pub music: Option<PathBuf>,
    /// This desk's default recordings folder.
    pub recordings: PathBuf,
}

/// What a migration did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Migration {
    /// The old desk's config folder.
    pub from: PathBuf,
    /// The files copied, relative to the config folder.
    pub copied: Vec<PathBuf>,
    /// The files left alone because the new folder already had them, relative to it.
    pub kept: Vec<PathBuf>,
    /// The old default recordings folder the migrated session now names, when it does.
    pub recordings_folder: Option<PathBuf>,
    /// What could not be done.
    pub failures: Vec<String>,
}

/// The folder the old identifier had beside `folder` (a folder Tauri named after this desk's
/// identifier); `None` for a folder with nothing above it.
pub fn legacy_folder(folder: &Path) -> Option<PathBuf> {
    Some(folder.parent()?.join(LEGACY_IDENTIFIER))
}

/// Brings the old desk's data into `desk`'s config folder when the rules above say so; `None` when
/// there was nothing to do.
pub fn bring_over(desk: &DeskFolders) -> Option<Migration> {
    let config = &desk.config;
    if config.join(SESSION_FILE).exists() || config.join(MIGRATION_MARKER).exists() {
        return None;
    }
    let old_config = legacy_folder(config)?;
    if old_config == *config || !old_config.is_dir() {
        return None;
    }
    let mut migration = Migration {
        from: old_config.clone(),
        ..Migration::default()
    };
    if let Err(error) = fs::create_dir_all(config) {
        migration.failures.push(format!(
            "cannot make the config folder \"{}\": {error}",
            config.display()
        ));
        return Some(migration);
    }
    for name in KEPT {
        copy_tree(
            &old_config.join(name),
            &config.join(name),
            Path::new(name),
            &mut migration,
        );
    }
    bring_session_over(desk, &old_config, &mut migration);
    if migration.failures.is_empty() {
        let mut note = format!(
            "Brought over from {} on this desk's first start as BandEar.\n",
            old_config.display()
        );
        for file in &migration.copied {
            note.push_str(&format!("copied {}\n", file.display()));
        }
        if let Err(error) = write_new(&config.join(MIGRATION_MARKER), note.as_bytes()) {
            migration
                .failures
                .push(format!("cannot write {MIGRATION_MARKER}: {error}"));
        }
    }
    Some(migration)
}

/// Copies the old session, last, naming the old default recordings folder in it when its takes
/// would otherwise no longer be listed (see the module notes).
fn bring_session_over(desk: &DeskFolders, old_config: &Path, migration: &mut Migration) {
    let source = old_config.join(SESSION_FILE);
    let target = desk.config.join(SESSION_FILE);
    let session = match fs::read(&source) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            migration
                .failures
                .push(format!("cannot read \"{}\": {error}", source.display()));
            return;
        }
    };
    let old_takes = old_default_recordings_folder(desk, old_config);
    let keep_old_takes = has_takes(&old_takes) && !desk.recordings.exists();
    let bytes = if keep_old_takes {
        match with_recordings_folder(session.as_deref(), &old_takes) {
            Ok(Some(bytes)) => {
                migration.recordings_folder = Some(old_takes);
                Some(bytes)
            }
            Ok(None) => session,
            Err(why) => {
                migration.failures.push(format!(
                    "the takes in \"{}\" may no longer be listed: the session {why}; choose that \
                     folder in the recording settings to see them",
                    old_takes.display()
                ));
                session
            }
        }
    } else {
        session
    };
    let Some(bytes) = bytes else {
        return;
    };
    match write_new(&target, &bytes) {
        Ok(()) => migration.copied.push(PathBuf::from(SESSION_FILE)),
        Err(error) => migration
            .failures
            .push(format!("cannot write \"{}\": {error}", target.display())),
    }
}

/// Where the old desk recorded by default, worked out as it did with its own folders.
fn old_default_recordings_folder(desk: &DeskFolders, old_config: &Path) -> PathBuf {
    let old_local_data = desk.local_data.as_deref().and_then(legacy_folder);
    default_recordings_folder_for(
        LEGACY_PRODUCT_FOLDER,
        desk.music.clone(),
        old_local_data,
        old_config,
    )
    .folder
}

/// Whether `folder` holds a take: a direct subfolder with a `take.json`.
fn has_takes(folder: &Path) -> bool {
    let Ok(entries) = fs::read_dir(folder) else {
        return false;
    };
    entries
        .flatten()
        .any(|entry| entry.path().join(TAKE_FILE).is_file())
}

/// The session `session` (a minimal one when there is none) with its recordings folder set to
/// `folder`; `None` when the session already names a folder, which is kept. An error says why the
/// session cannot take it.
fn with_recordings_folder(
    session: Option<&[u8]>,
    folder: &Path,
) -> Result<Option<Vec<u8>>, String> {
    let mut value = match session {
        Some(bytes) => serde_json::from_slice::<Value>(bytes)
            .map_err(|error| format!("cannot be read ({error})"))?,
        None => serde_json::json!({ "version": SESSION_VERSION }),
    };
    let root = value
        .as_object_mut()
        .ok_or_else(|| "is not a JSON object".to_owned())?;
    let recording = root
        .entry("recording")
        .or_insert_with(|| Value::Object(Map::new()));
    if recording.is_null() {
        *recording = Value::Object(Map::new());
    }
    let recording = recording
        .as_object_mut()
        .ok_or_else(|| "has a recording section that is not an object".to_owned())?;
    if matches!(recording.get("folder"), Some(Value::String(_))) {
        return Ok(None);
    }
    recording.insert(
        "folder".to_owned(),
        Value::String(folder.to_string_lossy().into_owned()),
    );
    serde_json::to_vec_pretty(&value)
        .map(Some)
        .map_err(|error| format!("cannot be written ({error})"))
}

/// Copies `source` (a file or a folder, followed down) to `target`, where nothing is replaced;
/// `relative` is the path both have below their config folder. A source that is not there is
/// nothing to copy.
fn copy_tree(source: &Path, target: &Path, relative: &Path, migration: &mut Migration) {
    let metadata = match fs::metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(error) => {
            migration
                .failures
                .push(format!("cannot read \"{}\": {error}", source.display()));
            return;
        }
    };
    if metadata.is_dir() {
        if let Err(error) = fs::create_dir_all(target) {
            migration
                .failures
                .push(format!("cannot make \"{}\": {error}", target.display()));
            return;
        }
        let mut names = match fs::read_dir(source) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.file_name())
                .collect::<Vec<_>>(),
            Err(error) => {
                migration
                    .failures
                    .push(format!("cannot list \"{}\": {error}", source.display()));
                return;
            }
        };
        names.sort();
        for name in names {
            copy_tree(
                &source.join(&name),
                &target.join(&name),
                &relative.join(&name),
                migration,
            );
        }
        return;
    }
    if target.exists() {
        migration.kept.push(relative.to_path_buf());
        return;
    }
    match fs::read(source).and_then(|bytes| write_new(target, &bytes)) {
        Ok(()) => migration.copied.push(relative.to_path_buf()),
        Err(error) => migration
            .failures
            .push(format!("cannot copy \"{}\": {error}", source.display())),
    }
}

/// Writes `bytes` to `target`, which is not there yet, through a temporary file beside it, so a
/// cut-short write never leaves half a file under the real name.
fn write_new(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut temporary = target.as_os_str().to_owned();
    temporary.push(".migrating");
    let temporary = PathBuf::from(temporary);
    let written = fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, target));
    if written.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    written
}

/// [bring_over], with what it did logged.
pub fn bring_over_and_log(desk: &DeskFolders) {
    let Some(migration) = bring_over(desk) else {
        return;
    };
    log_info!(
        "migration",
        "brought the MixLink desk's data over from \"{}\": {} file(s) copied, {} already here kept",
        migration.from.display(),
        migration.copied.len(),
        migration.kept.len()
    );
    for file in &migration.copied {
        log_info!("migration", "copied {}", file.display());
    }
    for file in &migration.kept {
        log_info!("migration", "kept this desk's {}", file.display());
    }
    if let Some(folder) = &migration.recordings_folder {
        log_info!(
            "migration",
            "takes stay in the MixLink recordings folder \"{}\"",
            folder.display()
        );
    }
    for failure in &migration.failures {
        log_warn!("migration", "{failure}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::recording::default_recordings_folder;
    use crate::session::SessionStore;

    /// A scratch home with the folders Tauri would give both desks.
    struct Home {
        root: PathBuf,
    }

    impl Home {
        fn new(test: &str) -> Self {
            let root = std::env::temp_dir()
                .join("bandear-migration-tests")
                .join(test);
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(&root).expect("scratch folder");
            Self { root }
        }

        fn config(&self) -> PathBuf {
            self.root.join("Roaming").join("com.bandear.desktop")
        }

        fn old_config(&self) -> PathBuf {
            self.root.join("Roaming").join(LEGACY_IDENTIFIER)
        }

        fn music(&self) -> PathBuf {
            self.root.join("Music")
        }

        fn desk(&self) -> DeskFolders {
            let config = self.config();
            let local_data = self.root.join("Local").join("com.bandear.desktop");
            let recordings =
                default_recordings_folder(Some(self.music()), Some(local_data.clone()), &config)
                    .folder;
            DeskFolders {
                config,
                local_data: Some(local_data),
                music: Some(self.music()),
                recordings,
            }
        }
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("folder");
        fs::write(path, text).expect("written");
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).expect("read")
    }

    /// Every file under `folder` with its bytes, by relative path.
    fn snapshot(folder: &Path) -> Vec<(PathBuf, Vec<u8>)> {
        fn walk(base: &Path, folder: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
            let mut entries: Vec<_> = fs::read_dir(folder)
                .expect("listed")
                .map(|entry| entry.expect("entry").path())
                .collect();
            entries.sort();
            for path in entries {
                if path.is_dir() {
                    walk(base, &path, out);
                } else {
                    let relative = path.strip_prefix(base).expect("inside").to_path_buf();
                    out.push((relative, fs::read(&path).expect("read")));
                }
            }
        }
        let mut out = Vec::new();
        walk(folder, folder, &mut out);
        out
    }

    fn old_desk(home: &Home) {
        let old = home.old_config();
        write(&old.join(SESSION_FILE), r#"{"version":1,"groups":[]}"#);
        write(
            &old.join(LEGACY_LABELS_FILE),
            r#"{"192.168.1.10:50000":"Ana"}"#,
        );
        write(
            &old.join(SESSIONS_DIRECTORY).join("show.json"),
            r#"{"version":1}"#,
        );
        write(
            &old.join(BACKUPS_DIRECTORY)
                .join("mixes-20261020T101500Z.json"),
            r#"{"version":1,"mixes":[]}"#,
        );
        write(&old.join("session.json.damaged-1"), "not json");
    }

    fn recording_folder(config: &Path) -> Option<String> {
        SessionStore::open(config)
            .recording()
            .and_then(|recording| recording.folder)
    }

    #[test]
    fn the_old_desks_session_sessions_labels_and_backups_are_copied_once() {
        let home = Home::new("copied-once");
        old_desk(&home);
        let desk = home.desk();

        let migration = bring_over(&desk).expect("migrated");

        let config = home.config();
        let old = home.old_config();
        assert_eq!(migration.from, old);
        assert!(migration.failures.is_empty(), "{:?}", migration.failures);
        for file in [
            PathBuf::from(SESSION_FILE),
            PathBuf::from(LEGACY_LABELS_FILE),
            Path::new(SESSIONS_DIRECTORY).join("show.json"),
            Path::new(BACKUPS_DIRECTORY).join("mixes-20261020T101500Z.json"),
        ] {
            assert_eq!(
                read(&config.join(&file)),
                read(&old.join(&file)),
                "{file:?}"
            );
            assert!(migration.copied.contains(&file), "{file:?} listed");
        }
        assert!(
            !config.join("session.json.damaged-1").exists(),
            "only the desk's own state comes over"
        );
        assert!(config.join(MIGRATION_MARKER).exists());

        // A second start finds the session (and the marker) and does nothing.
        write(&old.join(SESSIONS_DIRECTORY).join("later.json"), "{}");
        assert_eq!(bring_over(&desk), None);
        assert!(!config.join(SESSIONS_DIRECTORY).join("later.json").exists());
    }

    #[test]
    fn a_desk_that_had_no_session_is_still_migrated_only_once() {
        let home = Home::new("no-session-once");
        write(
            &home
                .old_config()
                .join(BACKUPS_DIRECTORY)
                .join("mixes-1.json"),
            "{}",
        );
        let desk = home.desk();

        let migration = bring_over(&desk).expect("migrated");
        assert_eq!(
            migration.copied,
            vec![Path::new(BACKUPS_DIRECTORY).join("mixes-1.json")]
        );
        assert!(!home.config().join(SESSION_FILE).exists());

        // The new desk prunes the backup; the next start must not bring it back.
        fs::remove_file(home.config().join(BACKUPS_DIRECTORY).join("mixes-1.json"))
            .expect("pruned");
        assert_eq!(bring_over(&desk), None);
        assert!(!home
            .config()
            .join(BACKUPS_DIRECTORY)
            .join("mixes-1.json")
            .exists());
    }

    #[test]
    fn a_file_the_new_desk_already_has_is_never_replaced() {
        let home = Home::new("never-overwrites");
        old_desk(&home);
        let backup = Path::new(BACKUPS_DIRECTORY).join("mixes-20261020T101500Z.json");
        write(&home.config().join(&backup), "the new desk's own");

        let migration = bring_over(&home.desk()).expect("migrated");

        assert_eq!(read(&home.config().join(&backup)), "the new desk's own");
        assert_eq!(migration.kept, vec![backup.clone()]);
        assert!(!migration.copied.contains(&backup));
        assert!(
            home.config().join(SESSION_FILE).exists(),
            "the rest still comes over"
        );
    }

    #[test]
    fn nothing_happens_when_the_new_desk_already_has_a_session() {
        let home = Home::new("new-session");
        old_desk(&home);
        write(&home.config().join(SESSION_FILE), r#"{"version":1}"#);

        assert_eq!(bring_over(&home.desk()), None);

        assert_eq!(read(&home.config().join(SESSION_FILE)), r#"{"version":1}"#);
        assert!(!home.config().join(SESSIONS_DIRECTORY).exists());
        assert!(!home.config().join(MIGRATION_MARKER).exists());
    }

    #[test]
    fn nothing_happens_without_an_old_desk() {
        let home = Home::new("no-old-desk");

        assert_eq!(bring_over(&home.desk()), None);

        assert!(!home.config().join(MIGRATION_MARKER).exists());
    }

    #[test]
    fn the_old_folders_are_left_exactly_as_they_were() {
        let home = Home::new("old-untouched");
        old_desk(&home);
        let takes = home.music().join(LEGACY_PRODUCT_FOLDER).join("Recordings");
        write(&takes.join("Take 1").join(TAKE_FILE), "{}");
        let before = snapshot(&home.old_config());
        let takes_before = snapshot(&takes);

        bring_over(&home.desk()).expect("migrated");

        assert_eq!(snapshot(&home.old_config()), before);
        assert_eq!(snapshot(&takes), takes_before);
    }

    #[test]
    fn takes_in_the_old_default_folder_stay_listed() {
        let home = Home::new("old-takes");
        old_desk(&home);
        let old_takes = home.music().join(LEGACY_PRODUCT_FOLDER).join("Recordings");
        write(&old_takes.join("Take 1").join(TAKE_FILE), "{}");

        let migration = bring_over(&home.desk()).expect("migrated");

        assert_eq!(migration.recordings_folder, Some(old_takes.clone()));
        assert_eq!(
            recording_folder(&home.config()),
            Some(old_takes.to_string_lossy().into_owned())
        );
        let session: Value =
            serde_json::from_str(&read(&home.config().join(SESSION_FILE))).expect("json");
        assert_eq!(
            session["groups"],
            Value::Array(Vec::new()),
            "the rest is kept"
        );
    }

    #[test]
    fn a_desk_without_a_session_but_with_takes_gets_a_session_naming_their_folder() {
        let home = Home::new("old-takes-no-session");
        let old_takes = home.music().join(LEGACY_PRODUCT_FOLDER).join("Recordings");
        write(&old_takes.join("Take 1").join(TAKE_FILE), "{}");
        fs::create_dir_all(home.old_config()).expect("old desk");

        bring_over(&home.desk()).expect("migrated");

        assert_eq!(
            recording_folder(&home.config()),
            Some(old_takes.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn a_recordings_folder_the_engineer_chose_is_kept() {
        let home = Home::new("chosen-folder");
        write(
            &home.old_config().join(SESSION_FILE),
            r#"{"version":1,"recording":{"folder":"D:\\Shows\\Takes","armed":[1]}}"#,
        );
        write(
            &home
                .music()
                .join(LEGACY_PRODUCT_FOLDER)
                .join("Recordings")
                .join("Take 1")
                .join(TAKE_FILE),
            "{}",
        );

        let migration = bring_over(&home.desk()).expect("migrated");

        assert_eq!(migration.recordings_folder, None);
        assert_eq!(
            read(&home.config().join(SESSION_FILE)),
            read(&home.old_config().join(SESSION_FILE))
        );
    }

    #[test]
    fn the_default_folder_stays_when_the_old_one_has_no_takes_or_the_new_one_exists() {
        // An old default folder with no take in it.
        let home = Home::new("no-old-takes");
        old_desk(&home);
        let old_takes = home.music().join(LEGACY_PRODUCT_FOLDER).join("Recordings");
        write(&old_takes.join("notes.txt"), "not a take");
        fs::create_dir_all(old_takes.join("Empty")).expect("folder");
        assert_eq!(
            bring_over(&home.desk())
                .expect("migrated")
                .recordings_folder,
            None
        );
        assert_eq!(recording_folder(&home.config()), None);

        // Takes in the old folder, but this desk's default folder is already there.
        let home = Home::new("new-folder-exists");
        old_desk(&home);
        let old_takes = home.music().join(LEGACY_PRODUCT_FOLDER).join("Recordings");
        write(&old_takes.join("Take 1").join(TAKE_FILE), "{}");
        let desk = home.desk();
        fs::create_dir_all(&desk.recordings).expect("new default folder");
        assert_eq!(bring_over(&desk).expect("migrated").recordings_folder, None);
        assert_eq!(recording_folder(&home.config()), None);
    }

    #[test]
    fn the_old_identifier_is_the_sibling_of_each_new_folder() {
        let folder = Path::new("C:\\Users\\Ana\\AppData\\Roaming").join("com.bandear.desktop");
        assert_eq!(
            legacy_folder(&folder),
            Some(Path::new("C:\\Users\\Ana\\AppData\\Roaming").join(LEGACY_IDENTIFIER))
        );
    }
}
