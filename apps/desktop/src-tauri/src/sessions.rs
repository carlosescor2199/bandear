//! Named sessions: snapshots of the desk's session the engineer saves with "Save session as…" and
//! loads later (see [crate::commands::save_session] and [crate::commands::load_session]).
//!
//! Each snapshot is one JSON file in the `sessions` folder of the app's config directory,
//! `sessions/<id>.json`, written atomically as the session itself is (a temporary file flushed to
//! disk, then renamed). It holds the whole session as [crate::session] describes it, at the same
//! version, plus the snapshot's name, when it was saved (unix milliseconds) and each musician's mix
//! at that moment:
//!
//! ```json
//! {
//!   "version": 1,
//!   "musicians": { "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b": { "label": "Ana" } },
//!   "channels": [{ "name": "Kick", "kind": "drums", "hidden": false, "link": false }],
//!   "groups": [{ "id": "g3f9a0c1d2e4b5a6", "name": "Drums", "channels": [1, 2, 3] }],
//!   "presets": [],
//!   "audio": { "host": "wasapi", "device": "INPUT 1/2 (Volt 4)", "bufferFrames": 256 },
//!   "local": {
//!     "mix": { "channels": [], "groups": { "g3f9a0c1d2e4b5a6": 60 }, "volumePercent": 70 },
//!     "output": { "device": "OUTPUT 1/2 (Volt 4)", "firstChannel": 0 }
//!   },
//!   "name": "Sunday service",
//!   "savedAt": 1760641500000,
//!   "mixes": {
//!     "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b": {
//!       "channels": [{ "gain": 80, "pan": 50, "muted": false, "solo": false }],
//!       "groups": { "g3f9a0c1d2e4b5a6": 70 },
//!       "volumePercent": 85,
//!       "muted": false
//!     }
//!   }
//! }
//! ```
//!
//! - `mixes` is keyed as `musicians` is: `client:<client_id>` for a phone that announced a stable
//!   client id, else `address:<ip:port>`. Each mix holds the level, pan, mute and solo of the inputs
//!   the device had when it was saved, in channel order (the first entry is CH 1; a field left out
//!   defaults to 100, 50, false, false), the group levels by group id (so they land on the right
//!   groups whatever their order), and the musician's master volume and master mute. Never the
//!   phone's own level cap, which is the phone's; the engineer's limit and locks are kept with the
//!   musician in `musicians`.
//! - `local` is the session's local section (the engineer's local mix and its output, see
//!   [crate::session]). A snapshot saved while an engine runs keeps the local mix the engine plays;
//!   loading one pushes it through the desk's path and opens its output only when the running
//!   driver offers it. A snapshot without one (saved before the local mix existed) leaves the
//!   desk's own local mix and output as they are.
//! - The session's `recording` section (how this desk records takes) is never saved in a snapshot:
//!   it belongs to the desk, and loading a snapshot keeps the desk's own.
//! - The id is the file's name: `s` and 16 hex digits for a snapshot the desk saved. Only a name of
//!   letters, digits, `-` and `_` is ever read or written, so an id can never reach outside the
//!   folder.
//! - A snapshot is read as tolerantly as the session: every section is read on its own, and one
//!   that cannot be read, or one musician's mix that cannot be read, is left out and named while the
//!   rest loads. A file that is not JSON, has no usable version or whose labels cannot be read is
//!   listed with its problem and cannot be loaded; neither can one written by a newer version. A
//!   snapshot that cannot be read is never changed or moved; it can be deleted.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::session::{
    read_session, unix_millis, versioned_fields, write_atomically, DamagedSection, Session,
    Versioned, SESSION_VERSION,
};

/// The folder of the app config directory the named sessions live in.
pub const SESSIONS_DIRECTORY: &str = "sessions";
/// The longest name of a saved session, in characters.
pub const MAX_SESSION_NAME_LENGTH: usize = 40;
/// The longest id the desk reads, in characters.
const MAX_ID_LENGTH: usize = 64;

/// One musician's mix as a snapshot keeps it (see the module documentation); the session keeps the
/// engineer's local mix in the same shape.
pub use crate::session::SessionMix;

/// A named snapshot of the session, as saved and as read back.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    #[serde(flatten)]
    pub session: Session,
    pub name: String,
    /// When it was saved, in unix milliseconds; `None` for a file that does not say.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<u64>,
    /// Each musician's mix, by the musician's key.
    pub mixes: BTreeMap<String, SessionMix>,
}

/// A snapshot read back for loading, with what of it could not be read and was left out.
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotRead {
    pub id: String,
    pub snapshot: Snapshot,
    /// One sentence for each section, or musician's mix, that could not be read.
    pub damaged: Vec<String>,
}

/// One saved session as the window lists it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotSummary {
    pub id: String,
    /// Its name; the id for a file that names none.
    pub name: String,
    /// When it was saved, in unix milliseconds; null when the file does not say.
    pub saved_at: Option<u64>,
    /// How many musicians' mixes it keeps.
    pub mixes: usize,
    /// Why it cannot be loaded (it cannot be read, or a newer MixLink wrote it); null when it can.
    pub problem: Option<String>,
}

impl SnapshotSummary {
    fn of(id: &str, snapshot: &Snapshot) -> Self {
        Self {
            id: id.to_owned(),
            name: snapshot.name.clone(),
            saved_at: snapshot.saved_at,
            mixes: snapshot.mixes.len(),
            problem: None,
        }
    }
}

/// The named sessions in one folder.
pub struct SnapshotStore {
    directory: PathBuf,
    /// Held across every change, so two saves cannot both take one name or one id.
    files: Mutex<()>,
}

impl SnapshotStore {
    /// The named sessions kept in `directory` (created when the first one is saved).
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            files: Mutex::new(()),
        }
    }

    /// Every saved session, the newest first (a file that does not say when it was saved last,
    /// by id); one that cannot be loaded is listed with its problem. No folder yet is no session.
    pub fn list(&self) -> Result<Vec<SnapshotSummary>, String> {
        let _files = self.lock();
        self.listed()
    }

    /// Reads the session saved as `id` for loading. A session that cannot be read, or that a newer
    /// MixLink wrote, is an error saying so.
    pub fn read(&self, id: &str) -> Result<SnapshotRead, String> {
        let _files = self.lock();
        let bytes = self.bytes(id)?;
        let (snapshot, damaged) = parse_snapshot(id, &bytes)
            .map_err(|problem| format!("the saved session `{id}` cannot be loaded: {problem}"))?;
        Ok(SnapshotRead {
            id: id.to_owned(),
            snapshot,
            damaged,
        })
    }

    /// Saves `session` and the musicians' `mixes` as a new snapshot called `name` (checked as
    /// [session_name] checks it) under an id `new_id` gives, one the folder does not use, and
    /// answers it as listed. A refused name, or a save that fails, leaves no file.
    pub fn save(
        &self,
        name: &str,
        session: Session,
        mixes: BTreeMap<String, SessionMix>,
        new_id: impl FnOnce(&mut HashSet<String>) -> String,
    ) -> Result<SnapshotSummary, String> {
        let _files = self.lock();
        let listed = self.listed()?;
        let name = session_name(name, &listed, None)?;
        let mut taken: HashSet<String> = listed.into_iter().map(|saved| saved.id).collect();
        let id = new_id(&mut taken);
        check_id(&id)?;
        let snapshot = Snapshot {
            session: Session {
                version: SESSION_VERSION,
                ..session
            },
            name,
            saved_at: Some(u64::try_from(unix_millis()).unwrap_or(u64::MAX)),
            mixes,
        };
        write_atomically(&self.path_of(&id), &snapshot)?;
        Ok(SnapshotSummary::of(&id, &snapshot))
    }

    /// Renames the session saved as `id` (the name checked as [session_name] checks it): only its
    /// `name` changes, everything else stays as written. One that cannot be loaded is not renamed.
    pub fn rename(&self, id: &str, name: &str) -> Result<SnapshotSummary, String> {
        let _files = self.lock();
        let listed = self.listed()?;
        let saved = listed
            .iter()
            .find(|saved| saved.id == id)
            .ok_or_else(|| missing(id))?;
        if let Some(problem) = &saved.problem {
            return Err(format!(
                "the saved session `{id}` cannot be renamed: {problem}"
            ));
        }
        let name = session_name(name, &listed, Some(id))?;
        let mut fields = match versioned_fields(&self.bytes(id)?) {
            Versioned::Current(fields) => fields,
            _ => return Err(format!("the saved session `{id}` cannot be renamed")),
        };
        fields.insert("name".to_owned(), Value::String(name.clone()));
        write_atomically(&self.path_of(id), &Value::Object(fields))?;
        Ok(SnapshotSummary {
            name,
            ..saved.clone()
        })
    }

    /// Deletes the session saved as `id`, whether or not it can be read.
    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _files = self.lock();
        check_id(id)?;
        fs::remove_file(self.path_of(id)).map_err(|error| match error.kind() {
            ErrorKind::NotFound => missing(id),
            _ => format!("the saved session `{id}` could not be deleted: {error}"),
        })
    }

    fn listed(&self) -> Result<Vec<SnapshotSummary>, String> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(format!(
                    "the saved sessions in {} could not be listed: {error}",
                    self.directory.display()
                ))
            }
        };
        let mut listed: Vec<SnapshotSummary> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
                    return None;
                }
                let id = path.file_stem()?.to_str()?.to_owned();
                check_id(&id).ok()?;
                Some(summary_of(&id, &path))
            })
            .collect();
        listed.sort_by(|one, other| {
            other
                .saved_at
                .cmp(&one.saved_at)
                .then_with(|| one.id.cmp(&other.id))
        });
        Ok(listed)
    }

    fn bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        check_id(id)?;
        fs::read(self.path_of(id)).map_err(|error| match error.kind() {
            ErrorKind::NotFound => missing(id),
            _ => format!("the saved session `{id}` could not be read: {error}"),
        })
    }

    fn path_of(&self, id: &str) -> PathBuf {
        self.directory.join(format!("{id}.json"))
    }

    // Nothing is kept behind the lock, so a poisoned one guards nothing broken.
    fn lock(&self) -> MutexGuard<'_, ()> {
        self.files.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn missing(id: &str) -> String {
    format!("there is no saved session with the id `{id}`")
}

/// An id the desk reads and writes: 1-64 letters, digits, `-` or `_`, so a file name built from
/// it stays in the folder.
fn check_id(id: &str) -> Result<(), String> {
    let safe = !id.is_empty()
        && id.len() <= MAX_ID_LENGTH
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'));
    if safe {
        Ok(())
    } else {
        Err(format!("`{id}` is not the id of a saved session"))
    }
}

/// `name` trimmed, when it can name a saved session among `listed` (the one with the id `renaming`
/// aside): not empty, at most [MAX_SESSION_NAME_LENGTH] characters, and no other saved session's
/// name in any case, so two never have to be told apart by when they were saved.
pub fn session_name(
    name: &str,
    listed: &[SnapshotSummary],
    renaming: Option<&str>,
) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("a saved session needs a name".to_owned());
    }
    if name.chars().count() > MAX_SESSION_NAME_LENGTH {
        return Err(format!(
            "a session name is at most {MAX_SESSION_NAME_LENGTH} characters"
        ));
    }
    let taken = listed.iter().any(|saved| {
        Some(saved.id.as_str()) != renaming && saved.name.to_lowercase() == name.to_lowercase()
    });
    if taken {
        return Err(format!("a session named \"{name}\" is already saved"));
    }
    Ok(name.to_owned())
}

/// How the window lists the file at `path` saved as `id`.
fn summary_of(id: &str, path: &Path) -> SnapshotSummary {
    let unreadable = |problem: String| SnapshotSummary {
        id: id.to_owned(),
        name: id.to_owned(),
        saved_at: None,
        mixes: 0,
        problem: Some(problem),
    };
    match fs::read(path) {
        Ok(bytes) => match parse_snapshot(id, &bytes) {
            Ok((snapshot, _)) => SnapshotSummary::of(id, &snapshot),
            Err(problem) => unreadable(problem),
        },
        Err(error) => unreadable(format!("it could not be read ({error})")),
    }
}

/// Reads a snapshot saved as `id`, with a sentence for each part that could not be read, or the
/// reason the whole file cannot be used.
fn parse_snapshot(id: &str, bytes: &[u8]) -> Result<(Snapshot, Vec<String>), String> {
    let mut fields = match versioned_fields(bytes) {
        Versioned::Current(fields) => fields,
        Versioned::Newer(version) => {
            return Err(format!(
                "it was saved by a newer MixLink (version {version})"
            ))
        }
        Versioned::Invalid(problem) => return Err(format!("the file {problem}")),
    };
    let name = fields
        .remove("name")
        .and_then(|name| name.as_str().map(|name| name.trim().to_owned()))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| id.to_owned());
    let saved_at = fields.remove("savedAt").and_then(|at| at.as_u64());
    let mut damaged = Vec::new();
    let mixes = match fields.remove("mixes") {
        None => BTreeMap::new(),
        Some(Value::Object(entries)) => entries
            .into_iter()
            .filter_map(|(key, raw)| match SessionMix::deserialize(&raw) {
                Ok(mix) => Some((key, mix)),
                Err(error) => {
                    damaged.push(format!("the saved mix of `{key}` cannot be read ({error})"));
                    None
                }
            })
            .collect(),
        Some(_) => {
            damaged.push("its `mixes` section cannot be read (it is not an object)".to_owned());
            BTreeMap::new()
        }
    };
    let mut sections: Vec<DamagedSection> = Vec::new();
    let session =
        read_session(fields, &mut sections).map_err(|problem| format!("the file {problem}"))?;
    damaged.splice(
        0..0,
        sections.iter().map(|section| {
            format!(
                "its `{}` section cannot be read ({})",
                section.name, section.problem
            )
        }),
    );
    Ok((
        Snapshot {
            session,
            name,
            saved_at,
            mixes,
        },
        damaged,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{
        MusicianEntry, SessionAudio, SessionChannel, SessionEngineer, SessionGroup, SessionLocal,
        SessionMirror, SessionOutput, SessionPreset, SessionPresetChannel, SessionPresetMix,
    };

    /// An empty scratch folder for one test's named sessions.
    fn test_directory(test: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-sessions-tests")
            .join(test);
        let _ = fs::remove_dir_all(&directory);
        directory
    }

    fn ids() -> impl FnMut(&mut HashSet<String>) -> String {
        let mut next = 0;
        move |taken: &mut HashSet<String>| loop {
            next += 1;
            let id = format!("s{next:016x}");
            if taken.insert(id.clone()) {
                return id;
            }
        }
    }

    fn channel(gain: u8, pan: u8, muted: bool, solo: bool) -> SessionPresetChannel {
        SessionPresetChannel {
            gain,
            pan,
            muted,
            solo,
        }
    }

    /// A session with something in every section.
    fn full_session() -> Session {
        let mut musicians = BTreeMap::new();
        musicians.insert(
            "client:phone-ana".to_owned(),
            MusicianEntry {
                limiter: Some(personal_monitoring::LimiterSettings {
                    enabled: true,
                    ceiling_db: -6.0,
                    release_ms: 80.0,
                }),
                label: Some("Ana".to_owned()),
                engineer: Some(SessionEngineer {
                    limit_percent: 60,
                    locked_channels: vec![1],
                }),
                mirror: None,
                buffer: None,
                redundancy: None,
            },
        );
        musicians.insert(
            "address:192.168.1.40:50000".to_owned(),
            MusicianEntry {
                limiter: None,
                label: Some("Rack A".to_owned()),
                engineer: None,
                mirror: Some(SessionMirror {
                    leader: "client:phone-ana".to_owned(),
                    name: "Ana".to_owned(),
                    leader_address: Some("192.168.1.30:50000".to_owned()),
                    follower_address: None,
                }),
                buffer: None,
                redundancy: None,
            },
        );
        Session {
            musicians,
            channels: vec![SessionChannel {
                name: "Kick".to_owned(),
                kind: "drums".to_owned(),
                hidden: false,
                link: false,
            }],
            groups: vec![SessionGroup {
                id: "kit".to_owned(),
                name: "Drums".to_owned(),
                channels: vec![1],
            }],
            presets: vec![SessionPreset {
                id: "p1".to_owned(),
                name: "Verse".to_owned(),
                mix: SessionPresetMix {
                    channels: vec![channel(80, 50, false, false)],
                    groups: BTreeMap::from([("kit".to_owned(), 70)]),
                },
            }],
            audio: Some(SessionAudio {
                host: "wasapi".to_owned(),
                device: Some("INPUT 1/2 (Volt 4)".to_owned()),
                buffer_frames: Some(256),
                asio_driver: None,
            }),
            local: Some(SessionLocal {
                limiter: Some(personal_monitoring::LimiterSettings {
                    enabled: true,
                    ceiling_db: -1.5,
                    release_ms: 50.0,
                }),
                mix: Some(SessionMix {
                    channels: vec![channel(90, 30, false, false)],
                    groups: BTreeMap::from([("kit".to_owned(), 60)]),
                    volume_percent: 70,
                    muted: false,
                }),
                output: Some(SessionOutput {
                    device: "OUTPUT 1/2 (Volt 4)".to_owned(),
                    first_channel: 2,
                }),
            }),
            ..Session::default()
        }
    }

    fn mixes() -> BTreeMap<String, SessionMix> {
        BTreeMap::from([
            (
                "client:phone-ana".to_owned(),
                SessionMix {
                    channels: vec![channel(80, 20, false, true), channel(40, 80, true, false)],
                    groups: BTreeMap::from([("kit".to_owned(), 70), ("vox".to_owned(), 30)]),
                    volume_percent: 85,
                    muted: false,
                },
            ),
            (
                "address:192.168.1.40:50000".to_owned(),
                SessionMix {
                    channels: vec![channel(100, 50, false, false)],
                    groups: BTreeMap::new(),
                    volume_percent: 60,
                    muted: true,
                },
            ),
        ])
    }

    fn write(directory: &Path, id: &str, contents: &str) {
        fs::create_dir_all(directory).expect("the folder should be created");
        fs::write(directory.join(format!("{id}.json")), contents).expect("written");
    }

    #[test]
    fn a_snapshot_keeps_every_section_and_each_musicians_mix_by_key() {
        let directory = test_directory("round-trip");
        let store = SnapshotStore::new(directory.clone());

        let saved = store
            .save(" Sunday service ", full_session(), mixes(), ids())
            .expect("saved");

        assert_eq!(saved.name, "Sunday service", "names are trimmed");
        assert_eq!(saved.mixes, 2);
        assert!(saved.saved_at.is_some_and(|at| at > 0), "{saved:?}");
        let read = store.read(&saved.id).expect("readable");
        assert_eq!(read.damaged, Vec::<String>::new());
        assert_eq!(read.snapshot.session, full_session());
        assert_eq!(read.snapshot.mixes, mixes());
        assert_eq!(read.snapshot.name, "Sunday service");
        let file: Value = serde_json::from_str(
            &fs::read_to_string(directory.join(format!("{}.json", saved.id))).expect("a file"),
        )
        .expect("JSON");
        assert_eq!(file["version"], 1);
        assert_eq!(
            file["mixes"]["client:phone-ana"]["groups"],
            serde_json::json!({ "kit": 70, "vox": 30 }),
            "group levels are kept by group id"
        );
        assert_eq!(
            file["mixes"]["client:phone-ana"]["volumePercent"], 85,
            "the master volume is kept, in the window's words"
        );
        assert!(
            file["mixes"]["client:phone-ana"]
                .get("maxLevelPercent")
                .is_none(),
            "the phone's own cap is never kept"
        );
        assert_eq!(
            file["local"],
            serde_json::json!({
                "mix": {
                    "channels": [{ "gain": 90, "pan": 30, "muted": false, "solo": false }],
                    "groups": { "kit": 60 },
                    "volumePercent": 70,
                    "muted": false
                },
                "output": { "device": "OUTPUT 1/2 (Volt 4)", "firstChannel": 2 },
                "limiter": { "enabled": true, "ceilingDb": -1.5, "releaseMs": 50.0 }
            }),
            "the local mix and its output are kept beside the other sections"
        );
        assert_eq!(
            file["musicians"]["client:phone-ana"]["limiter"],
            serde_json::json!({ "enabled": true, "ceilingDb": -6.0, "releaseMs": 80.0 }),
            "each musician's limiter is kept with them"
        );
    }

    #[test]
    fn a_damaged_local_section_of_a_snapshot_costs_only_itself() {
        let directory = test_directory("damaged-local");
        let store = SnapshotStore::new(directory.clone());
        write(
            &directory,
            "show",
            r#"{"version":1,"name":"Show","groups":[{"id":"kit","name":"Drums","channels":[1]}],
                "local":{"output":{"firstChannel":"three"}},
                "mixes":{"client:phone-ana":{"volumePercent":70}}}"#,
        );

        let read = store.read("show").expect("what can be read loads");

        assert_eq!(read.snapshot.session.local, None);
        assert_eq!(read.snapshot.session.groups.len(), 1);
        assert_eq!(read.snapshot.mixes.len(), 1);
        assert_eq!(read.damaged.len(), 1, "{:?}", read.damaged);
        assert!(read.damaged[0].contains("`local`"), "{:?}", read.damaged);
    }

    #[test]
    fn saved_sessions_are_listed_newest_first_and_a_missing_folder_is_none() {
        let directory = test_directory("list");
        let store = SnapshotStore::new(directory.clone());
        assert_eq!(store.list().expect("listed"), Vec::new());

        write(
            &directory,
            "older",
            r#"{"version":1,"name":"Rehearsal","savedAt":1000,"mixes":{"address:192.168.1.40:50000":{}}}"#,
        );
        write(
            &directory,
            "newer",
            r#"{"version":1,"name":"Show","savedAt":2000}"#,
        );
        write(&directory, "undated", r#"{"version":1}"#);
        fs::write(directory.join("notes.txt"), "not a session").expect("written");
        fs::write(directory.join("s1.json.tmp"), "half written").expect("written");

        let listed = store.list().expect("listed");

        let names: Vec<(&str, &str, usize)> = listed
            .iter()
            .map(|saved| (saved.id.as_str(), saved.name.as_str(), saved.mixes))
            .collect();
        assert_eq!(
            names,
            vec![
                ("newer", "Show", 0),
                ("older", "Rehearsal", 1),
                ("undated", "undated", 0)
            ],
            "a file without a name is listed by its id"
        );
        assert!(listed.iter().all(|saved| saved.problem.is_none()));
    }

    #[test]
    fn a_damaged_section_or_mix_costs_only_itself_and_is_named() {
        let directory = test_directory("damaged-section");
        let store = SnapshotStore::new(directory.clone());
        let contents = r#"{
            "version": 1,
            "name": "Show",
            "musicians": { "client:phone-ana": { "label": "Ana" } },
            "groups": "not a list",
            "channels": [{ "name": "Kick", "kind": "drums" }],
            "mixes": {
                "client:phone-ana": { "volumePercent": 70 },
                "address:192.168.1.40:50000": { "channels": "loud" }
            }
        }"#;
        write(&directory, "show", contents);

        let read = store.read("show").expect("what can be read loads");

        assert_eq!(read.snapshot.session.groups, Vec::new());
        assert_eq!(read.snapshot.session.channels.len(), 1);
        assert_eq!(
            read.snapshot.session.musicians["client:phone-ana"].label,
            Some("Ana".to_owned())
        );
        assert_eq!(
            read.snapshot
                .mixes
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["client:phone-ana"]
        );
        assert_eq!(read.snapshot.mixes["client:phone-ana"].volume_percent, 70);
        assert_eq!(read.damaged.len(), 2, "{:?}", read.damaged);
        assert!(read.damaged[0].contains("`groups`"), "{:?}", read.damaged);
        assert!(
            read.damaged[1].contains("address:192.168.1.40:50000"),
            "{:?}",
            read.damaged
        );
        assert_eq!(
            fs::read_to_string(directory.join("show.json")).expect("kept"),
            contents,
            "reading never changes the file"
        );
    }

    #[test]
    fn a_snapshot_that_cannot_be_read_is_listed_with_its_problem_and_never_changed() {
        let directory = test_directory("unreadable");
        let store = SnapshotStore::new(directory.clone());
        for (id, contents, says) in [
            ("broken", "this is not json", "not valid JSON"),
            ("newer", r#"{"version":2,"name":"Future"}"#, "newer"),
            (
                "labels",
                r#"{"version":1,"musicians":"Ana"}"#,
                "not a session",
            ),
        ] {
            write(&directory, id, contents);
            let error = store.read(id).expect_err("it cannot be loaded");
            assert!(error.contains(says), "{id}: {error}");
            let error = store.rename(id, "Better").expect_err("nor renamed");
            assert!(error.contains("cannot be renamed"), "{id}: {error}");
            assert_eq!(
                fs::read_to_string(directory.join(format!("{id}.json"))).expect("kept"),
                contents,
                "{id} is left as it was"
            );
        }

        let listed = store.list().expect("the folder is still listed");

        assert_eq!(listed.len(), 3);
        for saved in &listed {
            assert!(saved.problem.is_some(), "{saved:?}");
        }
        store
            .delete("broken")
            .expect("a damaged snapshot can be deleted");
        assert_eq!(store.list().expect("listed").len(), 2);
    }

    #[test]
    fn names_are_trimmed_unique_in_any_case_and_at_most_forty_characters() {
        let store = SnapshotStore::new(test_directory("names"));
        store
            .save("Show", Session::default(), BTreeMap::new(), ids())
            .expect("saved");

        for (name, says) in [
            ("   ", "needs a name"),
            (" show ", "already saved"),
            (&"x".repeat(41), "at most 40"),
        ] {
            let error = store
                .save(name, Session::default(), BTreeMap::new(), ids())
                .expect_err("refused");
            assert!(error.contains(says), "{name:?}: {error}");
        }
        store
            .save(&"x".repeat(40), Session::default(), BTreeMap::new(), ids())
            .expect("forty characters are a name");
        assert_eq!(store.list().expect("listed").len(), 2, "no refused file");
    }

    #[test]
    fn a_rename_changes_only_the_name_and_a_delete_removes_the_file() {
        let directory = test_directory("rename-delete");
        let store = SnapshotStore::new(directory.clone());
        let show = store
            .save("Show", full_session(), mixes(), ids())
            .expect("saved");
        let rehearsal = store
            .save("Rehearsal", Session::default(), BTreeMap::new(), ids())
            .expect("saved");

        let error = store
            .rename(&show.id, "REHEARSAL")
            .expect_err("another session has that name");
        assert!(error.contains("already saved"), "{error}");
        let renamed = store.rename(&show.id, " Show, late ").expect("renamed");
        assert_eq!(renamed.name, "Show, late");
        store
            .rename(&show.id, "show, LATE")
            .expect("its own name in another case is fine");

        let read = store.read(&show.id).expect("readable");
        assert_eq!(read.snapshot.name, "show, LATE");
        assert_eq!(read.snapshot.session, full_session(), "the rest is kept");
        assert_eq!(read.snapshot.mixes, mixes());
        assert_eq!(read.snapshot.saved_at, show.saved_at);

        store.delete(&rehearsal.id).expect("deleted");
        assert!(!directory.join(format!("{}.json", rehearsal.id)).exists());
        let error = store.delete(&rehearsal.id).expect_err("already gone");
        assert!(error.contains("no saved session"), "{error}");
        let error = store.read(&rehearsal.id).expect_err("gone");
        assert!(error.contains("no saved session"), "{error}");
    }

    #[test]
    fn an_id_that_could_reach_outside_the_folder_is_refused() {
        let directory = test_directory("ids");
        let store = SnapshotStore::new(directory.join("sessions"));
        fs::create_dir_all(&directory).expect("folder");
        fs::write(directory.join("secret.json"), r#"{"version":1}"#).expect("written");

        for id in ["../secret", "..\\secret", "", "a/b", "s.json"] {
            let error = store.read(id).expect_err("refused");
            assert!(error.contains("is not the id"), "{id:?}: {error}");
            assert!(store.delete(id).is_err(), "{id:?}");
        }
        assert!(directory.join("secret.json").exists());
    }

    #[test]
    fn a_failed_save_leaves_no_snapshot() {
        let directory = test_directory("failed-save");
        let store = SnapshotStore::new(directory.clone());
        fs::create_dir_all(&directory).expect("folder");
        // A folder where the temporary file goes makes the write fail before the rename.
        fs::create_dir(directory.join("s0000000000000001.json.tmp")).expect("blocker");

        let error = store
            .save("Show", Session::default(), BTreeMap::new(), ids())
            .expect_err("the write cannot happen");

        assert!(error.contains("could not save"), "{error}");
        assert_eq!(store.list().expect("listed"), Vec::new());
    }
}
