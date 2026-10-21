//! The phone's playback buffer set from the desk: the `set_musician_buffer` command, and the buffers
//! the session keeps handed to every engine the desk starts and taken from a loaded session.
//!
//! Per musician the engineer picks Auto (the phone adapts its own cushion, the default) or a fixed
//! length in milliseconds the phone keeps without adapting (see [personal_monitoring::PlaybackBuffer]).
//! The engine keeps the choice while it runs and sends it to the phone whenever it registers; the
//! session keeps it for the next start, under the key the engine kept it under (the phone's client
//! id, else the address). Only a phone that announced `playback.buffer` takes one: any other row
//! answers `unsupported`, which the window shows as "needs a newer app". The cushion the phone
//! plays comes back in its reports (`linkLatest.cushionMs`).
//!
//! The mix backups do not carry the buffer: they restore mixes, and the buffer is a setting the
//! session already keeps, so a restore never changes it.

use std::sync::Mutex;

use personal_monitoring::{
    log_info, log_warn, BufferOutcome, BufferSetting, EngineHandle, MusicianKey, PlaybackBuffer,
    MAX_FIXED_BUFFER_MS, MIN_FIXED_BUFFER_MS,
};

use super::{musician_key_of, DesktopState};
use crate::dto::{BufferDto, BufferSetDto};
use crate::session::{address_key, client_key, Session, SessionBuffer, SessionStore};

/// What the command needs from a running engine.
pub(crate) trait BufferEngine {
    fn set_playback_buffer(
        &self,
        key: &MusicianKey,
        buffer: PlaybackBuffer,
    ) -> Result<BufferSetting, String>;
}

impl BufferEngine for EngineHandle {
    fn set_playback_buffer(
        &self,
        key: &MusicianKey,
        buffer: PlaybackBuffer,
    ) -> Result<BufferSetting, String> {
        EngineHandle::set_playback_buffer(self, key, buffer).map_err(|error| error.to_string())
    }
}

/// The buffer the window asked for: `mode` `auto` (any `ms` is ignored) or `fixed` with `ms`
/// within [MIN_FIXED_BUFFER_MS]-[MAX_FIXED_BUFFER_MS]; anything else is refused, saying why.
fn requested_buffer(mode: &str, ms: Option<u32>) -> Result<PlaybackBuffer, String> {
    match mode {
        "auto" => Ok(PlaybackBuffer::Auto),
        "fixed" => match ms {
            Some(ms) => PlaybackBuffer::fixed(ms),
            None => Err("a fixed buffer needs its length in ms".to_owned()),
        },
        other => Err(format!(
            "`{other}` is no buffer mode: it is `auto` or `fixed`"
        )),
    }
}

/// `buffer` as the session keeps it: nothing for Auto.
fn session_buffer(buffer: PlaybackBuffer) -> Option<SessionBuffer> {
    buffer.fixed_ms().map(|ms| SessionBuffer {
        mode: buffer.mode().to_owned(),
        ms: Some(u32::from(ms)),
    })
}

/// The buffer a kept one stands for, or why it cannot be used (a mode this build does not know, or
/// a length out of range in a hand-edited session).
fn engine_buffer(kept: &SessionBuffer) -> Result<PlaybackBuffer, String> {
    requested_buffer(&kept.mode, kept.ms)
}

/// The buffers `session` keeps, as the engine takes them ([EngineHandle::replace_playback_buffers]):
/// each under its musician's key. One whose key or buffer this build cannot read is skipped and
/// logged, and that musician stays on Auto.
pub(crate) fn buffers_of(session: &Session) -> Vec<(MusicianKey, PlaybackBuffer)> {
    session
        .buffers()
        .into_iter()
        .filter_map(|(key, kept)| {
            let Some(musician) = musician_key_of(&key) else {
                log_warn!(
                    "desk",
                    "the buffer kept for `{key}` is skipped: it names no phone or address"
                );
                return None;
            };
            match engine_buffer(&kept) {
                Ok(buffer) => Some((musician, buffer)),
                Err(why) => {
                    log_warn!("desk", "the buffer kept for `{key}` is skipped: {why}");
                    None
                }
            }
        })
        .collect()
}

/// A buffer in words, for the window.
fn describe(buffer: PlaybackBuffer) -> String {
    match buffer {
        PlaybackBuffer::Auto => "Auto".to_owned(),
        PlaybackBuffer::Fixed { ms } => format!("a fixed {ms} ms"),
    }
}

/// Sets the playback buffer of the musician `musician_key` names (`client:<id>` or
/// `address:<ip:port>`): `mode` `auto`, or `fixed` with `ms`. Answers what was done
/// ([BufferSetDto]): `sent` to a connected phone, `notConnected` (kept, sent when it is back),
/// `unsupported` (needs a newer app, nothing changed) or `invalid` (no such buffer, nothing
/// changed). A buffer the engine took is then saved in the session for the next start; a save that
/// fails is reported in the answer, not as an error.
///
/// A key this build cannot read, a musician the engine does not list, an engine that is stopping
/// and no engine running are errors, and nothing changes. The change is one step under the engine
/// lock, with the session's inside it.
pub fn set_musician_buffer(
    state: &DesktopState,
    musician_key: &str,
    mode: &str,
    ms: Option<u32>,
) -> Result<BufferSetDto, String> {
    let outcome = set_buffer_with(&state.engine, &state.session, musician_key, mode, ms)?;
    log_info!(
        "desk",
        "set_musician_buffer: {musician_key} {mode} {ms:?} -> {} saved={}",
        outcome.outcome,
        outcome.saved
    );
    Ok(outcome)
}

/// [set_musician_buffer] over any engine slot and session.
pub(crate) fn set_buffer_with<E: BufferEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    musician_key: &str,
    mode: &str,
    ms: Option<u32>,
) -> Result<BufferSetDto, String> {
    let key = musician_key_of(musician_key)
        .ok_or_else(|| format!("`{musician_key}` names no phone or address"))?;
    let answer =
        |outcome: &'static str, buffer: Option<PlaybackBuffer>, message: String| BufferSetDto {
            musician_key: musician_key.to_owned(),
            outcome,
            buffer: buffer.map(BufferDto::from),
            saved: false,
            save_error: None,
            message,
            min_ms: MIN_FIXED_BUFFER_MS,
            max_ms: MAX_FIXED_BUFFER_MS,
        };
    let buffer = match requested_buffer(mode, ms) {
        Ok(buffer) => buffer,
        Err(why) => return Ok(answer("invalid", None, format!("Not set: {why}."))),
    };
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let set = engine.set_playback_buffer(&key, buffer)?;
    let (outcome, message) = match set.outcome {
        BufferOutcome::Unsupported => {
            return Ok(answer(
                "unsupported",
                None,
                "This phone needs a newer app to take its buffer from the desk.".to_owned(),
            ))
        }
        BufferOutcome::Sent => ("sent", format!("Sent to the phone: {}.", describe(buffer))),
        BufferOutcome::NotConnected => (
            "notConnected",
            format!(
                "Kept: the phone gets {} when it is connected again.",
                describe(buffer)
            ),
        ),
    };
    let kept_for = match &set.kept_for {
        MusicianKey::Client(id) => client_key(id),
        MusicianKey::Address(address) => address_key(&address.to_string()),
    };
    // The phone already has it: a save that fails only means the next start will not.
    let saved = session.set_buffer(&kept_for, &set.address.to_string(), session_buffer(buffer));
    let mut answer = answer(outcome, Some(buffer), message);
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
    const OLDER: &str = "192.168.1.31:50000";
    const AWAY: &str = "7d2e4c1a-9b3f-4e6d-8a5c-1f0e2d3c4b5a";

    /// An engine with Ana's phone (connected, takes a buffer), a phone away that takes one, and
    /// an older phone at [OLDER]; it records every buffer it took.
    #[derive(Default)]
    struct StandIn {
        took: Mutex<Vec<(MusicianKey, PlaybackBuffer)>>,
    }

    impl BufferEngine for StandIn {
        fn set_playback_buffer(
            &self,
            key: &MusicianKey,
            buffer: PlaybackBuffer,
        ) -> Result<BufferSetting, String> {
            let at = |address: &str| address.parse::<SocketAddr>().expect("an address");
            let (kept_for, address, outcome) = match key {
                MusicianKey::Client(id) if id == PHONE => {
                    (key.clone(), at(ADDRESS), BufferOutcome::Sent)
                }
                MusicianKey::Address(address) if address.to_string() == ADDRESS => (
                    MusicianKey::Client(PHONE.to_owned()),
                    at(ADDRESS),
                    BufferOutcome::Sent,
                ),
                MusicianKey::Client(id) if id == AWAY => (
                    key.clone(),
                    at("192.168.1.32:50000"),
                    BufferOutcome::NotConnected,
                ),
                MusicianKey::Address(address) if address.to_string() == OLDER => {
                    (key.clone(), at(OLDER), BufferOutcome::Unsupported)
                }
                _ => return Err("the musician is not here".to_owned()),
            };
            if outcome != BufferOutcome::Unsupported {
                self.took
                    .lock()
                    .expect("took lock")
                    .push((kept_for.clone(), buffer));
            }
            Ok(BufferSetting {
                kept_for,
                address,
                outcome,
            })
        }
    }

    fn test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bandear-desktop-tests")
            .join(format!("buffer-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn::default()))
    }

    fn took(engine: &Mutex<Option<StandIn>>) -> Vec<(MusicianKey, PlaybackBuffer)> {
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
    fn a_fixed_buffer_reaches_the_phone_and_is_saved_under_its_key() {
        let session = SessionStore::open(&test_directory("fixed"));
        let engine = running();

        let set = set_buffer_with(
            &engine,
            &session,
            &format!("address:{ADDRESS}"),
            "fixed",
            Some(40),
        )
        .expect("set");

        let json = serde_json::to_value(&set).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "musicianKey": format!("address:{ADDRESS}"),
                "outcome": "sent",
                "buffer": { "mode": "fixed", "ms": 40 },
                "saved": true,
                "saveError": null,
                "message": "Sent to the phone: a fixed 40 ms.",
                "minMs": 5,
                "maxMs": 500
            })
        );
        assert_eq!(
            took(&engine),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                PlaybackBuffer::Fixed { ms: 40 }
            )]
        );
        assert_eq!(
            session.buffers(),
            vec![(
                client_key(PHONE),
                SessionBuffer {
                    mode: "fixed".to_owned(),
                    ms: Some(40)
                }
            )],
            "kept under the key the engine kept it under"
        );

        let auto =
            set_buffer_with(&engine, &session, &client_key(PHONE), "auto", Some(99)).expect("set");
        assert_eq!(auto.outcome, "sent");
        assert_eq!(auto.buffer, Some(BufferDto::from(PlaybackBuffer::Auto)));
        assert_eq!(auto.message, "Sent to the phone: Auto.");
        assert_eq!(session.buffers(), Vec::new(), "Auto is kept as nothing");
    }

    #[test]
    fn a_phone_away_keeps_it_for_when_it_is_back() {
        let session = SessionStore::open(&test_directory("away"));

        let set = set_buffer_with(&running(), &session, &client_key(AWAY), "fixed", Some(80))
            .expect("set");

        assert_eq!(set.outcome, "notConnected");
        assert!(set.saved);
        assert_eq!(
            set.message,
            "Kept: the phone gets a fixed 80 ms when it is connected again."
        );
        assert_eq!(session.buffers().len(), 1);
    }

    #[test]
    fn an_older_phone_needs_a_newer_app_and_nothing_is_kept() {
        let session = SessionStore::open(&test_directory("older"));
        let engine = running();

        let set = set_buffer_with(
            &engine,
            &session,
            &format!("address:{OLDER}"),
            "fixed",
            Some(40),
        )
        .expect("answered");

        assert_eq!(set.outcome, "unsupported");
        assert_eq!(set.buffer, None);
        assert!(!set.saved);
        assert!(set.message.contains("needs a newer app"), "{}", set.message);
        assert_eq!(took(&engine), Vec::new());
        assert_eq!(session.buffers(), Vec::new());
    }

    #[test]
    fn a_length_out_of_range_or_missing_and_an_unknown_mode_are_invalid_and_change_nothing() {
        let session = SessionStore::open(&test_directory("invalid"));
        let engine = running();

        for (mode, ms, why) in [
            ("fixed", Some(4), "between 5 and 500 ms"),
            ("fixed", Some(501), "between 5 and 500 ms"),
            ("fixed", None, "needs its length"),
            ("elastic", Some(40), "no buffer mode"),
        ] {
            let set =
                set_buffer_with(&engine, &session, &client_key(PHONE), mode, ms).expect("answered");
            assert_eq!(set.outcome, "invalid", "{mode} {ms:?}");
            assert!(set.message.contains(why), "{}", set.message);
            assert_eq!(set.buffer, None);
        }
        assert_eq!(took(&engine), Vec::new());
        assert_eq!(session.buffers(), Vec::new());
    }

    #[test]
    fn an_unreadable_key_nobody_and_no_engine_are_errors() {
        let session = SessionStore::open(&test_directory("errors"));

        let unreadable =
            set_buffer_with(&running(), &session, "Ana", "auto", None).expect_err("no key form");
        assert!(
            unreadable.contains("names no phone or address"),
            "{unreadable}"
        );
        let nobody = set_buffer_with(&running(), &session, "client:someone", "auto", None)
            .expect_err("nobody");
        assert!(nobody.contains("not here"), "{nobody}");
        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            set_buffer_with(&stopped, &session, &client_key(PHONE), "auto", None)
                .expect_err("no engine"),
            "the engine is not running"
        );
    }

    #[test]
    fn the_session_hands_its_readable_buffers_to_an_engine_and_skips_the_rest() {
        let directory = test_directory("hand");
        std::fs::write(
            directory.join(crate::session::SESSION_FILE),
            format!(
                r#"{{"version":1,"musicians":{{
                    "client:{PHONE}":{{"buffer":{{"mode":"fixed","ms":40}}}},
                    "address:{OLDER}":{{"buffer":{{"mode":"fixed","ms":9000}}}},
                    "address:192.168.1.33:50000":{{"buffer":{{"mode":"elastic"}}}},
                    "Rack":{{"buffer":{{"mode":"fixed","ms":40}}}}
                }}}}"#
            ),
        )
        .expect("write");
        let session = SessionStore::open(&directory);

        assert_eq!(
            buffers_of(&session.current()),
            vec![(
                MusicianKey::Client(PHONE.to_owned()),
                PlaybackBuffer::Fixed { ms: 40 }
            )]
        );
    }
}
