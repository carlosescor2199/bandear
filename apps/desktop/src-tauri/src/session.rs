//! The desktop session: what this desk keeps between runs, in one versioned JSON file in the app's
//! config directory.
//!
//! The session is a desktop-app concern only: the engine never reads the file, and only what the
//! desk applies from it (the channel details and the groups) ever reaches a client. Version 1 has
//! this shape:
//!
//! ```json
//! {
//!   "version": 1,
//!   "musicians": {
//!     "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b": {
//!       "label": "Ana",
//!       "engineer": { "limitPercent": 60, "lockedChannels": [1, 2] }
//!     },
//!     "address:192.168.1.40:50000": {
//!       "label": "Rack A",
//!       "mirror": {
//!         "leader": "client:0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b",
//!         "name": "Ana",
//!         "leaderAddress": "192.168.1.30:50000"
//!       }
//!     }
//!   },
//!   "channels": [
//!     { "name": "Kick", "kind": "drums", "hidden": false, "link": false },
//!     { "name": "Keys L", "kind": "keys", "hidden": false, "link": true },
//!     { "name": "Keys R", "kind": "keys", "hidden": false, "link": false }
//!   ],
//!   "groups": [
//!     { "id": "g3f9a0c1d2e4b5a6", "name": "Drums", "channels": [1, 2, 3] }
//!   ],
//!   "presets": [
//!     {
//!       "id": "p0c4d2e6f8a1b3c5d",
//!       "name": "Vocal heavy",
//!       "mix": {
//!         "channels": [{ "gain": 80, "pan": 50, "muted": false, "solo": false }],
//!         "groups": { "g3f9a0c1d2e4b5a6": 70 }
//!       }
//!     }
//!   ],
//!   "audio": {
//!     "host": "wasapi",
//!     "device": "INPUT 1/2 (Volt 4)",
//!     "bufferFrames": 256,
//!     "asioDriver": null
//!   }
//! }
//! ```
//!
//! - `musicians` holds what the desk keeps about each musician: the engineer's local label and the
//!   engineer's controls (`engineer`: the limit on what the musician hears, 0-100, and the channels
//!   their phone cannot change, as 1-based channel numbers; absent when the engineer set neither).
//!   A phone that announced a stable client id is keyed `client:<client_id>`; a configured
//!   target, or a phone that sent no id, is keyed `address:<ip:port>`, the address exactly as the
//!   status reports it. Both are looked up by the client key first and the address key second, so
//!   a phone keeps its label and its controls when DHCP gives it another IP. The controls are given
//!   to every engine the desk starts (see [crate::commands::start_engine]), which puts them in
//!   force when that musician registers. An entry's `engineer` was added within version 1: an
//!   earlier build reads the entry and keeps its label, but drops the controls when it saves; an
//!   `engineer` that cannot be read costs only itself (it reads as none). An entry's `mirror` (added
//!   the same way) says whose mix this musician's follows: the leader's key (`client:` or
//!   `address:`, as above) and the name the follower's phone is told ("Following Ana's mix"). It is
//!   given to every engine the desk starts, which puts it in force whenever the follower registers;
//!   one that cannot be read costs only itself. When the leader is kept by their phone's key, the
//!   mirror also keeps the address they had when it was set (`leaderAddress`), and when the
//!   follower is, theirs (`followerAddress`): with no engine running these are the only way the
//!   desk knows that a phone and an address are one musician, so a mirror that would go round in a
//!   circle is refused whichever key each mirror was kept under. Both may be absent.
//! - `groups` holds the engineer's groups in their order: each with the stable id every musician's
//!   group level follows, a name and 1-based channel numbers (as the engineer numbers them). They are
//!   applied to every engine the desk starts (see [crate::commands::start_engine]) and replaced by
//!   every edit ([crate::commands::set_groups]). The section was reserved in the first version and
//!   every earlier build wrote it empty and kept whatever it read, so giving its entries a shape
//!   does not change the version: an earlier build still reads and keeps it.
//! - `channels` holds the engineer's details of each input, in channel order (the first entry is
//!   CH 1): a name (empty for none), an instrument kind (one of the engine's kinds, `other` by
//!   default), whether it is hidden, and `link` on the first input of a stereo pair. An entry may
//!   leave out any field, which then has its default. They are applied to every engine the desk
//!   starts and replaced by every edit ([crate::commands::set_channels]); an engine whose device has
//!   fewer inputs uses the details of the inputs it has, and the session keeps them all. Like
//!   `groups`, the section was reserved in the first version and every earlier build wrote it empty
//!   and kept whatever it read, so its shape does not change the version.
//! - `presets` holds the engineer's mix presets, in the order they were saved (see
//!   [crate::commands::save_preset]): each with a stable id, a name and the mix it keeps, the
//!   level, pan, mute and solo of each input in channel order (the first entry is CH 1; inputs the
//!   current device lacks are kept) and the group levels by group id, so a preset still lands on
//!   the right groups after they were edited. A preset never holds a musician's master volume,
//!   level cap or the engineer's controls. A channel entry may leave out any field (level 100, pan
//!   50, neither muted nor soloed). The section was reserved in the first version and every earlier
//!   build wrote it empty and kept whatever it read, so its shape does not change the version.
//! - `audio` is the audio input the desk last ran with: the driver (`auto`, `wasapi` or `asio`), the
//!   input device picked under `auto` or `wasapi` (null for the system's default input), the buffer
//!   in frames (null for the engine's default) and the ASIO driver the engineer picked. The window
//!   reads it when it opens ([crate::commands::audio_settings]) and builds every start from it, and
//!   keeps a new choice only once an engine started or switched with it
//!   ([crate::commands::set_audio_settings]). It is absent until then. Any field may be left out; a
//!   choice that names ASIO without a driver is read as written, and the window treats it as no ASIO
//!   (ASIO only ever starts on a driver the engineer picked). The section was added within version
//!   1: an earlier build reads the file and drops it when it saves.
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
//!
//! The engineer's named sessions are snapshots of this session, with each musician's mix, kept in
//! the `sessions` folder beside it (see [crate::sessions]); loading one replaces this session whole
//! ([SessionStore::replace]).

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// The engineer's details of each input, in channel order.
    #[serde(default)]
    pub channels: Vec<SessionChannel>,
    /// The engineer's groups, in their order.
    #[serde(default)]
    pub groups: Vec<SessionGroup>,
    /// The engineer's mix presets, in the order they were saved.
    #[serde(default)]
    pub presets: Vec<SessionPreset>,
    /// The audio input the desk last ran with; absent until an engine ran with a choice.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio: Option<SessionAudio>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            version: SESSION_VERSION,
            musicians: BTreeMap::new(),
            channels: Vec::new(),
            groups: Vec::new(),
            presets: Vec::new(),
            audio: None,
        }
    }
}

impl Session {
    /// What the session keeps for the musician with this client id at this address, read with
    /// `kept`: the entry kept for the phone first, then the one kept for the address.
    fn resolve<T>(
        &self,
        client_id: Option<&str>,
        address: &str,
        kept: impl Fn(&MusicianEntry) -> Option<T>,
    ) -> Option<T> {
        let at = |key: String| self.musicians.get(&key).and_then(&kept);
        client_id
            .and_then(|id| at(client_key(id)))
            .or_else(|| at(address_key(address)))
    }

    /// The engineer's controls for the musician with this client id at this address: those kept
    /// for the phone first, then those kept for the address.
    pub fn engineer_for(&self, client_id: Option<&str>, address: &str) -> Option<SessionEngineer> {
        self.resolve(client_id, address, |entry| entry.engineer.clone())
    }

    /// Every musician's controls the session keeps, each with its key ([client_key] or
    /// [address_key]), in key order.
    pub fn engineer_controls(&self) -> Vec<(String, SessionEngineer)> {
        self.musicians
            .iter()
            .filter_map(|(key, entry)| Some((key.clone(), entry.engineer.clone()?)))
            .collect()
    }

    /// The mirror kept for the musician with this client id at this address: the one kept for the
    /// phone first, then the one kept for the address.
    pub fn mirror_for(&self, client_id: Option<&str>, address: &str) -> Option<SessionMirror> {
        self.resolve(client_id, address, |entry| entry.mirror.clone())
    }

    /// Every mirror the session keeps, each with its follower's key ([client_key] or
    /// [address_key]), in key order.
    pub fn mirrors(&self) -> Vec<(String, SessionMirror)> {
        self.musicians
            .iter()
            .filter_map(|(key, entry)| Some((key.clone(), entry.mirror.clone()?)))
            .collect()
    }
}

/// The audio input the desk last ran with, as the window starts every engine from it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionAudio {
    /// The driver: `auto` (the system's own), `wasapi` or `asio`.
    #[serde(default = "auto_host")]
    pub host: String,
    /// The input device picked under `auto` or `wasapi`; `None` for the system's default input.
    #[serde(default)]
    pub device: Option<String>,
    /// The capture buffer in frames; `None` for the engine's default.
    #[serde(default)]
    pub buffer_frames: Option<u32>,
    /// The ASIO driver the engineer picked, the only one an ASIO start ever opens.
    #[serde(default)]
    pub asio_driver: Option<String>,
}

fn auto_host() -> String {
    "auto".to_owned()
}

/// What the desk keeps about one musician.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MusicianEntry {
    /// The engineer's local label, shown while the musician's phone announced no name of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// The engineer's limit and locks for this musician; `None` when they are the defaults (no
    /// limit, nothing locked).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_engineer"
    )]
    pub engineer: Option<SessionEngineer>,
    /// Whose mix this musician's follows; `None` when it follows nobody.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient_mirror"
    )]
    pub mirror: Option<SessionMirror>,
}

/// A mirror as the desk keeps it for its follower (see [crate::commands::set_mirror]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMirror {
    /// The leader's key: [client_key] for a phone that announced a client id, else [address_key].
    pub leader: String,
    /// The name the follower's phone is told it follows.
    #[serde(default)]
    pub name: String,
    /// The leader's address when the mirror was set, kept when [Self::leader] is their phone's key,
    /// so the desk knows both keys of the leader with no engine running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_address: Option<String>,
    /// The follower's address when the mirror was set, kept when the mirror is kept under their
    /// phone's key, so the desk knows both keys of the follower with no engine running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follower_address: Option<String>,
}

/// Reads an entry's `mirror` without ever failing the entry, as [lenient_engineer] does.
fn lenient_mirror<'de, D>(deserializer: D) -> Result<Option<SessionMirror>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| SessionMirror::deserialize(value).ok()))
}

/// One of the engineer's mix presets as the desk keeps it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPreset {
    /// The preset's stable id, given by the desk when it was saved.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub mix: SessionPresetMix,
}

/// What a preset keeps of a mix: each input's values in channel order, and the group levels by
/// group id.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPresetMix {
    #[serde(default)]
    pub channels: Vec<SessionPresetChannel>,
    #[serde(default)]
    pub groups: BTreeMap<String, u8>,
}

/// One input of a preset: its level and pan (0-100), mute and solo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionPresetChannel {
    #[serde(default = "full_level")]
    pub gain: u8,
    #[serde(default = "centre")]
    pub pan: u8,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub solo: bool,
}

fn full_level() -> u8 {
    100
}

fn centre() -> u8 {
    50
}

/// The engineer's controls for one musician as the desk keeps them (see
/// [personal_monitoring::EngineerControls]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEngineer {
    /// The limit on what the musician hears, 0-100; 100 is no limit.
    #[serde(default = "no_limit")]
    pub limit_percent: u8,
    /// The channels the musician's phone cannot change, as 1-based channel numbers in order.
    #[serde(default)]
    pub locked_channels: Vec<usize>,
}

impl Default for SessionEngineer {
    fn default() -> Self {
        Self {
            limit_percent: no_limit(),
            locked_channels: Vec::new(),
        }
    }
}

fn no_limit() -> u8 {
    100
}

/// Reads an entry's `engineer` without ever failing the entry: one that cannot be read is none, so
/// a damaged limit or lock list costs only itself and never the labels.
fn lenient_engineer<'de, D>(deserializer: D) -> Result<Option<SessionEngineer>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<Value>::deserialize(deserializer)?;
    Ok(value.and_then(|value| SessionEngineer::deserialize(value).ok()))
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

/// The engineer's details of one input as the desk keeps them; the list is in channel order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionChannel {
    /// The input's name; empty when it has none.
    #[serde(default)]
    pub name: String,
    /// The instrument kind's name, as the engine names its kinds (`voice`, ..., `other`). Kept as
    /// written, so a kind this build does not know survives a save; it applies as `other`.
    #[serde(default = "other_kind")]
    pub kind: String,
    #[serde(default)]
    pub hidden: bool,
    /// This input and the next one are a stereo pair; set on the pair's first input only.
    #[serde(default)]
    pub link: bool,
}

fn other_kind() -> String {
    "other".to_owned()
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
    /// The channel details a running engine took last could not be saved, so the session's are
    /// behind the engine's (see [Self::channels_behind]).
    channels_behind: AtomicBool,
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
            channels_behind: AtomicBool::new(false),
        }
    }

    /// Whether the channel details a running engine took last could not be saved, so the
    /// session's are behind the engine's and the session's kinds must not stand in for the
    /// engine's (see [crate::commands::channel_table]). Every save of the channel details (an edit
    /// or a loaded session) clears it; [Self::set_channels_behind] sets it, and a start, whose
    /// engine takes the session's details, clears it.
    pub fn channels_behind(&self) -> bool {
        self.channels_behind.load(Ordering::SeqCst)
    }

    /// Records whether the session's channel details are behind the running engine's (see
    /// [Self::channels_behind]).
    pub fn set_channels_behind(&self, behind: bool) {
        self.channels_behind.store(behind, Ordering::SeqCst);
    }

    /// What went wrong or changed when the session was opened (a file moved aside, a newer
    /// session left alone, labels imported), or `None` when it simply loaded.
    pub fn notice(&self) -> Option<String> {
        self.lock().notice.clone()
    }

    /// The config directory the session file lives in.
    pub fn directory(&self) -> &Path {
        self.path.parent().unwrap_or_else(|| Path::new("."))
    }

    /// The whole session as the desk keeps it now, for a named snapshot of it (see
    /// [crate::sessions]).
    pub fn current(&self) -> Session {
        self.lock().session.clone()
    }

    /// `Ok` when a change may be saved, else why the session file must not be replaced (one
    /// written by a newer version, or one that could not be set aside), so a change that touches
    /// more than the session can be refused before anything else changes.
    pub fn check_saves(&self) -> Result<(), String> {
        match &self.lock().refusal {
            Some(refusal) => Err(refusal.clone()),
            None => Ok(()),
        }
    }

    /// Replaces the whole session with `session` (a named snapshot loaded, see
    /// [crate::commands::load_session]), at this build's version. The file is written before
    /// success is reported; a save that fails, or is refused over a session this build must not
    /// replace, changes nothing.
    pub fn replace(&self, session: Session) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let session = Session {
            version: SESSION_VERSION,
            ..session
        };
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        self.set_channels_behind(false);
        Ok(())
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

    /// The engineer's controls for the musician with this client id at this address: those kept for
    /// the phone first, then those kept for the address.
    pub fn engineer_for(&self, client_id: Option<&str>, address: &str) -> Option<SessionEngineer> {
        self.lock().session.engineer_for(client_id, address)
    }

    /// Every musician's controls the session keeps, each with its key ([client_key] or
    /// [address_key]), in key order.
    pub fn engineer_controls(&self) -> Vec<(String, SessionEngineer)> {
        self.lock().session.engineer_controls()
    }

    /// Keeps the engineer's controls for the musician with this client id at this address, as
    /// [Self::set_label] keeps a label: under the phone's key, dropping any kept for the address,
    /// or under the address for a musician without one. Controls equal to the defaults are kept as
    /// none. The file is written before success is reported; a save that fails, or is refused over
    /// a session this build must not replace, changes nothing.
    pub fn set_engineer(
        &self,
        client_id: Option<&str>,
        address: &str,
        engineer: SessionEngineer,
    ) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let engineer = (engineer != SessionEngineer::default()).then_some(engineer);
        let mut session = loaded.session.clone();
        match client_id {
            Some(id) => {
                update_entry(&mut session, address_key(address), |entry| {
                    entry.engineer = None
                });
                update_entry(&mut session, client_key(id), |entry| {
                    entry.engineer = engineer
                });
            }
            None => update_entry(&mut session, address_key(address), |entry| {
                entry.engineer = engineer
            }),
        }
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        Ok(())
    }

    /// The mirror kept for the musician with this client id at this address: the one kept for the
    /// phone first, then the one kept for the address.
    pub fn mirror_for(&self, client_id: Option<&str>, address: &str) -> Option<SessionMirror> {
        self.lock().session.mirror_for(client_id, address)
    }

    /// Every mirror the session keeps, each with its follower's key ([client_key] or
    /// [address_key]), in key order.
    pub fn mirrors(&self) -> Vec<(String, SessionMirror)> {
        self.lock().session.mirrors()
    }

    /// Keeps the mirror of the musician with this client id at this address, or drops it with
    /// `None`, as [Self::set_engineer] keeps controls: under the phone's key, dropping any kept for
    /// the address, or under the address for a musician without one. The file is written before
    /// success is reported; a save that fails, or is refused over a session this build must not
    /// replace, changes nothing.
    pub fn set_mirror(
        &self,
        client_id: Option<&str>,
        address: &str,
        mirror: Option<SessionMirror>,
    ) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let mut session = loaded.session.clone();
        match client_id {
            Some(id) => {
                update_entry(&mut session, address_key(address), |entry| {
                    entry.mirror = None
                });
                update_entry(&mut session, client_key(id), |entry| entry.mirror = mirror);
            }
            None => update_entry(&mut session, address_key(address), |entry| {
                entry.mirror = mirror
            }),
        }
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        Ok(())
    }

    /// The engineer's mix presets, in the order they were saved.
    pub fn presets(&self) -> Vec<SessionPreset> {
        self.lock().session.presets.clone()
    }

    /// Replaces the engineer's mix presets. The file is written before success is reported; a save
    /// that fails, or is refused over a session this build must not replace, changes nothing.
    pub fn set_presets(&self, presets: Vec<SessionPreset>) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let session = Session {
            presets,
            ..loaded.session.clone()
        };
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

    /// The engineer's details of each input, in channel order.
    pub fn channels(&self) -> Vec<SessionChannel> {
        self.lock().session.channels.clone()
    }

    /// Replaces the engineer's channel details. The file is written before success is reported; a
    /// save that fails, or is refused over a session this build must not replace, changes nothing.
    pub fn set_channels(&self, channels: Vec<SessionChannel>) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let session = Session {
            channels,
            ..loaded.session.clone()
        };
        write_atomically(&self.path, &session)?;
        loaded.session = session;
        self.set_channels_behind(false);
        Ok(())
    }

    /// The audio input the desk last ran with, or `None` when none was kept.
    pub fn audio(&self) -> Option<SessionAudio> {
        self.lock().session.audio.clone()
    }

    /// Keeps the audio input the desk runs with. The file is written before success is reported; a
    /// save that fails, or is refused over a session this build must not replace, changes nothing.
    pub fn set_audio(&self, audio: SessionAudio) -> Result<(), String> {
        let mut loaded = self.lock();
        if let Some(refusal) = &loaded.refusal {
            return Err(refusal.clone());
        }
        let session = Session {
            audio: Some(audio),
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
    update_entry(session, key, |entry| entry.label = label);
}

/// Changes the entry `key` with `change`, creating it when needed; an entry left with nothing in
/// it is removed.
fn update_entry(session: &mut Session, key: String, change: impl FnOnce(&mut MusicianEntry)) {
    let entry = session.musicians.entry(key.clone()).or_default();
    change(entry);
    if *entry == MusicianEntry::default() {
        session.musicians.remove(&key);
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
pub(crate) struct DamagedSection {
    pub(crate) name: &'static str,
    pub(crate) raw: Value,
    pub(crate) problem: String,
}

/// What a versioned file of this format holds once its version was checked: its fields, or why it
/// cannot be used.
pub(crate) enum Versioned {
    Current(Map<String, Value>),
    Newer(u64),
    Invalid(String),
}

/// Reads `bytes` as a JSON object written at [SESSION_VERSION]: the session file and every named
/// snapshot of it (see [crate::sessions]) share the check.
pub(crate) fn versioned_fields(bytes: &[u8]) -> Versioned {
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(value) => value,
        Err(error) => return Versioned::Invalid(format!("is not valid JSON ({error})")),
    };
    let Some(version) = value.get("version").and_then(Value::as_u64) else {
        return Versioned::Invalid("has no version".to_owned());
    };
    if version > SESSION_VERSION {
        return Versioned::Newer(version);
    }
    if version != SESSION_VERSION {
        return Versioned::Invalid(format!("has version {version}, which no MixLink writes"));
    }
    match value {
        Value::Object(fields) => Versioned::Current(fields),
        _ => Versioned::Invalid("is not a session object".to_owned()),
    }
}

/// Reads a session out of a file's `fields`. Each section besides the labels is read on its own,
/// so one that is damaged costs only itself (it starts empty and is listed in `damaged`); fields
/// that are no part of a session are ignored. A file whose labels cannot be read is not a session
/// this build can use: that is the error.
pub(crate) fn read_session(
    mut fields: Map<String, Value>,
    damaged: &mut Vec<DamagedSection>,
) -> Result<Session, String> {
    let groups = take_section(&mut fields, "groups", damaged);
    let channels = take_section(&mut fields, "channels", damaged);
    let presets = take_section(&mut fields, "presets", damaged);
    let audio = take_section(&mut fields, "audio", damaged);
    serde_json::from_value::<Session>(Value::Object(fields))
        .map(|session| Session {
            groups,
            channels,
            presets,
            audio,
            ..session
        })
        .map_err(|error| format!("is not a session ({error})"))
}

fn parse(bytes: &[u8]) -> Parsed {
    match versioned_fields(bytes) {
        Versioned::Current(fields) => {
            let mut damaged = Vec::new();
            match read_session(fields, &mut damaged) {
                Ok(session) => Parsed::Current(session, damaged),
                Err(problem) => Parsed::Invalid(problem),
            }
        }
        Versioned::Newer(version) => Parsed::Newer(version),
        Versioned::Invalid(problem) => Parsed::Invalid(problem),
    }
}

/// Takes the section `name` out of the session's fields and reads it on its own. A section that
/// cannot be read starts empty and is listed in `damaged` with its raw JSON.
pub(crate) fn take_section<T: DeserializeOwned + Default>(
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

pub(crate) fn unix_millis() -> u128 {
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

/// Replaces the file at `path` with `value` as JSON (the session, or a named snapshot of it): a
/// temporary file `<name>.tmp` in the same directory, flushed to disk, then renamed over the old
/// one. On failure the old file is untouched.
pub(crate) fn write_atomically(path: &Path, value: &impl Serialize) -> Result<(), String> {
    if let Some(directory) = path.parent() {
        fs::create_dir_all(directory).map_err(|error| {
            format!(
                "could not create the config directory {}: {error}",
                directory.display()
            )
        })?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|error| format!("could not serialize the session: {error}"))?;
    let name = path
        .file_name()
        .map_or_else(|| SESSION_FILE.into(), |name| name.to_string_lossy());
    let temporary = path.with_file_name(format!("{name}.tmp"));
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
    fn the_other_sections_are_written_back_as_they_were_read() {
        let directory = test_directory("reserved");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{},"channels":[{"name":"Kick","kind":"drums","hidden":false,"link":false}],"groups":[],"presets":[{"id":"p1","name":"Verse","mix":{"channels":[{"gain":80,"pan":20,"muted":true,"solo":false}],"groups":{"kit":70}}}]}"#,
        )
        .expect("write");

        SessionStore::open(&directory)
            .set_label(None, ADDRESS, "Ana")
            .expect("set");

        let saved = read_json(&directory.join(SESSION_FILE));
        assert_eq!(
            saved["channels"],
            serde_json::json!([{ "name": "Kick", "kind": "drums", "hidden": false, "link": false }])
        );
        assert_eq!(
            saved["presets"],
            serde_json::json!([{
                "id": "p1",
                "name": "Verse",
                "mix": {
                    "channels": [{ "gain": 80, "pan": 20, "muted": true, "solo": false }],
                    "groups": { "kit": 70 }
                }
            }])
        );
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
                "channels":[{"name":"Kick","kind":"drums","hidden":false,"link":false}],
                "groups":[{"id":1}],"presets":[]}"#,
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
        assert_eq!(
            saved["channels"],
            serde_json::json!([{ "name": "Kick", "kind": "drums", "hidden": false, "link": false }])
        );
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

    fn channel(name: &str, kind: &str, hidden: bool, link: bool) -> SessionChannel {
        SessionChannel {
            name: name.to_owned(),
            kind: kind.to_owned(),
            hidden,
            link,
        }
    }

    #[test]
    fn channel_details_are_kept_in_channel_order_and_survive_a_reload() {
        let directory = test_directory("channels");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Ana").expect("set");
        let channels = vec![
            channel("Kick", "drums", false, false),
            channel("Keys L", "keys", false, true),
            channel("Keys R", "keys", false, false),
            channel("", "other", true, false),
        ];

        session
            .set_channels(channels.clone())
            .expect("the details are saved");

        assert_eq!(session.channels(), channels);
        assert_eq!(SessionStore::open(&directory).channels(), channels);
        assert_eq!(
            read_json(&directory.join(SESSION_FILE)),
            serde_json::json!({
                "version": 1,
                "musicians": { "address:192.168.1.30:50000": { "label": "Ana" } },
                "channels": [
                    { "name": "Kick", "kind": "drums", "hidden": false, "link": false },
                    { "name": "Keys L", "kind": "keys", "hidden": false, "link": true },
                    { "name": "Keys R", "kind": "keys", "hidden": false, "link": false },
                    { "name": "", "kind": "other", "hidden": true, "link": false }
                ],
                "groups": [],
                "presets": []
            }),
            "the same version 1 file: every earlier build wrote the section empty and keeps it"
        );
    }

    #[test]
    fn a_channel_entry_may_leave_out_what_it_does_not_change() {
        let directory = test_directory("channels-short");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"channels":[{"name":"Kick"},{}]}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(
            session.channels(),
            vec![
                channel("Kick", "other", false, false),
                channel("", "other", false, false)
            ]
        );
        assert_eq!(session.notice(), None);
    }

    #[test]
    fn a_misshapen_channel_entry_sets_the_section_aside_and_the_labels_are_kept() {
        let directory = test_directory("channels-misshapen");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{"address:192.168.1.30:50000":{"label":"Ana"}},
                "channels":[{"name":5}],"groups":[{"id":"kit","name":"Drums","channels":[1]}]}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert_eq!(session.groups().len(), 1, "the readable sections are kept");
        assert_eq!(session.channels(), Vec::new(), "the details start empty");
        let aside = files_in(&directory)
            .into_iter()
            .find(|name| name.starts_with(&format!("{SESSION_FILE}.channels-damaged-")))
            .expect("the damaged section is kept for recovery");
        assert_eq!(
            read_json(&directory.join(aside)),
            serde_json::json!([{ "name": 5 }])
        );
        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["channels"],
            serde_json::json!([])
        );
    }

    #[test]
    fn saving_channels_is_refused_over_a_newer_session_and_a_failed_save_changes_nothing() {
        let newer_directory = test_directory("channels-newer");
        fs::write(newer_directory.join(SESSION_FILE), r#"{"version":2}"#).expect("write");
        let newer = SessionStore::open(&newer_directory);
        assert!(newer
            .set_channels(vec![channel("Kick", "drums", false, false)])
            .expect_err("refused")
            .contains("newer"));
        assert_eq!(newer.channels(), Vec::new());

        let directory = test_directory("channels-failed-save");
        let session = SessionStore::open(&directory);
        let kept = vec![channel("Kick", "drums", false, false)];
        session.set_channels(kept.clone()).expect("saved");
        fs::create_dir(directory.join(format!("{SESSION_FILE}.tmp"))).expect("blocker");
        session
            .set_channels(Vec::new())
            .expect_err("the write cannot happen");
        assert_eq!(session.channels(), kept);
    }

    fn controls(limit_percent: u8, locked_channels: &[usize]) -> SessionEngineer {
        SessionEngineer {
            limit_percent,
            locked_channels: locked_channels.to_vec(),
        }
    }

    #[test]
    fn engineer_controls_are_kept_for_the_phone_beside_its_label_and_survive_a_reload() {
        let directory = test_directory("engineer-reload");
        let session = SessionStore::open(&directory);
        session
            .set_label(Some(PHONE), ADDRESS, "Ana")
            .expect("saved");

        session
            .set_engineer(Some(PHONE), ADDRESS, controls(60, &[1, 2]))
            .expect("saved");

        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({
                format!("client:{PHONE}"): {
                    "label": "Ana",
                    "engineer": { "limitPercent": 60, "lockedChannels": [1, 2] }
                }
            })
        );
        let reloaded = SessionStore::open(&directory);
        assert_eq!(
            reloaded.engineer_for(Some(PHONE), "192.168.1.99:50000"),
            Some(controls(60, &[1, 2])),
            "they follow the phone to any address"
        );
        assert_eq!(
            reloaded.engineer_controls(),
            vec![(format!("client:{PHONE}"), controls(60, &[1, 2]))]
        );
    }

    #[test]
    fn controls_kept_for_a_phone_replace_those_kept_for_its_address() {
        let directory = test_directory("engineer-address");
        let session = SessionStore::open(&directory);
        session
            .set_engineer(None, ADDRESS, controls(40, &[3]))
            .expect("saved");
        session.set_label(None, ADDRESS, "Rack").expect("saved");
        assert_eq!(
            session.engineer_for(Some(PHONE), ADDRESS),
            Some(controls(40, &[3])),
            "the address is the fallback"
        );

        session
            .set_engineer(Some(PHONE), ADDRESS, controls(80, &[]))
            .expect("saved");

        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({
                format!("client:{PHONE}"): { "engineer": { "limitPercent": 80, "lockedChannels": [] } },
                format!("address:{ADDRESS}"): { "label": "Rack" }
            }),
            "the address keeps its label but not the controls"
        );
    }

    #[test]
    fn controls_back_at_the_defaults_are_not_kept() {
        let directory = test_directory("engineer-defaults");
        let session = SessionStore::open(&directory);
        session
            .set_engineer(None, ADDRESS, controls(50, &[1]))
            .expect("saved");

        session
            .set_engineer(None, ADDRESS, SessionEngineer::default())
            .expect("saved");

        assert_eq!(session.engineer_for(None, ADDRESS), None);
        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({}),
            "an entry with nothing left in it goes"
        );
    }

    #[test]
    fn unreadable_engineer_controls_cost_only_themselves() {
        let directory = test_directory("engineer-unreadable");
        fs::write(
            directory.join(SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{"client:{PHONE}":{{"label":"Ana","engineer":{{"limitPercent":900}}}},"address:{ADDRESS}":{{"engineer":"loud"}}}}}}"#
            ),
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.notice(), None, "the file loads as it is");
        assert_eq!(
            session.label_for(Some(PHONE), ADDRESS),
            Some("Ana".to_owned())
        );
        assert_eq!(session.engineer_for(Some(PHONE), ADDRESS), None);
        assert_eq!(session.engineer_controls(), Vec::new());
    }

    #[test]
    fn saving_engineer_controls_is_refused_over_a_newer_session() {
        let directory = test_directory("engineer-newer");
        fs::write(directory.join(SESSION_FILE), r#"{"version":2}"#).expect("write");
        let session = SessionStore::open(&directory);

        let error = session
            .set_engineer(None, ADDRESS, controls(50, &[]))
            .expect_err("refused");

        assert!(error.contains("newer"), "{error}");
        assert_eq!(session.engineer_for(None, ADDRESS), None);
    }

    fn preset(id: &str, name: &str, gains: &[u8], groups: &[(&str, u8)]) -> SessionPreset {
        SessionPreset {
            id: id.to_owned(),
            name: name.to_owned(),
            mix: SessionPresetMix {
                channels: gains
                    .iter()
                    .map(|&gain| SessionPresetChannel {
                        gain,
                        pan: 50,
                        muted: false,
                        solo: false,
                    })
                    .collect(),
                groups: groups
                    .iter()
                    .map(|(id, level)| ((*id).to_owned(), *level))
                    .collect(),
            },
        }
    }

    #[test]
    fn presets_are_kept_in_order_and_survive_a_reload() {
        let directory = test_directory("presets");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Ana").expect("saved");
        let kept = vec![
            preset("p1", "Verse", &[80, 60], &[("kit", 70)]),
            preset("p2", "Chorus", &[100; 20], &[]),
        ];

        session.set_presets(kept.clone()).expect("saved");

        assert_eq!(session.presets(), kept);
        let reloaded = SessionStore::open(&directory);
        assert_eq!(
            reloaded.presets(),
            kept,
            "twenty inputs kept, whatever the device"
        );
        assert_eq!(reloaded.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["presets"][0]["mix"]["groups"],
            serde_json::json!({ "kit": 70 }),
            "group levels by group id"
        );
    }

    #[test]
    fn a_preset_channel_may_leave_out_what_it_does_not_change_and_a_misshapen_one_costs_the_section(
    ) {
        let directory = test_directory("presets-lenient");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{},"presets":[{"id":"p1","name":"Verse","mix":{"channels":[{"muted":true}]}}]}"#,
        )
        .expect("write");
        assert_eq!(
            SessionStore::open(&directory).presets()[0].mix.channels,
            [SessionPresetChannel {
                gain: 100,
                pan: 50,
                muted: true,
                solo: false,
            }]
        );

        let damaged = test_directory("presets-damaged");
        fs::write(
            damaged.join(SESSION_FILE),
            r#"{"version":1,"musicians":{"address:192.168.1.30:50000":{"label":"Ana"}},"presets":[{"name":"no id"}]}"#,
        )
        .expect("write");
        let session = SessionStore::open(&damaged);
        assert_eq!(session.presets(), Vec::new());
        assert_eq!(session.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert!(session.notice().expect("reported").contains("presets"));
    }

    fn follows(leader: &str, name: &str) -> SessionMirror {
        SessionMirror {
            leader: leader.to_owned(),
            name: name.to_owned(),
            leader_address: None,
            follower_address: None,
        }
    }

    #[test]
    fn a_mirror_is_kept_for_the_phone_beside_its_label_and_replaces_the_addresss() {
        let directory = test_directory("mirror");
        let session = SessionStore::open(&directory);
        session
            .set_mirror(
                None,
                ADDRESS,
                Some(follows("address:192.168.1.40:50000", "Rack")),
            )
            .expect("saved");
        session
            .set_label(Some(PHONE), ADDRESS, "Ana")
            .expect("saved");

        // Both by their phones, with the addresses they had beside.
        let bass = SessionMirror {
            leader_address: Some("192.168.1.12:50000".to_owned()),
            follower_address: Some(ADDRESS.to_owned()),
            ..follows(&client_key(OTHER_PHONE), "Bass")
        };
        session
            .set_mirror(Some(PHONE), ADDRESS, Some(bass.clone()))
            .expect("saved");

        assert_eq!(
            read_json(&directory.join(SESSION_FILE))["musicians"],
            serde_json::json!({
                format!("client:{PHONE}"): {
                    "label": "Ana",
                    "mirror": {
                        "leader": format!("client:{OTHER_PHONE}"),
                        "name": "Bass",
                        "leaderAddress": "192.168.1.12:50000",
                        "followerAddress": ADDRESS
                    }
                }
            }),
            "the address's mirror gave way"
        );
        let reloaded = SessionStore::open(&directory);
        assert_eq!(
            reloaded.mirror_for(Some(PHONE), "192.168.1.99:50000"),
            Some(bass.clone())
        );
        assert_eq!(reloaded.mirrors(), vec![(client_key(PHONE), bass)]);
        reloaded
            .set_mirror(Some(PHONE), ADDRESS, None)
            .expect("saved");
        assert_eq!(reloaded.mirrors(), Vec::new());
        assert_eq!(
            reloaded.label_for(Some(PHONE), ADDRESS),
            Some("Ana".to_owned())
        );
    }

    #[test]
    fn an_unreadable_mirror_costs_only_itself_and_a_newer_session_refuses_the_new_sections() {
        let directory = test_directory("mirror-unreadable");
        fs::write(
            directory.join(SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{"client:{PHONE}":{{"label":"Ana","mirror":{{"name":7}}}}}}}}"#
            ),
        )
        .expect("write");
        let session = SessionStore::open(&directory);
        assert_eq!(session.notice(), None, "the file loads as it is");
        assert_eq!(
            session.label_for(Some(PHONE), ADDRESS),
            Some("Ana".to_owned())
        );
        assert_eq!(session.mirrors(), Vec::new());

        let newer = test_directory("presets-newer");
        fs::write(newer.join(SESSION_FILE), r#"{"version":2}"#).expect("write");
        let refused = SessionStore::open(&newer);
        for error in [
            refused
                .set_presets(vec![preset("p1", "Verse", &[80], &[])])
                .expect_err("refused"),
            refused
                .set_mirror(
                    None,
                    ADDRESS,
                    Some(follows("address:192.168.1.40:50000", "Rack")),
                )
                .expect_err("refused"),
        ] {
            assert!(error.contains("newer"), "{error}");
        }
        assert_eq!(refused.presets(), Vec::new());
        assert_eq!(refused.mirrors(), Vec::new());
    }

    fn volt_on_wasapi() -> SessionAudio {
        SessionAudio {
            host: "wasapi".to_owned(),
            device: Some("INPUT 1/2 (Volt 4)".to_owned()),
            buffer_frames: Some(256),
            asio_driver: None,
        }
    }

    #[test]
    fn the_audio_input_survives_a_reload_beside_the_other_sections() {
        let directory = test_directory("audio");
        let session = SessionStore::open(&directory);
        session.set_label(None, ADDRESS, "Ana").expect("saved");

        session.set_audio(volt_on_wasapi()).expect("saved");

        assert_eq!(session.audio(), Some(volt_on_wasapi()));
        let reloaded = SessionStore::open(&directory);
        assert_eq!(reloaded.audio(), Some(volt_on_wasapi()));
        assert_eq!(reloaded.notice(), None);
        assert_eq!(
            read_json(&directory.join(SESSION_FILE)),
            serde_json::json!({
                "version": 1,
                "musicians": { "address:192.168.1.30:50000": { "label": "Ana" } },
                "channels": [],
                "groups": [],
                "presets": [],
                "audio": {
                    "host": "wasapi",
                    "device": "INPUT 1/2 (Volt 4)",
                    "bufferFrames": 256,
                    "asioDriver": null
                }
            }),
            "the same version 1 file: an earlier build reads it and drops the section it does not know"
        );
    }

    #[test]
    fn a_damaged_audio_section_is_set_aside_on_its_own_and_the_rest_is_kept() {
        let directory = test_directory("audio-damaged");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"musicians":{"address:192.168.1.30:50000":{"label":"Ana"}},
                "groups":[{"id":"kit","name":"Drums","channels":[1]}],
                "audio":{"host":"asio","bufferFrames":"lots"}}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(session.audio(), None, "the audio input starts unknown");
        assert_eq!(session.label_for(None, ADDRESS), Some("Ana".to_owned()));
        assert_eq!(session.groups().len(), 1, "the readable sections are kept");
        let aside = files_in(&directory)
            .into_iter()
            .find(|name| name.starts_with(&format!("{SESSION_FILE}.audio-damaged-")))
            .expect("the damaged section is kept for recovery");
        assert_eq!(
            read_json(&directory.join(aside)),
            serde_json::json!({ "host": "asio", "bufferFrames": "lots" })
        );
        assert!(session.notice().expect("reported").contains("audio"));
        assert_eq!(
            read_json(&directory.join(SESSION_FILE)).get("audio"),
            None,
            "the session is saved without it"
        );
    }

    #[test]
    fn an_audio_section_may_leave_out_what_it_does_not_choose() {
        let directory = test_directory("audio-short");
        fs::write(
            directory.join(SESSION_FILE),
            r#"{"version":1,"audio":{"host":"asio"}}"#,
        )
        .expect("write");

        let session = SessionStore::open(&directory);

        assert_eq!(
            session.audio(),
            Some(SessionAudio {
                host: "asio".to_owned(),
                device: None,
                buffer_frames: None,
                asio_driver: None,
            }),
            "read as written: the window treats ASIO without a driver as no ASIO"
        );
        assert_eq!(session.notice(), None);
    }

    #[test]
    fn saving_the_audio_input_is_refused_over_a_newer_session_and_a_failed_save_changes_nothing() {
        let newer_directory = test_directory("audio-newer");
        fs::write(newer_directory.join(SESSION_FILE), r#"{"version":2}"#).expect("write");
        let newer = SessionStore::open(&newer_directory);
        assert!(newer
            .set_audio(volt_on_wasapi())
            .expect_err("refused")
            .contains("newer"));
        assert_eq!(newer.audio(), None);

        let directory = test_directory("audio-failed-save");
        let session = SessionStore::open(&directory);
        session.set_audio(volt_on_wasapi()).expect("saved");
        fs::create_dir(directory.join(format!("{SESSION_FILE}.tmp"))).expect("blocker");
        let other = SessionAudio {
            device: None,
            ..volt_on_wasapi()
        };
        session
            .set_audio(other)
            .expect_err("the write cannot happen");
        assert_eq!(session.audio(), Some(volt_on_wasapi()));
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
