//! The shared reverb and delay, set from the desk: the `set_effect` command for their settings,
//! `set_musician_fx_return` and `set_local_fx_return` for each mix's return levels, and what the
//! session keeps handed to every engine the desk starts and taken from a loaded session.
//!
//! The engine has one reverb and one delay on two buses every input sends to (the sends are kept
//! with each channel's strip; see [super::processing::set_channel_send]), and every mix (each
//! musician's and the local mix) returns each at its own level, 0-100%, scaled by its master
//! volume (see [personal_monitoring::EngineHandle::set_reverb]). The returns are engineer-only: no
//! phone sets them and its `mix` does not carry them. The effects add no latency to the dry mix.
//!
//! The session keeps the effects' settings in its `effects` section, each musician's returns on
//! their entry and the local mix's in the `local` section (absent while they are the defaults).
//! Named sessions carry all three, as they carry the rest of the session.

use std::sync::Mutex;

use personal_monitoring::{
    log_info, DelaySettings, Effect, EffectLevels, EffectSettings, EngineHandle, MusicianKey,
    ReverbSettings,
};
use serde_json::Value;

use super::{musician_key_of, DesktopState};
use crate::dto::{EffectSetDto, FxReturnSetDto};
use crate::session::{address_key, client_key, Session, SessionStore};

/// What the commands need from a running engine.
pub(crate) trait EffectsEngine {
    /// Sets the reverb's settings; answers both effects' settings now in force.
    fn set_reverb(&self, settings: ReverbSettings) -> Result<EffectSettings, String>;
    /// Sets the delay's settings; answers both effects' settings now in force.
    fn set_delay(&self, settings: DelaySettings) -> Result<EffectSettings, String>;
    /// Replaces both effects' settings ([EngineHandle::replace_effects]).
    fn replace_effects(&self, settings: EffectSettings) -> Result<EffectSettings, String>;
    /// Sets one return in a musician's mix; answers the key it is kept under, their row and every
    /// return now in their mix.
    fn set_musician_fx_return(
        &self,
        key: &MusicianKey,
        effect: Effect,
        level: u8,
    ) -> Result<(MusicianKey, String, EffectLevels), String>;
    /// Replaces every musician's returns the engine keeps.
    fn replace_musician_fx_returns(
        &self,
        kept: &[(MusicianKey, EffectLevels)],
    ) -> Result<(), String>;
    /// Sets one return in the local mix; answers every return now in it.
    fn set_local_fx_return(&self, effect: Effect, level: u8) -> Result<EffectLevels, String>;
    /// Replaces the local mix's returns.
    fn replace_local_fx_returns(&self, levels: EffectLevels) -> Result<EffectLevels, String>;
}

impl EffectsEngine for EngineHandle {
    fn set_reverb(&self, settings: ReverbSettings) -> Result<EffectSettings, String> {
        EngineHandle::set_reverb(self, settings).map_err(|error| error.to_string())?;
        Ok(self.effects())
    }

    fn set_delay(&self, settings: DelaySettings) -> Result<EffectSettings, String> {
        EngineHandle::set_delay(self, settings).map_err(|error| error.to_string())?;
        Ok(self.effects())
    }

    fn replace_effects(&self, settings: EffectSettings) -> Result<EffectSettings, String> {
        EngineHandle::replace_effects(self, settings).map_err(|error| error.to_string())
    }

    fn set_musician_fx_return(
        &self,
        key: &MusicianKey,
        effect: Effect,
        level: u8,
    ) -> Result<(MusicianKey, String, EffectLevels), String> {
        EngineHandle::set_musician_fx_return(self, key, effect, level)
            .map(|set| (set.kept_for, set.address.to_string(), set.returns))
            .map_err(|error| error.to_string())
    }

    fn replace_musician_fx_returns(
        &self,
        kept: &[(MusicianKey, EffectLevels)],
    ) -> Result<(), String> {
        EngineHandle::replace_musician_fx_returns(self, kept)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn set_local_fx_return(&self, effect: Effect, level: u8) -> Result<EffectLevels, String> {
        EngineHandle::set_local_fx_return(self, effect, level).map_err(|error| error.to_string())
    }

    fn replace_local_fx_returns(&self, levels: EffectLevels) -> Result<EffectLevels, String> {
        EngineHandle::replace_local_fx_returns(self, levels).map_err(|error| error.to_string())
    }
}

/// `levels` as the session keeps them: nothing while nothing is returned.
fn kept(levels: EffectLevels) -> Option<EffectLevels> {
    (!levels.is_off()).then_some(levels)
}

/// The effects' settings `session` keeps; the defaults when it keeps none.
pub(crate) fn effects_of(session: &Session) -> EffectSettings {
    session.effects.unwrap_or_default()
}

/// The musicians' returns `session` keeps, as the engine takes them; one whose key this build
/// cannot read is skipped.
pub(crate) fn fx_returns_of(session: &Session) -> Vec<(MusicianKey, EffectLevels)> {
    session
        .fx_returns()
        .into_iter()
        .filter_map(|(key, levels)| Some((musician_key_of(&key)?, levels)))
        .collect()
}

/// The local mix's returns `session` keeps; nothing returned when it keeps none.
pub(crate) fn local_fx_returns_of(session: &Session) -> EffectLevels {
    session
        .local
        .as_ref()
        .and_then(|local| local.fx_returns)
        .unwrap_or_default()
}

/// Hands `engine` the effects `session` keeps: both effects' settings, every musician's returns
/// (replacing the engine's whole) and the local mix's. A start does this for the new engine and a
/// load for the loaded session; as for the limiters (see [super::limiter::hand_kept_limiters]), a
/// session saved without a local section keeps the desk's own, so the local mix's returns are
/// always the ones the session keeps. Answers what the engine refused, in words.
pub(crate) fn hand_kept_effects<E: EffectsEngine>(engine: &E, session: &Session) -> Vec<String> {
    let mut refused = Vec::new();
    if let Err(error) = engine.replace_effects(effects_of(session)) {
        refused.push(format!("the saved effects were not applied: {error}"));
    }
    if let Err(error) = engine.replace_musician_fx_returns(&fx_returns_of(session)) {
        refused.push(format!(
            "the saved effect returns were not applied: {error}"
        ));
    }
    if let Err(error) = engine.replace_local_fx_returns(local_fx_returns_of(session)) {
        refused.push(format!(
            "the saved local mix effect returns were not applied: {error}"
        ));
    }
    refused
}

/// Sets the shared `effect`'s settings (`reverb`: `{ sizePercent, dampingPercent, preDelayMs,
/// widthPercent }`; `delay`: `{ timeMs, tempoBpm, note, feedbackPercent, highCutHz, pingPong }`;
/// a field left out takes its default, a number out of range is held in range) from the engine's
/// next block, then keeps both in the session. Answers what was done ([EffectSetDto]): `set`, or
/// `invalid` for settings this build cannot read. An engine that is stopping, or none running, is
/// an error and nothing changes.
pub fn set_effect(
    state: &DesktopState,
    effect: Effect,
    settings: Value,
) -> Result<EffectSetDto, String> {
    let set = set_effect_with(&state.engine, &state.session, effect, settings)?;
    log_info!(
        "desk",
        "set_effect: {} -> {} saved={}",
        effect.name(),
        set.outcome,
        set.saved
    );
    Ok(set)
}

/// [set_effect] over any engine slot and session.
pub(crate) fn set_effect_with<E: EffectsEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    effect: Effect,
    settings: Value,
) -> Result<EffectSetDto, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let invalid = |error: serde_json::Error| EffectSetDto {
        outcome: "invalid",
        effect,
        effects: effects_of(&session.current()),
        saved: false,
        save_error: None,
        message: format!(
            "The {} settings cannot be read ({error}); nothing changed.",
            effect.name()
        ),
    };
    let effects = match effect {
        Effect::Reverb => match serde_json::from_value::<ReverbSettings>(settings) {
            Ok(settings) => engine.set_reverb(settings)?,
            Err(error) => return Ok(invalid(error)),
        },
        Effect::Delay => match serde_json::from_value::<DelaySettings>(settings) {
            Ok(settings) => engine.set_delay(settings)?,
            Err(error) => return Ok(invalid(error)),
        },
    };
    // The engine already plays them: a save that fails only means the next start will not.
    let saved = session.set_effects((effects != EffectSettings::default()).then_some(effects));
    let message = match effect {
        Effect::Reverb => format!(
            "The reverb is set: size {}%, damping {}%, pre-delay {} ms, width {}%.",
            effects.reverb.size_percent.round(),
            effects.reverb.damping_percent.round(),
            effects.reverb.pre_delay_ms.round(),
            effects.reverb.width_percent.round()
        ),
        Effect::Delay => format!(
            "The delay is set: {} ms, feedback {}%{}.",
            effects.delay.time_ms_in_force().round(),
            effects.delay.feedback_percent.round(),
            if effects.delay.ping_pong {
                ", ping-pong"
            } else {
                ""
            }
        ),
    };
    Ok(EffectSetDto {
        outcome: "set",
        effect,
        effects,
        saved: saved.is_ok(),
        save_error: saved.err(),
        message,
    })
}

/// What a return change says in words.
fn said(effect: Effect, level: u8, whose: &str) -> String {
    if level == 0 {
        format!("{whose} mix returns no {}.", effect.name())
    } else {
        format!("{whose} mix returns the {} at {level}%.", effect.name())
    }
}

/// Sets the return of `effect` in the mix of the musician `musician_key` names (`client:<id>` or
/// `address:<ip:port>`) to `level` percent (held to 100) from their next block, then keeps their
/// returns in the session. Answers what was done ([FxReturnSetDto]); a save that fails is reported
/// in the answer. A key this build cannot read, a musician the engine does not list, an engine that
/// is stopping and no engine running are errors, and nothing changes.
pub fn set_musician_fx_return(
    state: &DesktopState,
    musician_key: &str,
    effect: Effect,
    level: u8,
) -> Result<FxReturnSetDto, String> {
    let set =
        set_musician_fx_return_with(&state.engine, &state.session, musician_key, effect, level)?;
    log_info!(
        "desk",
        "set_musician_fx_return: {musician_key} {} {level} -> {:?} saved={}",
        effect.name(),
        set.returns,
        set.saved
    );
    Ok(set)
}

/// [set_musician_fx_return] over any engine slot and session.
pub(crate) fn set_musician_fx_return_with<E: EffectsEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    musician_key: &str,
    effect: Effect,
    level: u8,
) -> Result<FxReturnSetDto, String> {
    let key = musician_key_of(musician_key)
        .ok_or_else(|| format!("`{musician_key}` names no phone or address"))?;
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let (kept_for, address, returns) = engine.set_musician_fx_return(&key, effect, level)?;
    let kept_for = match &kept_for {
        MusicianKey::Client(id) => client_key(id),
        MusicianKey::Address(address) => address_key(&address.to_string()),
    };
    let saved = session.set_fx_returns(&kept_for, &address, kept(returns));
    Ok(FxReturnSetDto {
        musician_key: Some(musician_key.to_owned()),
        outcome: "set",
        effect,
        returns,
        saved: saved.is_ok(),
        save_error: saved.err(),
        message: said(effect, returns.get(effect), "This musician's"),
    })
}

/// Sets the return of `effect` in the engineer's local mix (the local output and the recorder's
/// local mix track both return it), then keeps the local mix's returns in the session's `local`
/// section. Answers as [set_musician_fx_return] does, with no `musicianKey`.
pub fn set_local_fx_return(
    state: &DesktopState,
    effect: Effect,
    level: u8,
) -> Result<FxReturnSetDto, String> {
    let set = set_local_fx_return_with(&state.engine, &state.session, effect, level)?;
    log_info!(
        "desk",
        "set_local_fx_return: {} {level} -> {:?} saved={}",
        effect.name(),
        set.returns,
        set.saved
    );
    Ok(set)
}

/// [set_local_fx_return] over any engine slot and session.
pub(crate) fn set_local_fx_return_with<E: EffectsEngine>(
    engine: &Mutex<Option<E>>,
    session: &SessionStore,
    effect: Effect,
    level: u8,
) -> Result<FxReturnSetDto, String> {
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let returns = engine.set_local_fx_return(effect, level)?;
    let saved = session.set_local_fx_returns(kept(returns));
    Ok(FxReturnSetDto {
        musician_key: None,
        outcome: "set",
        effect,
        returns,
        saved: saved.is_ok(),
        save_error: saved.err(),
        message: said(effect, returns.get(effect), "The local"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";
    const ADDRESS: &str = "192.168.1.30:50000";

    /// An engine with Ana's phone at [ADDRESS] that holds every number in range as the engine
    /// does and records what it took.
    #[derive(Default)]
    struct StandIn {
        effects: Mutex<EffectSettings>,
        returns: Mutex<EffectLevels>,
        replaced: Mutex<Vec<Vec<(MusicianKey, EffectLevels)>>>,
        local: Mutex<EffectLevels>,
        stopping: bool,
    }

    impl StandIn {
        fn check(&self) -> Result<(), String> {
            if self.stopping {
                Err("the engine is stopping".to_owned())
            } else {
                Ok(())
            }
        }
    }

    impl EffectsEngine for StandIn {
        fn set_reverb(&self, settings: ReverbSettings) -> Result<EffectSettings, String> {
            self.check()?;
            let mut effects = self.effects.lock().expect("effects");
            effects.reverb = settings.clamped();
            Ok(*effects)
        }

        fn set_delay(&self, settings: DelaySettings) -> Result<EffectSettings, String> {
            self.check()?;
            let mut effects = self.effects.lock().expect("effects");
            effects.delay = settings.clamped();
            Ok(*effects)
        }

        fn replace_effects(&self, settings: EffectSettings) -> Result<EffectSettings, String> {
            self.check()?;
            *self.effects.lock().expect("effects") = settings.clamped();
            Ok(settings.clamped())
        }

        fn set_musician_fx_return(
            &self,
            key: &MusicianKey,
            effect: Effect,
            level: u8,
        ) -> Result<(MusicianKey, String, EffectLevels), String> {
            self.check()?;
            let here = match key {
                MusicianKey::Client(id) => id == PHONE,
                MusicianKey::Address(address) => address.to_string() == ADDRESS,
            };
            if !here {
                return Err("the musician is not here".to_owned());
            }
            let mut returns = self.returns.lock().expect("returns");
            *returns = returns.with(effect, level);
            Ok((
                MusicianKey::Client(PHONE.to_owned()),
                ADDRESS.to_owned(),
                *returns,
            ))
        }

        fn replace_musician_fx_returns(
            &self,
            kept: &[(MusicianKey, EffectLevels)],
        ) -> Result<(), String> {
            self.check()?;
            self.replaced.lock().expect("replaced").push(kept.to_vec());
            Ok(())
        }

        fn set_local_fx_return(&self, effect: Effect, level: u8) -> Result<EffectLevels, String> {
            self.check()?;
            let mut local = self.local.lock().expect("local");
            *local = local.with(effect, level);
            Ok(*local)
        }

        fn replace_local_fx_returns(&self, levels: EffectLevels) -> Result<EffectLevels, String> {
            self.check()?;
            *self.local.lock().expect("local") = levels;
            Ok(levels)
        }
    }

    fn test_directory(name: &str) -> PathBuf {
        let directory = std::env::temp_dir()
            .join("bandear-desktop-tests")
            .join(format!("effects-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("test directory");
        directory
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn::default()))
    }

    #[test]
    fn an_effect_is_set_held_in_range_said_and_kept_in_the_session() {
        let session = SessionStore::open(&test_directory("set"));
        let engine = running();

        let set = set_effect_with(
            &engine,
            &session,
            Effect::Reverb,
            serde_json::json!({ "sizePercent": 250, "preDelayMs": 30 }),
        )
        .expect("set");
        assert_eq!(set.outcome, "set");
        assert_eq!(set.effects.reverb.size_percent, 100.0, "held in range");
        assert_eq!(
            set.message,
            "The reverb is set: size 100%, damping 40%, pre-delay 30 ms, width 100%."
        );
        assert!(set.saved);
        assert_eq!(
            session.effects().map(|kept| kept.reverb),
            Some(set.effects.reverb)
        );

        let delay = set_effect_with(
            &engine,
            &session,
            Effect::Delay,
            serde_json::json!({ "tempoBpm": 120, "note": "dottedEighth", "pingPong": true }),
        )
        .expect("set");
        assert_eq!(
            delay.message,
            "The delay is set: 375 ms, feedback 35%, ping-pong."
        );
        let json = serde_json::to_value(&delay).expect("serializes");
        assert_eq!(json["effect"], "delay");
        assert_eq!(json["effects"]["delay"]["note"], "dottedEighth");
        assert_eq!(json["saveError"], Value::Null);
        let reopened = SessionStore::open(&test_directory_kept("set"));
        assert_eq!(
            effects_of(&reopened.current()),
            delay.effects,
            "survives a reload"
        );

        let unreadable = set_effect_with(
            &engine,
            &session,
            Effect::Delay,
            serde_json::json!({ "note": "whole bar" }),
        )
        .expect("answered");
        assert_eq!(unreadable.outcome, "invalid");
        assert_eq!(unreadable.effects, delay.effects, "nothing changed");

        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            set_effect_with(&stopped, &session, Effect::Reverb, serde_json::json!({}))
                .expect_err("no engine"),
            "the engine is not running"
        );
    }

    /// The directory [test_directory] made, without clearing it.
    fn test_directory_kept(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join("bandear-desktop-tests")
            .join(format!("effects-{name}-{}", std::process::id()))
    }

    #[test]
    fn a_return_reaches_the_engine_and_is_kept_under_the_musicians_key_or_the_local_section() {
        let session = SessionStore::open(&test_directory("returns"));
        let engine = running();

        let set = set_musician_fx_return_with(
            &engine,
            &session,
            &address_key(ADDRESS),
            Effect::Reverb,
            40,
        )
        .expect("set");
        assert_eq!(
            serde_json::to_value(&set).expect("serializes"),
            serde_json::json!({
                "musicianKey": address_key(ADDRESS),
                "outcome": "set",
                "effect": "reverb",
                "returns": { "reverb": 40, "delay": 0 },
                "saved": true,
                "saveError": null,
                "message": "This musician's mix returns the reverb at 40%."
            })
        );
        assert_eq!(
            session.fx_returns(),
            vec![(
                client_key(PHONE),
                EffectLevels {
                    reverb: 40,
                    delay: 0
                }
            )],
            "kept under the key the engine kept it under"
        );
        set_musician_fx_return_with(&engine, &session, &client_key(PHONE), Effect::Reverb, 0)
            .expect("set");
        assert_eq!(
            session.fx_returns(),
            Vec::new(),
            "nothing returned keeps nothing"
        );

        let local = set_local_fx_return_with(&engine, &session, Effect::Delay, 25).expect("set");
        assert_eq!(local.musician_key, None);
        assert_eq!(local.message, "The local mix returns the delay at 25%.");
        assert_eq!(
            local_fx_returns_of(&session.current()),
            EffectLevels {
                reverb: 0,
                delay: 25
            }
        );

        let nobody = set_musician_fx_return_with(&engine, &session, "client:x", Effect::Delay, 5)
            .expect_err("nobody");
        assert!(nobody.contains("not here"), "{nobody}");
        assert!(set_musician_fx_return_with(&engine, &session, "Ana", Effect::Delay, 5).is_err());
    }

    #[test]
    fn an_engine_is_handed_the_saved_effects_and_returns() {
        let session = SessionStore::open(&test_directory("hand"));
        session
            .set_effects(Some(EffectSettings {
                delay: DelaySettings {
                    time_ms: 250.0,
                    ..DelaySettings::default()
                },
                ..EffectSettings::default()
            }))
            .expect("kept");
        session
            .set_fx_returns(
                &client_key(PHONE),
                ADDRESS,
                Some(EffectLevels {
                    reverb: 10,
                    delay: 20,
                }),
            )
            .expect("kept");
        session
            .set_local_fx_returns(Some(EffectLevels {
                reverb: 5,
                delay: 0,
            }))
            .expect("kept");
        let engine = StandIn::default();

        assert!(hand_kept_effects(&engine, &session.current()).is_empty());
        assert_eq!(engine.effects.lock().expect("effects").delay.time_ms, 250.0);
        assert_eq!(
            *engine.replaced.lock().expect("replaced"),
            vec![vec![(
                MusicianKey::Client(PHONE.to_owned()),
                EffectLevels {
                    reverb: 10,
                    delay: 20
                }
            )]]
        );
        assert_eq!(
            *engine.local.lock().expect("local"),
            EffectLevels {
                reverb: 5,
                delay: 0
            }
        );

        let fresh = StandIn::default();
        *fresh.local.lock().expect("local") = EffectLevels {
            reverb: 50,
            delay: 50,
        };
        assert!(hand_kept_effects(&fresh, &Session::default()).is_empty());
        assert_eq!(*fresh.local.lock().expect("local"), EffectLevels::default());
        assert_eq!(
            *fresh.effects.lock().expect("effects"),
            EffectSettings::default()
        );

        let stopping = StandIn {
            stopping: true,
            ..StandIn::default()
        };
        assert_eq!(hand_kept_effects(&stopping, &session.current()).len(), 3);
    }
}
