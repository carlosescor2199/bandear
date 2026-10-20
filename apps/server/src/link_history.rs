//! Link history: one musician's link over the last ten minutes, one sample a second, and the round
//! trip of their control channel.
//!
//! - A sample is taken from each `stats` report the phone sends (about one a second): its level,
//!   loss, underruns and timeouts, the round trip measured last, and the phone's jitter, cushion,
//!   backlog and recovered packets when it reports them ([crate::CAPABILITY_STATS_V2],
//!   [crate::CAPABILITY_REDUNDANCY_RECOVERED]).
//! - A second with no report while the musician's control session runs is kept as a gap sample
//!   (no numbers, [LinkSample::gap]), never dropped: a chart shows where the reports stopped. A
//!   gap is written once the next report is [GAP_AFTER_MS] late, by the control server's own tick
//!   or by the report that ends it. Before a session's first report and after its control channel
//!   closed there are no gaps: a phone that never reports (an older build) has no history at all.
//! - The history follows the musician, not the control session: a reconnect (back within the grace
//!   period, from a new IP with its client id, or after the grace period with its client id) keeps
//!   it, and the first sample of the new session is marked [LinkSample::reconnected]. It is gone
//!   when the engine stops: a new engine starts every history empty.
//! - At most [HISTORY_SAMPLES] samples are kept, in a ring allocated once with the history; the
//!   oldest goes first.
//!
//! The round trip is the control channel's: each heartbeat ping carries a number and the pong that
//! answers it gives the time between the two (see [PingClock]). The phone's WebSocket library
//! answers pings on its own, so every phone has one, an older build too. It is kept as the last
//! value and a smoothed one ([RoundTrip]).
//!
//! The audio path never touches this module: only the control server writes a history and only the
//! status and the history read take a copy, each under the history's own short lock.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::health::{link_status, LinkHealth, LinkReport};
use crate::protocol::StatsCommand;

/// How many samples a history keeps: ten minutes of one a second.
pub const HISTORY_SAMPLES: usize = 600;

/// The time one sample stands for, in milliseconds.
pub(crate) const SAMPLE_PERIOD_MS: u64 = 1_000;

/// How late the next report may be before the second after the last sample is kept as a gap, in
/// milliseconds: two periods, so a report that is merely late (Wi-Fi jitter) is never a gap.
pub(crate) const GAP_AFTER_MS: u64 = 2 * SAMPLE_PERIOD_MS;

/// The weight of a new round trip in the smoothed one: an eighth, as TCP smooths its own (RFC
/// 6298), so one slow pong moves it a little and a lasting change moves it within seconds.
const ROUND_TRIP_SMOOTHING: f64 = 0.125;

/// The control channel's round trip, in milliseconds: the last one measured and a smoothed one.
/// Both are `None` until the session's first pong.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundTrip {
    pub last_ms: Option<f64>,
    pub smoothed_ms: Option<f64>,
}

/// One second of a musician's link.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkSample {
    /// When it was taken, in Unix milliseconds (UTC): when its report arrived, or for a gap the
    /// second it stands for.
    pub at_ms: u64,
    /// No report arrived for this second while the session ran: every number but the round trip is
    /// `None` and the level is [LinkHealth::Unknown] (the status itself calls the link lost once
    /// the reports stay away for [crate::health::REPORT_STALE_AFTER]).
    pub gap: bool,
    /// The first sample of a new control session after earlier ones: the phone reconnected, so
    /// whatever lies between this and the sample before it went unmeasured.
    pub reconnected: bool,
    /// The level the report reached on its own numbers.
    pub health: LinkHealth,
    /// The interval the report covered, in milliseconds.
    pub interval_ms: Option<u32>,
    pub packets_received: Option<u64>,
    pub packets_lost: Option<u64>,
    /// Lost packets as a percentage of the expected ones (received plus lost).
    pub loss_percent: Option<f64>,
    pub underruns: Option<u64>,
    pub receive_timeouts: Option<u64>,
    /// The control channel's round trip measured last, in milliseconds.
    pub rtt_ms: Option<f64>,
    /// The phone's own numbers, when it reports them (see [StatsCommand]).
    pub jitter_ms: Option<f64>,
    pub cushion_ms: Option<f64>,
    pub backlog_ms: Option<f64>,
    pub recovered: Option<u64>,
}

impl LinkSample {
    /// The gap sample for the second at `at_ms`, with the round trip measured last.
    fn gap(at_ms: u64, rtt_ms: Option<f64>) -> Self {
        Self {
            at_ms,
            gap: true,
            reconnected: false,
            health: LinkHealth::Unknown,
            interval_ms: None,
            packets_received: None,
            packets_lost: None,
            loss_percent: None,
            underruns: None,
            receive_timeouts: None,
            rtt_ms,
            jitter_ms: None,
            cushion_ms: None,
            backlog_ms: None,
            recovered: None,
        }
    }
}

/// One musician's history, shared between the control server that writes it and the status that
/// reads it. The lock is only ever held for a short copy, never across an `.await`.
pub(crate) type SharedLinkHistory = Arc<Mutex<LinkHistory>>;

/// One musician's samples and round trip (see the module documentation).
#[derive(Debug)]
pub(crate) struct LinkHistory {
    samples: VecDeque<LinkSample>,
    /// Whether a control session runs: only then is a missing second a gap.
    in_session: bool,
    /// When the last sample of this session stands for; `None` before its first report, so a
    /// session that never reports leaves no gaps.
    last_at_ms: Option<u64>,
    /// The next sample is the first of a session that followed earlier samples.
    reconnect_pending: bool,
    round_trip: RoundTrip,
}

impl Default for LinkHistory {
    fn default() -> Self {
        Self {
            samples: VecDeque::with_capacity(HISTORY_SAMPLES),
            in_session: false,
            last_at_ms: None,
            reconnect_pending: false,
            round_trip: RoundTrip::default(),
        }
    }
}

impl LinkHistory {
    /// A new history in its own lock, its ring allocated now.
    pub(crate) fn shared() -> SharedLinkHistory {
        Arc::new(Mutex::new(Self::default()))
    }

    /// A control session starts (the phone registered on a new connection). The samples are kept;
    /// the next one is marked as a reconnect when there are earlier ones. The round trip starts
    /// over: the last session's no longer describes this connection.
    pub(crate) fn begin_session(&mut self) {
        self.in_session = true;
        self.last_at_ms = None;
        self.reconnect_pending = !self.samples.is_empty();
        self.round_trip = RoundTrip::default();
    }

    /// The control session ended (its last connection closed): no more gaps until the next one.
    pub(crate) fn end_session(&mut self) {
        self.in_session = false;
        self.last_at_ms = None;
    }

    /// Takes the sample for `report`, the `stats` message `command` it was read from, arriving at
    /// `at_ms` (Unix milliseconds); the seconds missing since the last sample are kept as gaps
    /// first. A report also starts a session for a phone that reports without registering (on a
    /// configured target).
    pub(crate) fn record_report(
        &mut self,
        report: &LinkReport,
        command: &StatsCommand,
        at_ms: u64,
    ) {
        self.in_session = true;
        self.fill_gaps(at_ms);
        // Judged on its own numbers, as if it had just arrived.
        let judged = link_status(Some(report), true, report.received_at);
        self.push(LinkSample {
            at_ms,
            gap: false,
            reconnected: false,
            health: judged.health,
            interval_ms: Some(report.interval_ms),
            packets_received: Some(report.packets_received),
            packets_lost: Some(report.packets_lost),
            loss_percent: judged.loss_percent,
            underruns: Some(report.underruns),
            receive_timeouts: Some(report.receive_timeouts),
            rtt_ms: self.round_trip.last_ms,
            jitter_ms: command.jitter_ms,
            cushion_ms: command.cushion_ms,
            backlog_ms: command.backlog_ms,
            recovered: command.recovered,
        });
        self.last_at_ms = Some(at_ms);
    }

    /// Keeps the seconds with no report up to `now_ms` (Unix milliseconds) as gaps; the control
    /// server's tick. Nothing outside a session or before its first report.
    pub(crate) fn fill_gaps(&mut self, now_ms: u64) {
        let Some(mut last) = self.last_at_ms.filter(|_| self.in_session) else {
            return;
        };
        // A silence longer than the ring keeps only its last ring's worth of gaps.
        let longest = HISTORY_SAMPLES as u64 * SAMPLE_PERIOD_MS;
        if now_ms.saturating_sub(last) > longest + GAP_AFTER_MS {
            last = now_ms - GAP_AFTER_MS - longest;
        }
        while now_ms.saturating_sub(last) >= GAP_AFTER_MS {
            last += SAMPLE_PERIOD_MS;
            self.push(LinkSample::gap(last, self.round_trip.last_ms));
        }
        self.last_at_ms = Some(last);
    }

    /// A pong answered a ping after `round_trip`.
    pub(crate) fn record_round_trip(&mut self, round_trip: Duration) {
        let measured = round_trip.as_secs_f64() * 1_000.0;
        let smoothed = match self.round_trip.smoothed_ms {
            None => measured,
            Some(smoothed) => smoothed + ROUND_TRIP_SMOOTHING * (measured - smoothed),
        };
        self.round_trip = RoundTrip {
            last_ms: Some(measured),
            smoothed_ms: Some(smoothed),
        };
    }

    pub(crate) fn round_trip(&self) -> RoundTrip {
        self.round_trip
    }

    /// The newest sample, if any.
    pub(crate) fn latest(&self) -> Option<LinkSample> {
        self.samples.back().copied()
    }

    /// The samples taken after `since_ms` (Unix milliseconds), the oldest first; all of them with
    /// no `since_ms`. A reader that passes the newest `at_ms` it has gets only what is new.
    pub(crate) fn samples_since(&self, since_ms: Option<u64>) -> Vec<LinkSample> {
        self.samples
            .iter()
            .filter(|sample| since_ms.is_none_or(|since| sample.at_ms > since))
            .copied()
            .collect()
    }

    /// Adds `sample` as the newest, dropping the oldest when the ring is full.
    fn push(&mut self, mut sample: LinkSample) {
        if self.samples.len() == HISTORY_SAMPLES {
            self.samples.pop_front();
        }
        sample.reconnected = std::mem::take(&mut self.reconnect_pending);
        self.samples.push_back(sample);
    }
}

/// The numbers of the control channel's heartbeat pings: each ping carries the next one, and the
/// pong that carries it back measures the round trip.
///
/// Only the latest ping is waited for: a pong that answers an older one (it came back later than a
/// whole ping interval) measures nothing.
#[derive(Debug, Default)]
pub(crate) struct PingClock {
    next: u64,
    waiting: Option<(u64, Instant)>,
}

impl PingClock {
    /// The payload of a ping sent at `now`.
    pub(crate) fn ping(&mut self, now: Instant) -> [u8; 8] {
        let number = self.next;
        self.next = self.next.wrapping_add(1);
        self.waiting = Some((number, now));
        number.to_be_bytes()
    }

    /// The round trip of the ping a pong with `payload` that arrived at `now` answers; `None` for a
    /// pong that answers no ping waited for.
    pub(crate) fn pong(&mut self, payload: &[u8], now: Instant) -> Option<Duration> {
        let (number, sent) = self.waiting?;
        if payload != number.to_be_bytes() {
            return None;
        }
        self.waiting = None;
        Some(now.saturating_duration_since(sent))
    }
}

/// The time now in Unix milliseconds (UTC); 0 for a clock set before 1970.
pub(crate) fn unix_millis_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::parse_stats_command;

    /// A one-second report of `received` and `lost` packets with the phone's extras, as the
    /// control connection reads it.
    fn report(received: u64, lost: u64, extras: &str) -> (LinkReport, StatsCommand) {
        let command = parse_stats_command(&format!(
            r#"{{"type":"stats","interval_ms":1000,"packets_received":{received},"packets_lost":{lost},"underruns":1,"receive_timeouts":0{extras}}}"#
        ))
        .expect("a stats report");
        (LinkReport::from_command(&command, Instant::now()), command)
    }

    fn record(history: &mut LinkHistory, at_ms: u64) {
        let (report, command) = report(200, 0, "");
        history.record_report(&report, &command, at_ms);
    }

    fn times(history: &LinkHistory) -> Vec<(u64, bool)> {
        history
            .samples_since(None)
            .iter()
            .map(|sample| (sample.at_ms, sample.gap))
            .collect()
    }

    const START: u64 = 1_760_814_245_000;

    #[test]
    fn a_report_becomes_a_sample_with_its_level_counts_extras_and_round_trip() {
        let mut history = LinkHistory::default();
        history.begin_session();
        history.record_round_trip(Duration::from_millis(12));
        let (report, command) = report(
            196,
            4,
            r#","jitter_ms":2.5,"cushion_ms":40,"backlog_ms":35,"recovered":2"#,
        );

        history.record_report(&report, &command, START);

        let sample = history.latest().expect("a sample");
        assert_eq!(sample.at_ms, START);
        assert!(!sample.gap);
        assert!(!sample.reconnected, "the first session is no reconnect");
        assert_eq!(sample.health, LinkHealth::Bad, "2 % loss is bad");
        assert_eq!(sample.interval_ms, Some(1_000));
        assert_eq!(sample.packets_received, Some(196));
        assert_eq!(sample.packets_lost, Some(4));
        assert_eq!(sample.loss_percent, Some(2.0));
        assert_eq!(sample.underruns, Some(1));
        assert_eq!(sample.receive_timeouts, Some(0));
        assert_eq!(sample.rtt_ms, Some(12.0));
        assert_eq!(sample.jitter_ms, Some(2.5));
        assert_eq!(sample.cushion_ms, Some(40.0));
        assert_eq!(sample.backlog_ms, Some(35.0));
        assert_eq!(sample.recovered, Some(2));
    }

    #[test]
    fn an_older_phones_report_has_no_extras_and_no_round_trip_before_the_first_pong() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, START);

        let sample = history.latest().expect("a sample");
        assert_eq!(sample.health, LinkHealth::Warning, "one underrun");
        assert_eq!(sample.rtt_ms, None);
        assert_eq!(sample.jitter_ms, None);
        assert_eq!(sample.cushion_ms, None);
        assert_eq!(sample.backlog_ms, None);
        assert_eq!(sample.recovered, None);
    }

    #[test]
    fn a_second_with_no_report_is_kept_as_a_gap_and_a_late_report_is_not_one() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, START);
        // Merely late: no gap.
        history.fill_gaps(START + 1_999);
        record(&mut history, START + 1_900);
        assert_eq!(
            times(&history),
            vec![(START, false), (START + 1_900, false)]
        );

        // Two seconds with nothing: the second after the last sample is a gap.
        history.fill_gaps(START + 3_900);
        assert_eq!(history.latest().map(|sample| sample.gap), Some(true));
        assert_eq!(
            history.latest().map(|sample| sample.at_ms),
            Some(START + 2_900)
        );

        // A report long after fills every second before it, then is a sample of its own.
        record(&mut history, START + 6_900);
        assert_eq!(
            times(&history),
            vec![
                (START, false),
                (START + 1_900, false),
                (START + 2_900, true),
                (START + 3_900, true),
                (START + 4_900, true),
                (START + 5_900, true),
                (START + 6_900, false),
            ]
        );
    }

    #[test]
    fn a_gap_carries_no_numbers_but_the_round_trip() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, START);
        history.record_round_trip(Duration::from_millis(8));

        history.fill_gaps(START + 2_000);

        let gap = history.latest().expect("a gap");
        assert_eq!(gap, LinkSample::gap(START + 1_000, Some(8.0)));
        assert_eq!(gap.health, LinkHealth::Unknown);
        assert_eq!(gap.loss_percent, None);
    }

    #[test]
    fn no_gaps_before_a_sessions_first_report_or_after_it_ends() {
        let mut history = LinkHistory::default();
        history.begin_session();
        history.fill_gaps(START + 60_000);
        assert!(
            history.samples_since(None).is_empty(),
            "a phone that never reports has no history"
        );

        record(&mut history, START + 60_000);
        history.end_session();
        history.fill_gaps(START + 120_000);
        assert_eq!(
            times(&history),
            vec![(START + 60_000, false)],
            "gone: no gaps"
        );
    }

    #[test]
    fn the_ring_keeps_the_last_ten_minutes_and_never_grows() {
        let mut history = LinkHistory::default();
        let capacity = history.samples.capacity();
        assert!(capacity >= HISTORY_SAMPLES, "allocated once, up front");
        history.begin_session();
        for second in 0..(HISTORY_SAMPLES as u64 + 50) {
            record(&mut history, START + second * SAMPLE_PERIOD_MS);
        }

        let samples = history.samples_since(None);
        assert_eq!(samples.len(), HISTORY_SAMPLES);
        assert_eq!(
            samples[0].at_ms,
            START + 50 * SAMPLE_PERIOD_MS,
            "the oldest went"
        );
        assert_eq!(history.samples.capacity(), capacity);
    }

    #[test]
    fn a_very_long_silence_fills_at_most_the_ring() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, START);

        history.fill_gaps(START + 100 * 24 * 3_600 * 1_000);

        let samples = history.samples_since(None);
        assert_eq!(samples.len(), HISTORY_SAMPLES);
        assert!(samples.iter().all(|sample| sample.gap));
    }

    #[test]
    fn a_reconnect_keeps_the_samples_and_marks_the_first_of_the_new_session() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, START);
        history.record_round_trip(Duration::from_millis(30));
        history.end_session();

        history.begin_session();
        assert_eq!(
            history.round_trip(),
            RoundTrip::default(),
            "the round trip starts over"
        );
        // The time away is no gap: the session had ended.
        history.fill_gaps(START + 20_000);
        record(&mut history, START + 20_000);
        record(&mut history, START + 21_000);

        let samples = history.samples_since(None);
        assert_eq!(
            samples
                .iter()
                .map(|sample| (sample.at_ms, sample.reconnected))
                .collect::<Vec<_>>(),
            vec![
                (START, false),
                (START + 20_000, true),
                (START + 21_000, false)
            ]
        );
    }

    #[test]
    fn a_report_without_a_registration_starts_a_session() {
        let mut history = LinkHistory::default();
        record(&mut history, START);
        history.fill_gaps(START + 2_000);

        assert_eq!(times(&history), vec![(START, false), (START + 1_000, true)]);
    }

    #[test]
    fn samples_since_answers_only_the_newer_ones() {
        let mut history = LinkHistory::default();
        history.begin_session();
        for second in 0..5 {
            record(&mut history, START + second * SAMPLE_PERIOD_MS);
        }

        let newer = history.samples_since(Some(START + 2_000));

        assert_eq!(
            newer.iter().map(|sample| sample.at_ms).collect::<Vec<_>>(),
            vec![START + 3_000, START + 4_000]
        );
        assert_eq!(history.samples_since(Some(START + 4_000)), Vec::new());
    }

    #[test]
    fn the_round_trip_keeps_the_last_value_and_a_smoothed_one() {
        let mut history = LinkHistory::default();
        history.record_round_trip(Duration::from_millis(100));
        assert_eq!(
            history.round_trip(),
            RoundTrip {
                last_ms: Some(100.0),
                smoothed_ms: Some(100.0),
            },
            "the first measurement is the smoothed one"
        );

        history.record_round_trip(Duration::from_millis(20));
        assert_eq!(
            history.round_trip(),
            RoundTrip {
                last_ms: Some(20.0),
                smoothed_ms: Some(90.0),
            },
            "an eighth of the way to the new one"
        );

        history.record_round_trip(Duration::from_micros(2_500));
        assert_eq!(
            history.round_trip().last_ms,
            Some(2.5),
            "to the microsecond"
        );
    }

    #[test]
    fn a_pong_measures_the_round_trip_of_the_ping_it_answers() {
        let mut clock = PingClock::default();
        let sent = Instant::now();
        let first = clock.ping(sent);

        assert_eq!(clock.pong(b"other", sent), None, "answers no ping");
        assert_eq!(
            clock.pong(&first, sent + Duration::from_millis(30)),
            Some(Duration::from_millis(30))
        );
        assert_eq!(
            clock.pong(&first, sent + Duration::from_millis(40)),
            None,
            "measured once"
        );

        let second = clock.ping(sent + Duration::from_secs(2));
        let third = clock.ping(sent + Duration::from_secs(4));
        assert_ne!(second, third);
        assert_eq!(
            clock.pong(&second, sent + Duration::from_secs(5)),
            None,
            "a pong older than the latest ping measures nothing"
        );
        assert_eq!(
            clock.pong(&third, sent + Duration::from_millis(4_015)),
            Some(Duration::from_millis(15))
        );
        assert_eq!(clock.pong(&[], sent), None, "an empty pong");
    }
}
