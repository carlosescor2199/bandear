//! Packet redundancy set from the desk: the `set_musician_redundancy` command, and the settings the
//! session keeps handed to every engine the desk starts and taken from a loaded session.
//!
//! Per musician the engineer turns redundancy on (each packet sent again a few blocks later, so a
//! Wi-Fi burst must take both copies to cost audio) or off, the default (see
//! [personal_monitoring::Redundancy]). It doubles that musician's bandwidth and nothing else's. The
//! engine keeps the setting while it runs, whether the musician is here or not; the session keeps
//! it for the next start, under the key the engine kept it under (the phone's client id, else the
//! address). Every phone takes it: an older one drops the copies as duplicates, one that announced
//! `redundancy.recovered` plays a copy whose original was lost and reports it (`recovered` on the
//! row and in its link history).
//!
//! The mix backups do not carry it: they restore mixes, and the session already keeps it.

use std::sync::Mutex;

use personal_monitoring::{
    log_info, log_warn, EngineHandle, MusicianKey, Redundancy, RedundancySetting,
    DEFAULT_REDUNDANCY_DELAY_BLOCKS, MAX_REDUNDANCY_DELAY_BLOCKS, MIN_REDUNDANCY_DELAY_BLOCKS,
};

use super::{musician_key_of, DesktopState};
use crate::dto::{RedundancyDto, RedundancySetDto};
use crate::session::{address_key, client_key, Session, SessionRedundancy, SessionStore};

/// What the command needs from a running engine.
pub(crate) trait RedundancyEngine {
    fn set_redundancy(
        &self,
        key: &MusicianKey,
        redundancy: Redundancy,
    ) -> Result<RedundancySetting, String>;
}

impl RedundancyEngine for EngineHandle {
    fn set_redundancy(
        &self,
        key: &MusicianKey,
        redundancy: Redundancy,
    ) -> Result<RedundancySetting, String> {
        EngineHandle::set_redundancy(self, key, redundancy).map_err(|error| error.to_string())
    }
}

/// The redundancy the window asked for: off (any delay is ignored), or on with `delay_blocks`
/// within [MIN_REDUNDANCY_DELAY_BLOCKS]-[MAX_REDUNDANCY_DELAY_BLOCKS], or
/// [DEFAULT_REDUNDANCY_DELAY_BLOCKS] when it names none.
fn requested_redundancy(on: bool, delay_blocks: Option<u32>) -> Result<Redundancy, String> {
    if !on {
        return Ok(Redundancy::Off);
    }
    Redundancy::on(delay_blocks.unwrap_or(u32::from(DEFAULT_REDUNDANCY_DELAY_BLOCKS)))
}

/// `redundancy` as the session keeps it: nothing when off.
fn session_redundancy(redundancy: Redundancy) -> Option<SessionRedundancy> {
    redundancy
        .delay_blocks()
        .map(|delay_blocks| SessionRedundancy {
            delay_blocks: Some(u32::from(delay_blocks)),
        })
}

/// The redundancy settings `session` keeps, as the engine takes them
/// ([EngineHandle::replace_redundancies]): each under its musician's key. One whose key or delay
/// this build cannot read is skipped and logged, and that musician's audio goes once.
pub(crate) fn redundancies_of(session: &Session) -> Vec<(MusicianKey, Redundancy)> {
    session
        .redundancies()
        .into_iter()
        .filter_map(|(key, kept)| {
            let Some(musician) = musician_key_of(&key) else {
                log_warn!(
                    "desk",
                    "the redundancy kept for `{key}` is skipped: it names no phone or address"
                );
                return None;
            };
            match requested_redundancy(true, kept.delay_blocks) {
                Ok(redundancy) => Some((musician, redundancy)),
                Err(why) => {
                    log_warn!("desk", "the redundancy kept for `{key}` is skipped: {why}");
                    None
                }
            }
        })
        .collect()
}

/// Turns redundancy on (`on`, with the copies `delay_blocks` blocks late, 2 when it names none) or
/// off for the musician `musician_key` names (`client:<id>` or `address:<ip:port>`). Answers what
/// was done ([RedundancySetDto]): `set`, in force from the next block, with what it does to that
/// musician's bandwidth, or `invalid` (a delay out of range, nothing changed). A setting the engine
/// took is then saved in the session for the next start; a save that fails is reported in the
/// answer, not as an error.
///
/// A key this build cannot read, a musician the engine does not list, an engine that is stopping
/// and no engine running are errors, and nothing changes. The change is one step under the engine
/// lock, with the session's inside it.
pub fn set_musician_redundancy(
    state: &DesktopState,
    musician_key: &str,
    on: bool,
    delay_blocks: Option<u32>,
) -> Result<RedundancySetDto, String> {
    let outcome = set_redundancy_with(
        &state.engine,
        &state.session,
        musician_key,
        on,
        delay_blocks,
    )?;
    log_info!(
        "desk",
        "set_musician_redundancy: {musician_key} on={on} {delay_blocks:?} -> {} saved={}",
        outcome.outcome,
        outcome.saved
    );
    Ok(outcome)
}

/// [set_musician_redundancy] over any engine slot and session.
pub(crate) fn set_redundancy_with<E: RedundancyEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    musician_key: &str,
    on: bool,
    delay_blocks: Option<u32>,
) -> Result<RedundancySetDto, String> {
    let key = musician_key_of(musician_key)
        .ok_or_else(|| format!("`{musician_key}` names no phone or address"))?;
    let answer =
        |outcome: &'static str, redundancy: Option<Redundancy>, message: String| RedundancySetDto {
            musician_key: musician_key.to_owned(),
            outcome,
            redundancy: redundancy.map(RedundancyDto::from),
            bandwidth_factor: match redundancy {
                Some(Redundancy::On { .. }) => 2,
                _ => 1,
            },
            saved: false,
            save_error: None,
            message,
            min_delay_blocks: MIN_REDUNDANCY_DELAY_BLOCKS,
            max_delay_blocks: MAX_REDUNDANCY_DELAY_BLOCKS,
            default_delay_blocks: DEFAULT_REDUNDANCY_DELAY_BLOCKS,
        };
    let redundancy = match requested_redundancy(on, delay_blocks) {
        Ok(redundancy) => redundancy,
        Err(why) => return Ok(answer("invalid", None, format!("Not set: {why}."))),
    };
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let set = engine.set_redundancy(&key, redundancy)?;
    let message = match redundancy {
        Redundancy::Off => "Each packet is sent once again: this musician's bandwidth is back to \
                            normal."
            .to_owned(),
        Redundancy::On { delay_blocks } => format!(
            "Each packet is sent twice, the copy {delay_blocks} {} later: this musician's \
             bandwidth doubles.",
            if delay_blocks == 1 { "block" } else { "blocks" }
        ),
    };
    let kept_for = match &set.kept_for {
        MusicianKey::Client(id) => client_key(id),
        MusicianKey::Address(address) => address_key(&address.to_string()),
    };
    // The engine already sends it: a save that fails only means the next start will not.
    let saved = session.set_redundancy(
        &kept_for,
        &set.address.to_string(),
        session_redundancy(redundancy),
    );
    let mut answer = answer("set", Some(redundancy), message);
    answer.saved = saved.is_ok();
    answer.save_error = saved.err();
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::SocketAddr;
    use std::path::PathBuf;

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const ADDRESS: &str = "192.168.1.30:50000";
    const RACK: &str = "192.168.1.31:50000";

    /// An engine with Ana's phone at [ADDRESS] and a rack at [RACK]; it records every setting it
    /// took.
    #[derive(Default)]
    struct StandIn {
        took: Mutex<Vec<(MusicianKey, Redundancy)>>,
    }

    impl RedundancyEngine for StandIn {
        fn set_redundancy(
            &self,
            key: &MusicianKey,
            redundancy: Redundancy,
        ) -> Result<RedundancySetting, String> {
            let at = |address: &str| address.parse::<SocketAddr>().expect("an address");
            let (kept_for, address) = match key {
                MusicianKey::Client(id) if id == PHONE => (key.clone(), at(ADDRESS)),
                MusicianKey::Address(address) if address.to_string() == ADDRESS => {
                    (MusicianKey::Client(PHONE.to_owned()), at(ADDRESS))
                }
                MusicianKey::Address(address) if address.to_string() == RACK => {
                    (key.clone(), at(RACK))
                }
                _ => return Err("the musician is not here".to_owned()),
            };
            self.took
                .lock()
                .expect("took lock")
                .push((kept_for.clone(), redundancy));
            Ok(RedundancySetting {
                kept_for,
                address,
                redundancy,
            })
        }
    }

    fn test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("mixlink-desktop-tests")
            .join(format!("redundancy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn::default()))
    }

    fn took(engine: &Mutex<Option<StandIn>>) -> Vec<(MusicianKey, Redundancy)> {
        engine
            .lock()
            .expect("engine lock")
            .as_ref()
            .expect("running")
            .took
            .lock()
            .expect("took lock")
            .clone()
    }

    #[test]
    fn redundancy_on_reaches_the_engine_says_the_bandwidth_and_is_saved_under_its_key() {
        let session = SessionStore::open(&test_directory("on"));
        let engine = running();

        let set = set_redundancy_with(
            &engine,
            &session,
            &format!("address:{ADDRESS}"),
            true,
            Some(3),
        )
        .expect("set");

        let json = serde_json::to_value(&set).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "musicianKey": format!("address:{ADDRESS}"),
                "outcome": "set",
                "redundancy": { "on": true, "delayBlocks": 3 },
                "bandwidthFactor": 2,
                "saved": true,
                "saveError": null,
                "message": "Each packet is sent twice, the copy 3 blocks later: this musician's bandwidth doubles.",
                "minDelayBlocks": 1,
                "maxDelayBlocks": 4,
                "defaultDelayBlocks": 2
            })
        );
        assert_eq!(
            took(&engine),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                Redundancy::On { delay_blocks: 3 }
            )]
        );
        assert_eq!(
            session.redundancies(),
            vec![(
                client_key(PHONE),
                SessionRedundancy {
                    delay_blocks: Some(3)
                }
            )],
            "kept under the key the engine kept it under"
        );

        let off = set_redundancy_with(&engine, &session, &client_key(PHONE), false, Some(4))
            .expect("set");
        assert_eq!(off.outcome, "set");
        assert_eq!(off.redundancy, Some(RedundancyDto::from(Redundancy::Off)));
        assert_eq!(off.bandwidth_factor, 1);
        assert!(off.message.contains("back to normal"), "{}", off.message);
        assert_eq!(session.redundancies(), Vec::new(), "off is kept as nothing");
    }

    #[test]
    fn on_without_a_delay_takes_the_default_and_a_rack_is_kept_by_its_address() {
        let session = SessionStore::open(&test_directory("default"));
        let engine = running();

        let set =
            set_redundancy_with(&engine, &session, &address_key(RACK), true, None).expect("set");

        assert_eq!(
            set.redundancy,
            Some(RedundancyDto {
                on: true,
                delay_blocks: Some(2)
            })
        );
        assert!(set.message.contains("2 blocks later"), "{}", set.message);
        assert_eq!(session.redundancies()[0].0, address_key(RACK));
    }

    #[test]
    fn a_delay_out_of_range_is_invalid_and_changes_nothing() {
        let session = SessionStore::open(&test_directory("invalid"));
        let engine = running();

        for delay in [0, 5, 64] {
            let set = set_redundancy_with(&engine, &session, &client_key(PHONE), true, Some(delay))
                .expect("answered");
            assert_eq!(set.outcome, "invalid", "{delay}");
            assert!(
                set.message.contains("from 1 to 4 blocks"),
                "{}",
                set.message
            );
            assert_eq!(set.redundancy, None);
            assert_eq!(set.bandwidth_factor, 1);
        }
        assert_eq!(took(&engine), Vec::new());
        assert_eq!(session.redundancies(), Vec::new());
    }

    #[test]
    fn an_unreadable_key_nobody_and_no_engine_are_errors() {
        let session = SessionStore::open(&test_directory("errors"));

        let unreadable =
            set_redundancy_with(&running(), &session, "Ana", true, None).expect_err("no key form");
        assert!(
            unreadable.contains("names no phone or address"),
            "{unreadable}"
        );
        let nobody = set_redundancy_with(&running(), &session, "client:someone", true, None)
            .expect_err("nobody");
        assert!(nobody.contains("not here"), "{nobody}");
        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            set_redundancy_with(&stopped, &session, &client_key(PHONE), true, None)
                .expect_err("no engine"),
            "the engine is not running"
        );
    }

    #[test]
    fn the_session_hands_its_readable_settings_to_an_engine_and_skips_the_rest() {
        let directory = test_directory("hand");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{
                    "client:{PHONE}":{{"redundancy":{{"delayBlocks":3}}}},
                    "address:{RACK}":{{"redundancy":{{}}}},
                    "address:192.168.1.33:50000":{{"redundancy":{{"delayBlocks":9}}}},
                    "address:192.168.1.34:50000":{{"redundancy":"lots"}},
                    "Rack":{{"redundancy":{{"delayBlocks":2}}}}
                }}}}"#
            ),
        )
        .expect("write");
        let session = SessionStore::open(&directory);

        assert_eq!(
            redundancies_of(&session.current()),
            vec![
                (
                    MusicianKey::Address(RACK.parse().unwrap()),
                    Redundancy::On { delay_blocks: 2 }
                ),
                (
                    MusicianKey::Client(PHONE.to_owned()),
                    Redundancy::On { delay_blocks: 3 }
                ),
            ],
            "no delay is the default; one out of range, unreadable or under no key is skipped"
        );
    }
}
