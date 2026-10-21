//! The master limiter on every mix, set from the desk: the `set_musician_limiter` and
//! `set_local_limiter` commands, and the settings the session keeps handed to every engine the
//! desk starts and taken from a loaded session.
//!
//! Each mix (every musician's, and the engineer's local mix) ends with a peak limiter after its sum
//! and before the clamp at the mix's ceiling (see [personal_monitoring::EngineHandle::set_musician_limiter]):
//! off by default, its ceiling -24 to 0 dBFS (0 by default, which limits right at the mix's own
//! ceiling, the lower of the phone's level cap and the engineer's limit) and its release 1 to
//! 1,000 ms (50 by default). It adds no latency. It is engineer-only: no phone sets it and its
//! `mix` does not carry it.
//!
//! The engine keeps a musician's while it runs, whether they are here or not, under the key it
//! kept it under (the phone's client id, else the address); the session keeps it there too, and
//! the local mix's in its `local` section. Named sessions carry both, as they carry the rest of
//! the session.

use std::sync::Mutex;

use personal_monitoring::{log_info, log_warn, EngineHandle, LimiterSettings, MusicianKey};

use super::{musician_key_of, DesktopState};
use crate::dto::LimiterSetDto;
use crate::session::{address_key, client_key, Session, SessionStore};

/// What the commands need from a running engine.
pub(crate) trait LimiterEngine {
    /// Sets the master limiter on the musician's mix; answers the key it is kept under, their row
    /// and the settings kept, held in range.
    fn set_musician_limiter(
        &self,
        key: &MusicianKey,
        settings: LimiterSettings,
    ) -> Result<(MusicianKey, String, LimiterSettings), String>;
    /// Sets the master limiter on the local mix; answers the settings kept, held in range.
    fn set_local_limiter(&self, settings: LimiterSettings) -> Result<LimiterSettings, String>;
    /// Replaces every musician's master limiter the engine keeps
    /// ([EngineHandle::replace_musician_limiters]).
    fn replace_musician_limiters(
        &self,
        kept: &[(MusicianKey, LimiterSettings)],
    ) -> Result<(), String>;
}

impl LimiterEngine for EngineHandle {
    fn set_musician_limiter(
        &self,
        key: &MusicianKey,
        settings: LimiterSettings,
    ) -> Result<(MusicianKey, String, LimiterSettings), String> {
        EngineHandle::set_musician_limiter(self, key, settings)
            .map(|set| (set.kept_for, set.address.to_string(), set.settings))
            .map_err(|error| error.to_string())
    }

    fn set_local_limiter(&self, settings: LimiterSettings) -> Result<LimiterSettings, String> {
        EngineHandle::set_local_limiter(self, settings).map_err(|error| error.to_string())
    }

    fn replace_musician_limiters(
        &self,
        kept: &[(MusicianKey, LimiterSettings)],
    ) -> Result<(), String> {
        EngineHandle::replace_musician_limiters(self, kept)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// `settings` as the session keeps them: nothing while they are the default.
fn kept(settings: LimiterSettings) -> Option<LimiterSettings> {
    (settings != LimiterSettings::master()).then_some(settings)
}

/// The musicians' limiters `session` keeps, as the engine takes them
/// ([EngineHandle::replace_musician_limiters]): each under its musician's key. One whose key this
/// build cannot read is skipped and logged, and that musician's mix has none.
pub(crate) fn limiters_of(session: &Session) -> Vec<(MusicianKey, LimiterSettings)> {
    session
        .limiters()
        .into_iter()
        .filter_map(|(key, settings)| match musician_key_of(&key) {
            Some(musician) => Some((musician, settings)),
            None => {
                log_warn!(
                    "desk",
                    "the limiter kept for `{key}` is skipped: it names no phone or address"
                );
                None
            }
        })
        .collect()
}

/// The local mix's limiter `session` keeps; the default (off) when it keeps none.
pub(crate) fn local_limiter_of(session: &Session) -> LimiterSettings {
    session
        .local
        .as_ref()
        .and_then(|local| local.limiter)
        .unwrap_or_else(LimiterSettings::master)
}

/// Hands `engine` every limiter `session` keeps: each musician's replaces the engine's whole (a
/// musician the session keeps none for has none), and the local mix takes the session's (off when
/// it keeps none). A start does this for the new engine, and a load for the loaded session. Rule
/// for a loaded session saved without a local section: the desk keeps its own local section, its
/// limiter included, so `session` is the session as loaded and the engine and the session always
/// agree on the local mix's limiter. Answers what the engine refused, in words.
pub(crate) fn hand_kept_limiters<E: LimiterEngine>(engine: &E, session: &Session) -> Vec<String> {
    let mut refused = Vec::new();
    if let Err(error) = engine.replace_musician_limiters(&limiters_of(session)) {
        refused.push(format!("the saved limiters were not applied: {error}"));
    }
    if let Err(error) = engine.set_local_limiter(local_limiter_of(session)) {
        refused.push(format!(
            "the saved local mix limiter was not applied: {error}"
        ));
    }
    refused
}

/// What a set says in words.
fn said(settings: &LimiterSettings, whose: &str) -> String {
    if settings.enabled {
        format!(
            "{whose} limiter is on: peaks are held at {} dBFS, released over {} ms.",
            trim(settings.ceiling_db),
            trim(settings.release_ms)
        )
    } else {
        format!("{whose} limiter is off.")
    }
}

/// A setting's number as the window writes it: at most one decimal, no trailing zero.
fn trim(value: f32) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

/// Sets the master limiter on the mix of the musician `musician_key` names (`client:<id>` or
/// `address:<ip:port>`) from their next block, then keeps it in the session for the next start.
/// Answers what was done ([LimiterSetDto]): `set`, with the settings in force (held in range); a
/// save that fails is reported in the answer, not as an error.
///
/// A key this build cannot read, a musician the engine does not list, an engine that is stopping
/// and no engine running are errors, and nothing changes. The change is one step under the engine
/// lock, with the session's inside it.
pub fn set_musician_limiter(
    state: &DesktopState,
    musician_key: &str,
    settings: LimiterSettings,
) -> Result<LimiterSetDto, String> {
    let set = set_musician_limiter_with(&state.engine, &state.session, musician_key, settings)?;
    log_info!(
        "desk",
        "set_musician_limiter: {musician_key} {settings:?} -> {:?} saved={}",
        set.settings,
        set.saved
    );
    Ok(set)
}

/// [set_musician_limiter] over any engine slot and session.
pub(crate) fn set_musician_limiter_with<E: LimiterEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    musician_key: &str,
    settings: LimiterSettings,
) -> Result<LimiterSetDto, String> {
    let key = musician_key_of(musician_key)
        .ok_or_else(|| format!("`{musician_key}` names no phone or address"))?;
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let (kept_for, address, settings) = engine.set_musician_limiter(&key, settings)?;
    let kept_for = match &kept_for {
        MusicianKey::Client(id) => client_key(id),
        MusicianKey::Address(address) => address_key(&address.to_string()),
    };
    // The engine already limits: a save that fails only means the next start will not.
    let saved = session.set_limiter(&kept_for, &address, kept(settings));
    Ok(LimiterSetDto {
        musician_key: Some(musician_key.to_owned()),
        outcome: "set",
        settings,
        saved: saved.is_ok(),
        save_error: saved.err(),
        message: said(&settings, "This musician's"),
    })
}

/// Sets the master limiter on the engineer's local mix from its next block (the local output and
/// the recorder's local mix track both go through it), then keeps it in the session's `local`
/// section. Answers as [set_musician_limiter] does, with no `musicianKey`. An engine that is
/// stopping and no engine running are errors, and nothing changes.
pub fn set_local_limiter(
    state: &DesktopState,
    settings: LimiterSettings,
) -> Result<LimiterSetDto, String> {
    let set = set_local_limiter_with(&state.engine, &state.session, settings)?;
    log_info!(
        "desk",
        "set_local_limiter: {settings:?} -> {:?} saved={}",
        set.settings,
        set.saved
    );
    Ok(set)
}

/// [set_local_limiter] over any engine slot and session.
pub(crate) fn set_local_limiter_with<E: LimiterEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    settings: LimiterSettings,
) -> Result<LimiterSetDto, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let settings = engine.set_local_limiter(settings)?;
    let saved = session.set_local_limiter(kept(settings));
    Ok(LimiterSetDto {
        musician_key: None,
        outcome: "set",
        settings,
        saved: saved.is_ok(),
        save_error: saved.err(),
        message: said(&settings, "The local mix's"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const ADDRESS: &str = "192.168.1.30:50000";
    const RACK: &str = "192.168.1.31:50000";

    /// An engine with Ana's phone at [ADDRESS] and a rack at [RACK] that holds every number in
    /// range as the engine does and records what it took.
    #[derive(Default)]
    struct StandIn {
        took: Mutex<Vec<(MusicianKey, LimiterSettings)>>,
        local: Mutex<Vec<LimiterSettings>>,
        /// Every list of musicians' limiters it took whole.
        replaced: Mutex<Vec<Vec<(MusicianKey, LimiterSettings)>>>,
        stopping: bool,
    }

    impl LimiterEngine for StandIn {
        fn set_musician_limiter(
            &self,
            key: &MusicianKey,
            settings: LimiterSettings,
        ) -> Result<(MusicianKey, String, LimiterSettings), String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let (kept_for, address) = match key {
                MusicianKey::Client(id) if id == PHONE => (key.clone(), ADDRESS),
                MusicianKey::Address(address) if address.to_string() == ADDRESS => {
                    (MusicianKey::Client(PHONE.to_owned()), ADDRESS)
                }
                MusicianKey::Address(address) if address.to_string() == RACK => (key.clone(), RACK),
                _ => return Err("the musician is not here".to_owned()),
            };
            let settings = settings.clamped();
            self.took
                .lock()
                .expect("took lock")
                .push((kept_for.clone(), settings));
            Ok((kept_for, address.to_owned(), settings))
        }

        fn set_local_limiter(&self, settings: LimiterSettings) -> Result<LimiterSettings, String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            let settings = settings.clamped();
            self.local.lock().expect("local lock").push(settings);
            Ok(settings)
        }

        fn replace_musician_limiters(
            &self,
            kept: &[(MusicianKey, LimiterSettings)],
        ) -> Result<(), String> {
            if self.stopping {
                return Err("the engine is stopping".to_owned());
            }
            self.replaced
                .lock()
                .expect("replaced lock")
                .push(kept.to_vec());
            Ok(())
        }
    }

    fn test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-desktop-tests")
            .join(format!("limiter-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn::default()))
    }

    fn on(ceiling_db: f32) -> LimiterSettings {
        LimiterSettings {
            enabled: true,
            ceiling_db,
            release_ms: 120.0,
        }
    }

    #[test]
    fn a_musicians_limiter_reaches_the_engine_is_said_and_saved_under_its_key() {
        let session = SessionStore::open(&test_directory("musician"));
        let engine = running();

        let set = set_musician_limiter_with(
            &engine,
            &session,
            &address_key(ADDRESS),
            LimiterSettings {
                ceiling_db: -40.0,
                ..on(-6.0)
            },
        )
        .expect("set");

        assert_eq!(
            serde_json::to_value(&set).expect("serializes"),
            serde_json::json!({
                "musicianKey": address_key(ADDRESS),
                "outcome": "set",
                "settings": { "enabled": true, "ceilingDb": -24.0, "releaseMs": 120.0 },
                "saved": true,
                "saveError": null,
                "message": "This musician's limiter is on: peaks are held at -24 dBFS, released over 120 ms."
            })
        );
        assert_eq!(
            session.limiters(),
            vec![(client_key(PHONE), on(-24.0))],
            "kept under the key the engine kept it under, held in range"
        );

        let off = set_musician_limiter_with(
            &engine,
            &session,
            &client_key(PHONE),
            LimiterSettings::master(),
        )
        .expect("set");
        assert_eq!(off.message, "This musician's limiter is off.");
        assert_eq!(session.limiters(), Vec::new(), "the default keeps nothing");
    }

    #[test]
    fn the_local_mixs_limiter_reaches_the_engine_and_is_kept_in_the_local_section() {
        let session = SessionStore::open(&test_directory("local"));
        let engine = running();

        let set = set_local_limiter_with(&engine, &session, on(-3.5)).expect("set");
        assert_eq!(set.musician_key, None);
        assert_eq!(set.settings, on(-3.5));
        assert!(set.message.contains("-3.5 dBFS"), "{}", set.message);
        assert_eq!(local_limiter_of(&session.current()), on(-3.5));

        let reopened = SessionStore::open(&test_directory_kept("local"));
        assert_eq!(
            local_limiter_of(&reopened.current()),
            on(-3.5),
            "survives a reload"
        );

        set_local_limiter_with(&engine, &session, LimiterSettings::master()).expect("set");
        assert_eq!(session.local().and_then(|local| local.limiter), None);
    }

    /// The directory [test_directory] made, without clearing it.
    fn test_directory_kept(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("mixlink-desktop-tests")
            .join(format!("limiter-{name}-{}", std::process::id()))
    }

    #[test]
    fn an_unreadable_key_nobody_a_stopping_engine_and_no_engine_are_errors() {
        let session = SessionStore::open(&test_directory("errors"));

        let unreadable = set_musician_limiter_with(&running(), &session, "Ana", on(-1.0))
            .expect_err("no key form");
        assert!(
            unreadable.contains("names no phone or address"),
            "{unreadable}"
        );
        let nobody = set_musician_limiter_with(&running(), &session, "client:someone", on(-1.0))
            .expect_err("nobody");
        assert!(nobody.contains("not here"), "{nobody}");
        let stopping = Mutex::new(Some(StandIn {
            stopping: true,
            ..StandIn::default()
        }));
        assert!(set_local_limiter_with(&stopping, &session, on(-1.0)).is_err());
        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            set_local_limiter_with(&stopped, &session, on(-1.0)).expect_err("no engine"),
            "the engine is not running"
        );
        assert_eq!(session.limiters(), Vec::new());
        assert_eq!(session.local(), None, "nothing kept");
    }

    #[test]
    fn the_session_hands_its_readable_limiters_to_an_engine_and_skips_the_rest() {
        let directory = test_directory("hand");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{
                    "client:{PHONE}":{{"label":"Ana","limiter":{{"enabled":true,"ceilingDb":-6,"releaseMs":80}}}},
                    "address:{RACK}":{{"limiter":"loud"}},
                    "Rack":{{"limiter":{{"enabled":true}}}}
                }},
                "local":{{"limiter":{{"enabled":true,"ceilingDb":-2,"releaseMs":40}}}}}}"#
            ),
        )
        .expect("write");
        let session = SessionStore::open(&directory);
        let current = session.current();

        assert_eq!(
            limiters_of(&current),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                LimiterSettings {
                    enabled: true,
                    ceiling_db: -6.0,
                    release_ms: 80.0,
                }
            )],
            "one that cannot be read or is under no key is skipped"
        );
        assert_eq!(
            current.musicians[&client_key(PHONE)].label.as_deref(),
            Some("Ana"),
            "an unreadable limiter costs only itself"
        );
        assert_eq!(
            local_limiter_of(&current),
            LimiterSettings {
                enabled: true,
                ceiling_db: -2.0,
                release_ms: 40.0,
            }
        );
        assert_eq!(
            local_limiter_of(&Session::default()),
            LimiterSettings::master()
        );
    }

    /// What a start hands the new engine: the saved musicians' limiters whole and the saved local
    /// mix's; a session with none hands an empty list and the default.
    #[test]
    fn an_engine_is_handed_the_saved_musician_and_local_limiters() {
        let session = SessionStore::open(&test_directory("hand-engine"));
        session
            .set_limiter(&client_key(PHONE), ADDRESS, Some(on(-6.0)))
            .expect("kept");
        session.set_local_limiter(Some(on(-2.0))).expect("kept");
        let engine = StandIn::default();

        assert_eq!(
            hand_kept_limiters(&engine, &session.current()),
            Vec::<String>::new()
        );
        assert_eq!(
            *engine.replaced.lock().expect("replaced lock"),
            vec![vec![(MusicianKey::Client(PHONE.to_owned()), on(-6.0))]]
        );
        assert_eq!(*engine.local.lock().expect("local lock"), vec![on(-2.0)]);

        let fresh = StandIn::default();
        assert!(hand_kept_limiters(&fresh, &Session::default()).is_empty());
        assert_eq!(
            *fresh.replaced.lock().expect("replaced lock"),
            vec![Vec::new()]
        );
        assert_eq!(
            *fresh.local.lock().expect("local lock"),
            vec![LimiterSettings::master()],
            "no local section: the local mix's limiter goes back to off"
        );

        let stopping = StandIn {
            stopping: true,
            ..StandIn::default()
        };
        assert_eq!(
            hand_kept_limiters(&stopping, &session.current()),
            vec![
                "the saved limiters were not applied: the engine is stopping".to_owned(),
                "the saved local mix limiter was not applied: the engine is stopping".to_owned(),
            ]
        );
    }
}
