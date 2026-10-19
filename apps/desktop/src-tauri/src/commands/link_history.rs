//! The `link_history` command: one musician's link over the last minutes, one sample a second (see
//! [personal_monitoring::LinkSample]), for the Network and system screen's chart.
//!
//! The status event never carries a history (it goes out four times a second); each row carries
//! only its newest sample and round trip (`linkLatest`, `rttMs`, `rttLastMs`). A window that charts
//! a musician reads the whole history once, then only what is newer by passing the newest `atMs`
//! it has as `sinceMs`.

use std::sync::Mutex;

use personal_monitoring::{EngineHandle, LinkSample, MusicianKey, HISTORY_SAMPLES};

use super::{musician_key_of, DesktopState};
use crate::dto::{LinkHistoryDto, LinkSampleDto};

/// The time one sample stands for, in milliseconds, as the window is told it.
const SAMPLE_PERIOD_MS: u64 = 1_000;

/// What the command reads from an engine: its history of the musician a key names.
pub(crate) trait LinkHistoryEngine {
    fn link_history(&self, key: &MusicianKey, since_ms: Option<u64>) -> Option<Vec<LinkSample>>;
}

impl LinkHistoryEngine for EngineHandle {
    fn link_history(&self, key: &MusicianKey, since_ms: Option<u64>) -> Option<Vec<LinkSample>> {
        EngineHandle::link_history(self, key, since_ms)
    }
}

/// The link history of the musician `musician_key` names (`client:<id>` or `address:<ip:port>`,
/// the keys the desk keeps musicians by), the samples after `since_ms` or all of them. A key this
/// build cannot read, one that names nobody, and no engine running are errors.
pub fn link_history(
    state: &DesktopState,
    musician_key: &str,
    since_ms: Option<u64>,
) -> Result<LinkHistoryDto, String> {
    link_history_with(&state.engine, musician_key, since_ms)
}

/// [link_history] over any engine slot.
pub(crate) fn link_history_with<E: LinkHistoryEngine>(
    engine: &Mutex<Option<E>>,
    musician_key: &str,
    since_ms: Option<u64>,
) -> Result<LinkHistoryDto, String> {
    let key = musician_key_of(musician_key)
        .ok_or_else(|| format!("`{musician_key}` names no phone or address"))?;
    let guard = engine
        .lock()
        .map_err(|_| "engine state lock was poisoned".to_owned())?;
    let engine = guard
        .as_ref()
        .ok_or_else(|| "the engine is not running".to_owned())?;
    let samples = engine
        .link_history(&key, since_ms)
        .ok_or_else(|| format!("`{musician_key}` names no musician"))?;
    Ok(LinkHistoryDto {
        musician_key: musician_key.to_owned(),
        samples: samples.iter().map(LinkSampleDto::from).collect(),
        max_samples: HISTORY_SAMPLES,
        period_ms: SAMPLE_PERIOD_MS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use personal_monitoring::LinkHealth;

    const PHONE: &str = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b";

    /// An engine with one phone's history: a report and a gap.
    struct StandIn;

    fn sample(at_ms: u64) -> LinkSample {
        LinkSample {
            at_ms,
            gap: false,
            reconnected: false,
            health: LinkHealth::Good,
            interval_ms: Some(1_000),
            packets_received: Some(200),
            packets_lost: Some(0),
            loss_percent: Some(0.0),
            underruns: Some(0),
            receive_timeouts: Some(0),
            rtt_ms: Some(4.5),
            jitter_ms: Some(1.25),
            cushion_ms: Some(40.0),
            backlog_ms: None,
            recovered: None,
        }
    }

    fn gap(at_ms: u64) -> LinkSample {
        LinkSample {
            gap: true,
            reconnected: true,
            health: LinkHealth::Unknown,
            interval_ms: None,
            packets_received: None,
            packets_lost: None,
            loss_percent: None,
            underruns: None,
            receive_timeouts: None,
            jitter_ms: None,
            cushion_ms: None,
            ..sample(at_ms)
        }
    }

    impl LinkHistoryEngine for StandIn {
        fn link_history(
            &self,
            key: &MusicianKey,
            since_ms: Option<u64>,
        ) -> Option<Vec<LinkSample>> {
            let named = match key {
                MusicianKey::Client(id) => id == PHONE,
                MusicianKey::Address(address) => address.to_string() == "192.168.1.30:50000",
            };
            named.then(|| {
                [sample(1_000), gap(2_000)]
                    .into_iter()
                    .filter(|sample| since_ms.is_none_or(|since| sample.at_ms > since))
                    .collect()
            })
        }
    }

    fn running() -> Mutex<Option<StandIn>> {
        Mutex::new(Some(StandIn))
    }

    #[test]
    fn the_history_reaches_the_window_in_camel_case_with_its_bounds() {
        let history = link_history_with(&running(), &format!("client:{PHONE}"), None)
            .expect("a musician's history");

        let json = serde_json::to_value(&history).expect("serializes");
        assert_eq!(
            json,
            serde_json::json!({
                "musicianKey": format!("client:{PHONE}"),
                "samples": [
                    {
                        "atMs": 1000, "gap": false, "reconnected": false, "health": "good",
                        "intervalMs": 1000, "packetsReceived": 200, "packetsLost": 0,
                        "lossPercent": 0.0, "underruns": 0, "receiveTimeouts": 0, "rttMs": 4.5,
                        "jitterMs": 1.25, "cushionMs": 40.0, "backlogMs": null, "recovered": null
                    },
                    {
                        "atMs": 2000, "gap": true, "reconnected": true, "health": "unknown",
                        "intervalMs": null, "packetsReceived": null, "packetsLost": null,
                        "lossPercent": null, "underruns": null, "receiveTimeouts": null,
                        "rttMs": 4.5, "jitterMs": null, "cushionMs": null, "backlogMs": null,
                        "recovered": null
                    }
                ],
                "maxSamples": 600,
                "periodMs": 1000
            })
        );
    }

    #[test]
    fn since_answers_only_the_newer_samples_and_an_address_key_works_too() {
        let history = link_history_with(&running(), "address:192.168.1.30:50000", Some(1_000))
            .expect("a musician's history");

        assert_eq!(history.musician_key, "address:192.168.1.30:50000");
        assert_eq!(
            history
                .samples
                .iter()
                .map(|sample| sample.at_ms)
                .collect::<Vec<_>>(),
            vec![2_000]
        );
    }

    #[test]
    fn an_unreadable_key_nobody_and_no_engine_are_errors() {
        let unreadable = link_history_with(&running(), "Ana", None).expect_err("no key form");
        assert!(
            unreadable.contains("names no phone or address"),
            "{unreadable}"
        );

        let nobody =
            link_history_with(&running(), "client:someone-else", None).expect_err("nobody");
        assert!(nobody.contains("names no musician"), "{nobody}");

        let stopped: Mutex<Option<StandIn>> = Mutex::new(None);
        assert_eq!(
            link_history_with(&stopped, &format!("client:{PHONE}"), None).expect_err("no engine"),
            "the engine is not running"
        );
    }
}
