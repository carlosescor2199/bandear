//! The desktop session: what this desk keeps between runs, in one versioned JSON file in the app's
//! config directory.
//!
//! The session is a desktop-app concern only: the engine never reads the file, and only what the
//! desk applies from it (the groups) ever reaches a client. Version 1 has this shape:
//!
//! ```json
//! {
//!   "version": 1,
//!   "musicians": {
//!     "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b": { "label": "Ana" },
//!     "address:192.168.1.40:50000": { "label": "Rack A" }
//!   },
//!   "channels": [],
//!   "groups": [
//!     { "id": "g3f9a0c1d2e4b5a6", "name": "Drums", "channels": [1, 2, 3] }
//!   ],
//!   "presets": []
//! }
//! ```
//!
//! - `musicians` holds what the desk keeps about each musician, today only the engineer's local
//!   label. A phone that announced a stable client id is keyed `client:<client_id>`; a configured
//!   target, or a phone that sent no id, is keyed `address:<ip:port>`, the address exactly as the
//!   status reports it. A label is looked up by the client key first and the address key second,
//!   so a phone keeps its label when DHCP gives it another IP.
//! - `groups` holds the engineer's groups in their order: each with the stable id every musician's
//!   group level follows, a name and 1-based channel numbers (as the engineer numbers them). They are
//!   applied to every engine the desk starts (see [crate::commands::start_engine]) and replaced by
//!   every edit ([crate::commands::set_groups]). The section was reserved in the first version and
//!   every earlier build wrote it empty and kept whatever it read, so giving its entries a shape
//!   does not change the version: an earlier build still reads and keeps it.
//! - `channels` and `presets` are reserved for the channel table and the engineer's mix presets.
//!   Until the code that owns them lands they are written back exactly as they were read.
//!
//! Every save writes a temporary file next to the session, flushes it to disk and renames it over
//! the session, so a crash or a full disk leaves the old file or the new one, never a torn one, and
//! a save that fails changes nothing. The sections besides `musicians` are read one by one: a
//! section that cannot be read is kept as it was in `session.json.<section>-damaged-<unix
//! milliseconds>`, starts empty, and the session is saved without it at once, so the labels and
//! every readable section survive. Only a file that is not JSON, has no usable version or whose
//! `musicians` cannot be read is moved aside whole, as `session.json.corrupt-<unix milliseconds>`,
//! and the desk starts with an empty session; a session written by a newer version is left
//! untouched and nothing is ever saved over it. All of these are logged and kept in
//! [SessionStore::notice], and none stops the app from starting: labels are a convenience, and
//! losing them must not stop the server.
//!
//! `musicians.json`, the address-to-label map earlier versions kept, is imported once: when there
//! is no session yet, its labels become `address:` entries and the session is written. The old
//! file is left in place untouched, and once the session exists it is no longer read.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The session's file name inside the app config directory.
pub const SESSION_FILE: &str = "session.json";
/// The address-to-label map earlier versions kept, imported once into the session.
pub const LEGACY_LABELS_FILE: &str = "musicians.json";
/// The session format this build reads and writes.
pub const SESSION_VERSION: u64 = 1;

/// The whole session file (see the module documentation for its shape).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Session {
    pub version: u64,
    /// What the desk keeps about each musician, by [client_key] or [address_key].
    #[serde(default)]
    pub musicians: BTreeMap<String, MusicianEntry>,
    /// Reserved for the channel table; kept as read.
    #[serde(default)]
    pub channels: Vec<Value>,
    /// The engineer's groups, in their order.
    #[serde(default)]
    pub groups: Vec<SessionGroup>,
    /// Reserved for the engineer's mix presets; kept as read.
    #[serde(default)]
    pub presets: Vec<Value>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            musicians: BTreeMap::new(),
            channels: Vec::new(),
            groups: Vec::new(),
            presets: Vec::new(),
        }
    }
}

/// What the desk keeps about one musician.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicianEntry {
    /// The engineer's local label, shown while the musician's phone announced no name of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// One of the engineer's groups as the desk keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionGroup {
    /// The group's stable id, generated by the desk when the group was created.
    pub id: String,
    pub name: String,
    /// 1-based channel numbers.
    pub channels: Vec<usize>,
}

/// The key of what the desk keeps about the phone with this stable client id.
pub fn client_key(client_id: &str) -> String {
    format!("client:{client_id}")
}

/// The key of what the desk keeps about the musician at this address, for a configured target or a
/// phone that announced no client id.
pub fn address_key(address: &str) -> String {
    format!("address:{address}")
}

/// The session as loaded, and whether it may be saved.
struct Loaded {
    session: Session,
    /// Why saving is refused, when the file on disk must not be replaced.
    refusal: Option<String>,
    /// What went wrong or changed while loading, for the log and [SessionStore::notice].
    notice: Option<String>,
}

impl Loaded {
    fn fresh(notice: Option<String>) -> Self {
        Self {
            session: Session::default(),
            refusal: None,
            notice,
        }
    }
}

/// The session file and the session in memory.
///
/// The session in memory is only ever replaced whole, after its file was written, so what the desk
/// shows is always what a restart would load.
pub struct SessionStore {
    path: PathBuf,
    loaded: Mutex<Loaded>,
}

impl SessionStore {
    /// Opens the session in the config `directory`: loads `session.json`, or imports
    /// `musicians.json` when there is no session yet, or starts empty. Never fails; a problem is
    /// logged and kept in [Self::notice].
    pub fn open(directory: &Path) -> Self {
        let path = directory.join(SESSION_FILE);
        let loaded = load(&path, &directory.join(LEGACY_LABELS_FILE));
        if let Some(notice) = &loaded.notice {
            eprintln!("[mixlink-desktop] {notice}");
        }
        Self {
            path,
            loaded: Mutex::new(loaded),
        }
    }

    /// What went wrong or changed when the session was opened (a file moved aside, a newer
    /// session left alone, labels imported), or `None` when it simply loaded.
    pub fn notice(&self) -> Option<String> {
        self.lock().notice.clone()
    }

    /// The local label of the musician with this client id at this address: the one kept for the
    /// phone first, then the one kept for the address.
    pub fn label_for(&self, client_id: Option<&str>, address: &str) -> Option<String> {
        let loaded = self.lock();
        let musicians = &loaded.session.musicians;
        let label = |key: String| musicians.get(&key).and_then(|entry| entry.label.clone());
        client_id
            .and_then(|id| label(client_key(id)))
            .or_else(|| label(address_key(address)))
    }

    /// Sets the local label of the musician with this client id at this address, or clears it when
    /// `name` is empty or whitespace. The file is written before success is reported.
    ///
    /// A musician with a client id gets the label under their phone's key, so it follows the phone
    /// to any address, and any label kept for the address goes: what the row shows is what was just
    /// set. A musician without one gets it under the address.
    pub fn set_label(
        &self,
        client_id: Option<&str>,
        address: &str,
        name: &str,
    ) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let name = name.trim();
        let label = (!name.is_empty()).then(|| name.to_owned());
        let mut session = loaded.session.clone();
        match client_id {
            Some(id) => {
                set_entry_label(&mut session, address_key(address), None);
                set_entry_label(&mut session, client_key(id), label);
            }
            None => set_entry_label(&mut session, address_key(address), label),
        }
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        Ok(())
    }

    /// The engineer's groups, in their order.
    pub fn groups(&self) -> Vec<SessionGroup> {
        self.lock().session.groups.clone()
    }

    /// Replaces the engineer's groups. The file is written before success is reported; a save
    /// that fails, or is refused over a session this build must not replace, changes nothing.
    pub fn set_groups(&self, groups: Vec<SessionGroup>) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let session = Session {
            groups,
            ..loaded.session.clone()
        };
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        Ok(())
    }

    // The session is only replaced after a successful write, so even a poisoned lock guards a
    // session that matches the file; the desk keeps working rather than panicking.
    fn lock(&self) -> MutexGuard<'_, Loaded> {
        self.loaded.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn set_entry_label(session: &mut Session, key: String, label: Option<String>) {
    match label {
        Some(label) => session.musicians.entry(key).or_default().label = Some(label),
        None => {
            if let Some(entry) = session.musicians.get_mut(&key) {
                entry.label = None;
                if *entry == MusicianEntry::default() {
                    session.musicians.remove(&key);
                }
            }
        }
    }
}

fn load(path: &Path, legacy: &Path) -> Loaded {
    match fs::read(path) {
        Ok(bytes) => match parse(&bytes) {
            Parsed::Current(session, damaged) if damaged.is_empty() => Loaded {
                session,
                refusal: None,
                notice: None,
            },
            Parsed::Current(session, damaged) => set_sections_aside(path, session, damaged),
            Parsed::Newer(version) => {
                let refusal = format!(
                    "the session file {} was written by a newer MixLink (version {version}); it is \
                     left untouched, the desk starts with an empty session and changes are not saved",
                    path.display()
                );
                Loaded {
                    session: Session::default(),
                    refusal: Some(refusal.clone()),
                    notice: Some(refusal),
                }
            }
            Parsed::Invalid(problem) => set_aside(path, &problem),
        },
        Err(error) if error.kind() == ErrorKind::NotFound => import_legacy(path, legacy),
        Err(error) => set_aside(path, &format!("could not be read ({error})")),
    }
}

enum Parsed {
    /// A session this build reads, with the sections that could not be read (they start empty).
    Current(Session, Vec<DamagedSection>),
    Newer(u64),
    Invalid(String),
}

/// A section of the session, other than the labels, that could not be read, as the file had it.
struct DamagedSection {
    name: &'static str,
    raw: Value,
    problem: String,
}

fn parse(bytes: &[u8]) -> Parsed {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return Parsed::Invalid(format!("is not valid JSON ({error})")),
    };
    let Some(version) = value.get("version").and_then(Value::as_u64) else {
        return Parsed::Invalid("has no version".to_owned());
    };
    if version > SESSION_VERSION {
        return Parsed::Newer(version);
    }
    if version != SESSION_VERSION {
        return Parsed::Invalid(format!("has version {version}, which no MixLink writes"));
    }
    let Value::Object(mut fields) = value else {
        return Parsed::Invalid("is not a session object".to_owned());
    };
    // Each section besides the labels is read on its own, so one that is damaged costs only
    // itself; a file whose labels cannot be read is not a session this build can use.
    let mut damaged = Vec::new();
    let groups = take_section(&mut fields, "groups", &mut damaged);
    let channels = take_section(&mut fields, "channels", &mut damaged);
    let presets = take_section(&mut fields, "presets", &mut damaged);
    match serde_json::from_value::<Session>(Value::Object(fields)) {
        Ok(session) => Parsed::Current(
            Session {
                groups,
                channels,
                presets,
                ..session
            },
            damaged,
        ),
        Err(error) => Parsed::Invalid(format!("is not a session ({error})")),
    }
}

/// Takes the section `name` out of the session's fields and reads it on its own. A section that
/// cannot be read starts empty and is listed in `damaged` with its raw JSON.
fn take_section<T: DeserializeOwned + Default>(
    fields: &mut Map<String, Value>,
    name: &'static str,
    damaged: &mut Vec<DamagedSection>,
) -> T {
    let Some(raw) = fields.remove(name) else {
        return T::default();
    };
    match T::deserialize(&raw) {
        Ok(section) => section,
        Err(error) => {
            damaged.push(DamagedSection {
                name,
                raw,
                problem: error.to_string(),
            });
            T::default()
        }
    }
}

/// Keeps each section that could not be read in a file of its own next to the session,
/// `session.json.<section>-damaged-<unix milliseconds>`, and saves the session without them at
/// once: the labels and every readable section are kept, and the next start loads cleanly. A
/// section that cannot be written aside would be lost by a save, so the session is then not saved
/// over at all.
fn set_sections_aside(path: &Path, session: Session, damaged: Vec<DamagedSection>) -> Loaded {
    let millis = unix_millis();
    let mut kept = Vec::with_capacity(damaged.len());
    for section in &damaged {
        let aside =
            path.with_file_name(format!("{SESSION_FILE}.{}-damaged-{millis}", section.name));
        let written = serde_json::to_string_pretty(&section.raw)
            .map_err(io::Error::other)
            .and_then(|json| fs::write(&aside, json));
        if let Err(error) = written {
            let notice = format!(
                "the session file {} has a `{}` section that cannot be read ({}) and it could not \
                 be kept aside ({error}); the section starts empty and the session is not saved \
                 over",
                path.display(),
                section.name,
                section.problem
            );
            return Loaded {
                session,
                refusal: Some(notice.clone()),
                notice: Some(notice),
            };
        }
        kept.push(format!(
            "its `{}` section cannot be read ({}) and was kept in {}",
            section.name,
            section.problem,
            aside.display()
        ));
    }
    let saved = match write_atomically(path, &session) {
        Ok(()) => String::new(),
        Err(error) => format!(" ({error}; the next start finds the damaged sections again)"),
    };
    Loaded {
        session,
        refusal: None,
        notice: Some(format!(
            "the session file {}: {}; those sections start empty, and the musician labels and the \
             rest of the session are kept{saved}",
            path.display(),
            kept.join("; ")
        )),
    }
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default()
}

/// Moves a session that cannot be used out of the way and starts an empty one, saved at once: a
/// session existed, so `musicians.json` must not be imported on the next start. A file that cannot
/// be moved either is not saved over, so it is never lost.
fn set_aside(path: &Path, problem: &str) -> Loaded {
    let aside = path.with_file_name(format!("{SESSION_FILE}.corrupt-{}", unix_millis()));
    match fs::rename(path, &aside) {
        Ok(()) => {
            let saved = match write_atomically(path, &Session::default()) {
                Ok(()) => String::new(),
                Err(error) => format!(
                    " ({error}; until a change is saved, the next start may import the old label \
                     file again)"
                ),
            };
            Loaded::fresh(Some(format!(
                "the session file {} {problem}; it was moved to {} and the desk starts with an \
                 empty session{saved}",
                path.display(),
                aside.display()
            )))
        }
        Err(error) => {
            let notice = format!(
                "the session file {} {problem} and could not be moved aside ({error}); the desk \
                 starts with an empty session and does not save over it",
                path.display()
            );
            Loaded {
                session: Session::default(),
                refusal: Some(notice.clone()),
                notice: Some(notice),
            }
        }
    }
}

/// Imports the labels of `musicians.json`, when there is one, into a new session and saves it.
fn import_legacy(path: &Path, legacy: &Path) -> Loaded {
    let text = match fs::read_to_string(legacy) {
        Ok(text) => text,
        Err(error) if error.kind() == ErrorKind::NotFound => return Loaded::fresh(None),
        Err(error) => {
            return Loaded::fresh(Some(format!(
                "the old label file {} could not be read ({error}); no labels were imported",
                legacy.display()
            )))
        }
    };
    let labels: BTreeMap<String, String> = match serde_json::from_str(&text) {
        Ok(labels) => labels,
        Err(error) => {
            return Loaded::fresh(Some(format!(
                "the old label file {} is not valid JSON ({error}); no labels were imported",
                legacy.display()
            )))
        }
    };
    let mut session = Session::default();
    for (address, label) in labels {
        let label = label.trim();
        if !label.is_empty() {
            set_entry_label(&mut session, address_key(&address), Some(label.to_owned()));
        }
    }
    let count = session.musicians.len();
    let notice = match write_atomically(path, &session) {
        Ok(()) => format!(
            "imported {count} labels from {} into {}; the old file is kept but no longer read",
            legacy.display(),
            path.display()
        ),
        Err(error) => format!(
            "imported {count} labels from {} but could not save them ({error}); they are imported \
             again on the next start",
            legacy.display()
        ),
    };
    Loaded {
        session,
        refusal: None,
        notice: Some(notice),
    }
}

/// Replaces the session file with `session`: a temporary file in the same directory, flushed to
/// disk, then renamed over the old one. On failure the old file is untouched.
fn write_atomically(path: &Path, session: &Session) -> Result<(), String> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| {
            format!(
                "could not create the config directory {}: {error}",
                directory.display()
            )
        })?;
    }
    let json = serde_json::to_string_pretty(session)
        .map_err(|error| format!("could not serialize the session: {error}"))?;
    let temporary = path.with_file_name(format!("{SESSION_FILE}.tmp"));
    let written = (|| -> io::Result<()> {
        let mut file = File::create(&temporary)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()
    })();
    let replaced = written.and_then(|()| fs::rename(&temporary, path));
    replaced.map_err(|error| {
        let _ = fs::remove_file(&temporary);
        format!("could not save {}: {error}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const OTHER_PHONE: &str = "7d1e2f3a-4b5c-4d6e-8f9a-0b1c2d3e4f5a";
    const ADDRESS: &str = "192.168.1.30:50000";

    /// An empty scratch config directory for one test.
    fn test_directory(test: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-session-tests")
            .join(test);
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("test directory should be created");
        directory
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(path).expect("the file should be readable"))
            .expect("the file should be JSON")
    }

    /// The names of the files in `directory`, sorted.
    fn files_in(directory: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(directory)
            .expect("the directory should be listable")
            .map(|entry| {
                entry
                    .expect("a directory entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_label_survives_a_reload() {
        let directory = test_directory("reload");
        let session = SessionStore::open(&directory);

        session
            .set_label(Some(PHONE), ADDRESS, "Ana")
            .expect("setting a label should persist");

        let reloaded = SessionStore::open(&directory);
        assert_eq!(
            reloaded.label_for(Some(PHONE), ADDRESS),
            Some("Ana".to_owned())
        );
        assert_eq!(reloaded.notice(), None, "a healthy session reports nothing");
    }

    #[test]
    fn the_file_is_versioned_keys_labels_by_client_id_and_reserves_the_later_sections() {
        let directory = test_directory("shape");
        let session = SessionStore::open(&directory);

        session.set_label(Some(PHONE), ADDRESS, "Ana").expect("set");
        session
            .set_label(None, "192.168.1.40:50000", "Rack A")
            .expect("set");

        assert_eq!(
            read_json(&directory.join(SESSION_FILE)),
            serde_json::json!({
                "version": 1,
                "musicians": {
                    format!("client:{PHONE}"): { "label": "Ana" },
                    "address:192.168.1.40:50000": { "label": "Rack A" }
                },
                "channels": [],
                "groups": [],
                "presets": []
            })
        );
    }

    #[test]
    fn a_phone_keeps_its_label_when_its_ip_changes() {
        let directory = test_directory("ip-change");
        let session = SessionStore::open(&directory);
        session
            .set_label(Some(PHONE), "192.168.1.10:50000", "Ana")
            .expect("set");

        assert_eq!(
            session.label_for(Some(PHONE), "192.168.1.22:50000"),
            Some("Ana".to_owned()),
            "the same phone at a new address"
        );
        assert_eq!(
            SessionStore::open(&directory).label_for(Some(PHONE), "192.168.1.22:50000"),
            Some("Ana".to_owned()),
            "and after a restart"
        );
        assert_eq!(
            session.label_for(Some(OTHER_PHONE), "192.168.1.10:50000"),
            None,
            "another phone that now has the old address does not inherit it"
        );
    }

    #[test]
    fn an_address_label_is_the_fallback_and_a_phone_label_replaces_it() {
        let directory = test_directory("fallback");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Rack A").expect("set");

        assert_eq!(session.label_for(None, ADDRESS), Some("Rack A".to_owned()));
        assert_eq!(
            session.label_for(Some(PHONE), ADDRESS),
            Some("Rack A".to_owned()),
            "a phone with no label of its own falls back to its address's"
        );

        session.set_label(Some(PHONE), ADDRESS, "Ana").expect("set");

        assert_eq!(
            session.label_for(Some(PHONE), ADDRESS),
            Some("Ana".to_owned())
        );
        assert_eq!(
            session.label_for(None, ADDRESS),
            None,
            "the label moved to the phone, so the row shows what was just set"
        );
    }

    #[test]
    fn an_empty_name_clears_the_label_and_its_entry() {
        let directory = test_directory("clear");
        let session = SessionStore::open(&directory);
        session.set_label(Some(PHONE), ADDRESS, "Ana").expect("set");

        session
            .set_label(Some(PHONE), ADDRESS, "   ")
            .expect("clear");

        assert_eq!(session.label_for(Some(PHONE), ADDRESS), None);
        assert_eq!(
            SessionStore::open(&directory).label_for(Some(PHONE), ADDRESS),
            None
        );
        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({})
        );
    }

    #[test]
    fn renaming_one_musician_leaves_the_others_alone() {
        let directory = test_directory("rename");
        let session = SessionStore::open(&directory);
        session.set_label(Some(PHONE), ADDRESS, "Ana").expect("set");
        session
            .set_label(Some(OTHER_PHONE), "192.168.1.31:50000", "Beto")
            .expect("set");

        session
            .set_label(Some(PHONE), ADDRESS, "Ana Maria")
            .expect("rename");

        let reloaded = SessionStore::open(&directory);
        assert_eq!(
            reloaded.label_for(Some(PHONE), ADDRESS),
            Some("Ana Maria".to_owned())
        );
        assert_eq!(
            reloaded.label_for(Some(OTHER_PHONE), "192.168.1.31:50000"),
            Some("Beto".to_owned())
        );
    }

    #[test]
    fn with_no_files_the_session_starts_empty_and_writes_nothing_until_a_change() {
        let directory = test_directory("fresh").join("nested");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), None);
        assert_eq!(session.notice(), None);
        assert!(!directory.join(SESSION_FILE).exists());
        session.set_label(None, ADDRESS, "Ana").expect("set");
        assert_eq!(
            SessionStore::open(&directory).label_for(None, ADDRESS),
            Some("Ana".to_owned()),
            "the missing directory is created on the first save"
        );
    }

    #[test]
    fn a_save_goes_through_a_temporary_file_that_is_not_left_behind() {
        let directory = test_directory("atomic");
        let session = SessionStore::open(&directory);

        session.set_label(None, ADDRESS, "Ana").expect("set");
        session
            .set_label(None, ADDRESS, "Ana Maria")
            .expect("rename");

        assert_eq!(files_in(&directory), vec![SESSION_FILE.to_owned()]);
    }

    #[test]
    fn a_failed_save_leaves_the_saved_session_and_the_label_shown_as_they_were() {
        let directory = test_directory("failed-save");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Ana").expect("set");
        let before = fs::read(directory.join(SESSION_FILE)).expect("saved");
        // A directory where the temporary file goes makes the write fail before the rename.
        fs::create_dir(directory.join(format!("{SESSION_FILE}.tmp"))).expect("blocker");

        let error = session
            .set_label(None, ADDRESS, "Beto")
            .expect_err("the write cannot happen");

        assert!(error.contains(SESSION_FILE), "names the file: {error}");
        assert_eq!(
            fs::read(directory.join(SESSION_FILE)).expect("still there"),
            before,
            "the saved session is untouched"
        );
        assert_eq!(
            session.label_for(None, ADDRESS),
            Some("Ana".to_owned()),
            "a label that was not saved is not shown as if it were"
        );
    }

    #[test]
    fn a_corrupt_file_is_moved_aside_and_the_desk_starts_with_an_empty_session() {
        let directory = test_directory("corrupt");
        fs::write(directory.join(SESSION_FILE), "this is not json").expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), None);
        let notice = session.notice().expect("the problem is reported");
        assert!(notice.contains("moved"), "says what happened: {notice}");
        let files = files_in(&directory);
        assert_eq!(
            files.len(),
            2,
            "the new session and the moved file: {files:?}"
        );
        assert_eq!(files[0], SESSION_FILE);
        assert!(
            files[1].starts_with(&format!("{SESSION_FILE}.corrupt-")),
            "{files:?}"
        );
        assert_eq!(
            fs::read_to_string(directory.join(&files[1])).expect("kept"),
            "this is not json",
            "the bad file is kept for inspection, byte for byte"
        );
        assert_eq!(
            read_json(&directory.join(SESSION_FILE)),
            serde_json::to_value(Session::default()).expect("serializes"),
            "the empty session it starts with is saved at once"
        );
        session
            .set_label(None, ADDRESS, "Ana")
            .expect("a new session saves");
        assert_eq!(
            SessionStore::open(&directory).label_for(None, ADDRESS),
            Some("Ana".to_owned())
        );
    }

    #[test]
    fn after_a_recovery_the_old_label_file_is_never_imported_again() {
        let directory = test_directory("recovery-no-reimport");
        let legacy = r#"{"192.168.1.30:50000": "Stale"}"#;
        fs::write(directory.join(LEGACY_LABELS_FILE), legacy).expect("write");
        fs::write(directory.join(SESSION_FILE), "this is not json").expect("write");

        let recovered = SessionStore::open(&directory);
        // Nothing is saved before the next start.
        drop(recovered);
        let reopened = SessionStore::open(&directory);

        assert_eq!(
            reopened.label_for(None, ADDRESS),
            None,
            "a session existed, so the old file's labels stay out"
        );
        assert_eq!(reopened.notice(), None, "the second start simply loads");
        assert_eq!(
            fs::read_to_string(directory.join(LEGACY_LABELS_FILE)).expect("kept"),
            legacy
        );
    }

    #[test]
    fn a_file_without_the_sessions_shape_is_moved_aside_too() {
        for (case, contents) in [
            ("no-version", r#"{"musicians":{}}"#),
            ("not-an-object", "[1, 2, 3]"),
            ("wrong-type", r#"{"version":1,"musicians":"Ana"}"#),
            ("version-zero", r#"{"version":0}"#),
        ] {
            let directory = test_directory(&format!("shape-{case}"));
            fs::write(directory.join(SESSION_FILE), contents).expect("write");

            let session = SessionStore::open(&directory);

            assert!(session.notice().is_some(), "{case} is reported");
            let moved = files_in(&directory)
                .into_iter()
                .find(|name| name.starts_with(&format!("{SESSION_FILE}.corrupt-")))
                .unwrap_or_else(|| panic!("{case} is moved aside"));
            assert_eq!(
                fs::read_to_string(directory.join(moved)).expect("kept"),
                contents
            );
            assert_eq!(
                read_json(&directory.join(SESSION_FILE)),
                serde_json::to_value(Session::default()).expect("serializes"),
                "{case}: an empty session replaces it"
            );
            session
                .set_label(None, ADDRESS, "Ana")
                .unwrap_or_else(|error| panic!("{case}: a new session saves: {error}"));
        }
    }

    #[test]
    fn a_session_from_a_newer_version_is_never_overwritten() {
        let directory = test_directory("newer");
        let newer = r#"{"version":2,"musicians":{"client:x":{"label":"Ana","colour":"red"}}}"#;
        fs::write(directory.join(SESSION_FILE), newer).expect("write");

        let session = SessionStore::open(&directory);
        let error = session
            .set_label(None, ADDRESS, "Beto")
            .expect_err("saving over a newer session is refused");

        assert!(error.contains("newer"), "says why: {error}");
        assert!(
            session.notice().expect("reported").contains("newer"),
            "the start reports it too"
        );
        assert_eq!(
            fs::read_to_string(directory.join(SESSION_FILE)).expect("still there"),
            newer,
            "the newer file is left exactly as it was"
        );
        assert_eq!(files_in(&directory), vec![SESSION_FILE.to_owned()]);
    }

    #[test]
    fn the_reserved_sections_are_written_back_as_they_were_read() {
        let directory = test_directory("reserved");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{},"channels":[{"name":"Kick"}],"groups":[],"presets":[{"name":"Verse"}]}"#,
        )
        .expect("write");

        SessionStore::open(&directory)
            .set_label(None, ADDRESS, "Ana")
            .expect("set");

        let saved = read_json(&directory.join(SESSION_FILE));
        assert_eq!(saved["channels"], serde_json::json!([{ "name": "Kick" }]));
        assert_eq!(saved["presets"], serde_json::json!([{ "name": "Verse" }]));
        assert_eq!(
            saved["musicians"],
            serde_json::json!({ "address:192.168.1.30:50000": { "label": "Ana" } })
        );
    }

    fn group(id: &str, name: &str, channels: &[usize]) -> SessionGroup {
        SessionGroup {
            id: id.to_owned(),
            name: name.to_owned(),
            channels: channels.to_vec(),
        }
    }

    #[test]
    fn groups_are_kept_with_their_ids_and_survive_a_reload() {
        let directory = test_directory("groups");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Ana").expect("set");
        let groups = vec![group("kit", "Drums", &[1, 2]), group("vox", "Vocals", &[5])];

        session
            .set_groups(groups.clone())
            .expect("the groups are saved");

        assert_eq!(session.groups(), groups);
        assert_eq!(SessionStore::open(&directory).groups(), groups);
        assert_eq!(
            read_json(&directory.join(SESSION_FILE)),
            serde_json::json!({
                "version": 1,
                "musicians": { "address:192.168.1.30:50000": { "label": "Ana" } },
                "channels": [],
                "groups": [
                    { "id": "kit", "name": "Drums", "channels": [1, 2] },
                    { "id": "vox", "name": "Vocals", "channels": [5] }
                ],
                "presets": []
            }),
            "the same version 1 file: an earlier build reads the section as it always did"
        );
    }

    #[test]
    fn saving_groups_is_refused_over_a_newer_session_and_a_failed_save_changes_nothing() {
        let newer_directory = test_directory("groups-newer");
        fs::write(newer_directory.join(SESSION_FILE), r#"{"version":2}"#).expect("write");
        let newer = SessionStore::open(&newer_directory);
        assert!(newer
            .set_groups(vec![group("kit", "Drums", &[1])])
            .expect_err("refused")
            .contains("newer"));
        assert_eq!(newer.groups(), Vec::new());

        let directory = test_directory("groups-failed-save");
        let session = SessionStore::open(&directory);
        session
            .set_groups(vec![group("kit", "Drums", &[1])])
            .expect("saved");
        fs::create_dir(directory.join(format!("{SESSION_FILE}.tmp"))).expect("blocker");
        session
            .set_groups(Vec::new())
            .expect_err("the write cannot happen");
        assert_eq!(session.groups(), vec![group("kit", "Drums", &[1])]);
    }

    #[test]
    fn a_damaged_groups_section_is_set_aside_on_its_own_and_the_labels_are_kept() {
        let directory = test_directory("groups-damaged");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{"address:192.168.1.30:50000":{"label":"Ana"}},
                "channels":[{"name":"Kick"}],"groups":[{"id":1}],"presets":[]}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(
            session.label_for(None, ADDRESS),
            Some("Ana".to_owned()),
            "the labels are kept"
        );
        assert_eq!(session.groups(), Vec::new(), "the groups start empty");
        let files = files_in(&directory);
        assert!(
            !files.iter().any(|name| name.contains(".corrupt-")),
            "the file as a whole is not set aside: {files:?}"
        );
        let aside = files
            .iter()
            .find(|name| name.starts_with(&format!("{SESSION_FILE}.groups-damaged-")))
            .unwrap_or_else(|| panic!("the damaged section is kept for recovery: {files:?}"));
        assert_eq!(
            read_json(&directory.join(aside)),
            serde_json::json!([{ "id": 1 }]),
            "byte for byte as JSON"
        );
        let notice = session.notice().expect("the problem is reported");
        assert!(
            notice.contains("groups") && notice.contains(aside.as_str()),
            "says which section and where it went: {notice}"
        );
        let saved = read_json(&directory.join(SESSION_FILE));
        assert_eq!(saved["groups"], serde_json::json!([]));
        assert_eq!(saved["channels"], serde_json::json!([{ "name": "Kick" }]));
        assert_eq!(
            saved["musicians"],
            serde_json::json!({ "address:192.168.1.30:50000": { "label": "Ana" } })
        );
        let reopened = SessionStore::open(&directory);
        assert_eq!(reopened.notice(), None, "the next start simply loads");
        assert_eq!(reopened.label_for(None, ADDRESS), Some("Ana".to_owned()));
    }

    #[test]
    fn a_damaged_reserved_section_is_set_aside_on_its_own_too() {
        let directory = test_directory("channels-damaged");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{"address:192.168.1.30:50000":{"label":"Ana"}},
                "channels":5,"groups":[{"id":"kit","name":"Drums","channels":[1]}]}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert_eq!(session.groups().len(), 1, "the readable sections are kept");
        let aside = files_in(&directory)
            .into_iter()
            .find(|name| name.starts_with(&format!("{SESSION_FILE}.channels-damaged-")))
            .expect("the damaged section is kept for recovery");
        assert_eq!(read_json(&directory.join(aside)), serde_json::json!(5));
        assert!(session.notice().expect("reported").contains("channels"));
    }

    #[test]
    fn musicians_json_is_imported_once_and_left_untouched() {
        let directory = test_directory("migrate");
        let legacy = r#"{"192.168.1.30:50000": "Ana", "192.168.1.31:50000": "  "}"#;
        fs::write(directory.join(LEGACY_LABELS_FILE), legacy).expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert!(session.notice().expect("reported").contains("imported"));
        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({ "address:192.168.1.30:50000": { "label": "Ana" } })
        );
        assert_eq!(
            fs::read_to_string(directory.join(LEGACY_LABELS_FILE)).expect("kept"),
            legacy,
            "the old file is left in place, byte for byte"
        );

        // Once the session exists the old file is no longer read.
        fs::write(
            directory.join(LEGACY_LABELS_FILE),
            r#"{"192.168.1.30:50000": "Someone else"}"#,
        )
        .expect("write");
        let reopened = SessionStore::open(&directory);
        assert_eq!(reopened.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert_eq!(reopened.notice(), None);
    }

    #[test]
    fn a_corrupt_musicians_json_imports_nothing_and_is_left_untouched() {
        let directory = test_directory("migrate-corrupt");
        fs::write(directory.join(LEGACY_LABELS_FILE), "not json").expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), None);
        assert!(session.notice().is_some(), "the problem is reported");
        assert_eq!(
            fs::read_to_string(directory.join(LEGACY_LABELS_FILE)).expect("kept"),
            "not json"
        );
        session.set_label(None, ADDRESS, "Ana").expect("saves");
    }
}
