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
//!   it, and the first sample of the new session is marked [LinkSample::reconnected]. A phone that
//!   reports without registering (on a configured target) starts its session with its first
//!   report. It is gone when the engine stops: a new engine starts every history empty, each with
//!   a new [LinkHistoryRead::history_id].
//! - At most [HISTORY_SAMPLES] samples are kept, in a ring allocated once with the history; the
//!   oldest goes first.
//!
//! Time: the samples are ordered, spaced and found on the monotonic clock ([LinkMoment::at]), never
//! on the wall clock, which the system may step back or forward at any time (a time sync, the user
//! changing the clock). Each sample has a sequence number ([LinkSample::seq], the cursor a reader
//! passes to get only what is new) and its monotonic time since the history began
//! ([LinkSample::mono_ms], the axis a chart draws on); its wall clock time ([LinkSample::at_ms]) is
//! for display only. A gap only ever stands for a second that really passed.
//!
//! The round trip is the control channel's: each heartbeat ping carries a number and the pong that
//! answers it gives the time between the two (see [PingClock]). The phone's WebSocket library
//! answers pings on its own, so every phone has one, an older build too. It is kept as the last
//! value and a smoothed one ([RoundTrip]) while a control connection is open.
//!
//! The audio path never touches this module: only the control server writes a history and only the
//! status and the history read take a copy, each under the history's own short lock.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
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

/// The id the next history gets: unique in the process, so a reader can tell a new history (an
/// engine restart, another phone taking the row over) from the one it was following.
static NEXT_HISTORY_ID: AtomicU64 = AtomicU64::new(1);

/// The control channel's round trip, in milliseconds: the last one measured and a smoothed one.
/// Both are `None` until the session's first pong.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RoundTrip {
    pub last_ms: Option<f64>,
    pub smoothed_ms: Option<f64>,
}

/// A moment as a history reads it: the monotonic clock that orders and spaces the samples and the
/// wall clock shown beside them.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LinkMoment {
    pub(crate) at: Instant,
    /// Unix milliseconds (UTC), for display only.
    pub(crate) wall_ms: u64,
}

impl LinkMoment {
    pub(crate) fn now() -> Self {
        Self {
            at: Instant::now(),
            wall_ms: unix_millis_now(),
        }
    }

    /// The wall clock time of the earlier moment `at`, read back from this one's wall clock, so a
    /// gap is shown on the wall clock in force now.
    fn wall_ms_of(self, at: Instant) -> u64 {
        let before =
            u64::try_from(self.at.saturating_duration_since(at).as_millis()).unwrap_or(u64::MAX);
        self.wall_ms.saturating_sub(before)
    }
}

/// One second of a musician's link.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LinkSample {
    /// The sample's number in its history: 1 for the first, one more for each after it. A reader
    /// passes the newest it has to get only what is new.
    pub seq: u64,
    /// When it was taken on the monotonic clock, in milliseconds since the history began: it only
    /// ever grows, by the time that really passed. The axis a chart draws on.
    pub mono_ms: u64,
    /// When it was taken on the wall clock, in Unix milliseconds (UTC): when its report arrived,
    /// or for a gap the second it stands for. For display only: the wall clock may be stepped, so
    /// two samples need not be in this order.
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
    /// The gap sample at `mono_ms` and `at_ms`, with the round trip measured last; numbered when
    /// it is kept.
    fn gap(mono_ms: u64, at_ms: u64, rtt_ms: Option<f64>) -> Self {
        Self {
            seq: 0,
            mono_ms,
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

/// What a read of a history answers: which history it is and its samples after the cursor.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkHistoryRead {
    /// Unique in the process: a reader that sees another one drops what it had and reads it whole
    /// (the engine restarted, or another phone took the row over).
    pub history_id: u64,
    /// The samples after the cursor, the oldest first.
    pub samples: Vec<LinkSample>,
}

/// One musician's history, shared between the control server that writes it and the status that
/// reads it. The lock is only ever held for a short copy, never across an `.await`.
pub(crate) type SharedLinkHistory = Arc<Mutex<LinkHistory>>;

/// One musician's samples and round trip (see the module documentation).
#[derive(Debug)]
pub(crate) struct LinkHistory {
    id: u64,
    /// When the history began, on the monotonic clock: [LinkSample::mono_ms] counts from here.
    epoch: Instant,
    /// The number the next sample gets.
    next_seq: u64,
    samples: VecDeque<LinkSample>,
    /// Whether a control session runs: only then is a missing second a gap.
    in_session: bool,
    /// When the last sample of this session stands for, on the monotonic clock; `None` before its
    /// first report, so a session that never reports leaves no gaps.
    last_at: Option<Instant>,
    /// The next sample is the first of a session that followed earlier samples.
    reconnect_pending: bool,
    round_trip: RoundTrip,
}

impl Default for LinkHistory {
    fn default() -> Self {
        Self {
            id: NEXT_HISTORY_ID.fetch_add(1, Ordering::Relaxed),
            epoch: Instant::now(),
            next_seq: 1,
            samples: VecDeque::with_capacity(HISTORY_SAMPLES),
            in_session: false,
            last_at: None,
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
        self.round_trip = RoundTrip::default();
        self.start_session();
    }

    /// The control session ended (its last connection closed): no more gaps until the next one,
    /// and no round trip, as there is no connection to measure.
    pub(crate) fn end_session(&mut self) {
        self.in_session = false;
        self.last_at = None;
        self.round_trip = RoundTrip::default();
    }

    /// Takes the sample for `report`, the `stats` message `command` it was read from, arriving at
    /// `now`; the seconds missing since the last sample are kept as gaps first. A report outside a
    /// session starts one, for a phone that reports without registering (on a configured target):
    /// marked as a reconnect after earlier samples, keeping the round trip its connection measured
    /// (the last connection's went when it closed).
    pub(crate) fn record_report(
        &mut self,
        report: &LinkReport,
        command: &StatsCommand,
        now: LinkMoment,
    ) {
        if !self.in_session {
            self.start_session();
        }
        self.fill_gaps(now);
        // Judged on its own numbers, as if it had just arrived.
        let judged = link_status(Some(report), true, report.received_at);
        self.push(LinkSample {
            seq: 0,
            mono_ms: self.mono_ms_of(now.at),
            at_ms: now.wall_ms,
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
        self.last_at = Some(now.at);
    }

    /// Keeps the seconds with no report up to `now` as gaps; the control server's tick. Nothing
    /// outside a session or before its first report. Only the monotonic clock counts the seconds,
    /// so a tick never adds more gaps than seconds really passed (and never more than the ring),
    /// whatever the wall clock did.
    pub(crate) fn fill_gaps(&mut self, now: LinkMoment) {
        let Some(mut last) = self.last_at.filter(|_| self.in_session) else {
            return;
        };
        let period = Duration::from_millis(SAMPLE_PERIOD_MS);
        let silent = now.at.saturating_duration_since(last);
        let Some(overdue) = silent.checked_sub(Duration::from_millis(GAP_AFTER_MS)) else {
            return;
        };
        // Every whole second from the last sample up to `GAP_AFTER_MS` before now is missing.
        let overdue_ms = u64::try_from(overdue.as_millis()).unwrap_or(u64::MAX);
        let missing = overdue_ms / SAMPLE_PERIOD_MS + 1;
        // A silence longer than the ring keeps only its last ring's worth of gaps.
        let kept = missing.min(HISTORY_SAMPLES as u64);
        last += Duration::from_millis((missing - kept).saturating_mul(SAMPLE_PERIOD_MS));
        for _ in 0..kept {
            last += period;
            let gap = LinkSample::gap(
                self.mono_ms_of(last),
                now.wall_ms_of(last),
                self.round_trip.last_ms,
            );
            self.push(gap);
        }
        self.last_at = Some(last);
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

    /// The samples numbered after `since_seq`, the oldest first; all of them with no `since_seq`.
    /// A reader that passes the newest [LinkSample::seq] it has gets only what is new.
    pub(crate) fn samples_since(&self, since_seq: Option<u64>) -> Vec<LinkSample> {
        self.samples
            .iter()
            .filter(|sample| since_seq.is_none_or(|since| sample.seq > since))
            .copied()
            .collect()
    }

    /// The history's id and its samples after `since_seq` (see [Self::samples_since]).
    pub(crate) fn read(&self, since_seq: Option<u64>) -> LinkHistoryRead {
        LinkHistoryRead {
            history_id: self.id,
            samples: self.samples_since(since_seq),
        }
    }

    /// Starts a session without touching the round trip (see [Self::begin_session]).
    fn start_session(&mut self) {
        self.in_session = true;
        self.last_at = None;
        self.reconnect_pending = !self.samples.is_empty();
    }

    /// `at` in milliseconds since the history began.
    fn mono_ms_of(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.epoch).as_millis()).unwrap_or(u64::MAX)
    }

    /// Adds `sample` as the newest, numbered, dropping the oldest when the ring is full.
    fn push(&mut self, mut sample: LinkSample) {
        if self.samples.len() == HISTORY_SAMPLES {
            self.samples.pop_front();
        }
        sample.seq = self.next_seq;
        self.next_seq += 1;
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

/// A test moment `ms` milliseconds after one fixed instant of the test run, its wall clock showing
/// the same number: tests that never step the wall clock read their times as before. The instant
/// lies a day ahead, so it is after every history a test makes (a history counts from when it
/// began).
#[cfg(test)]
pub(crate) fn test_moment(ms: u64) -> LinkMoment {
    use std::sync::OnceLock;
    static BASE: OnceLock<Instant> = OnceLock::new();
    let base = *BASE.get_or_init(|| Instant::now() + Duration::from_secs(86_400));
    LinkMoment {
        at: base + Duration::from_millis(ms),
        wall_ms: ms,
    }
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

    /// The moment `ms` after the test's start, its wall clock at `START` plus `ms`.
    fn at(ms: u64) -> LinkMoment {
        LinkMoment {
            wall_ms: START + ms,
            ..test_moment(ms)
        }
    }

    /// The moment `ms` after the test's start with its wall clock at `wall_ms`.
    fn at_wall(ms: u64, wall_ms: u64) -> LinkMoment {
        LinkMoment { wall_ms, ..at(ms) }
    }

    fn record_at(history: &mut LinkHistory, now: LinkMoment) {
        let (report, command) = report(200, 0, "");
        history.record_report(&report, &command, now);
    }

    fn record(history: &mut LinkHistory, ms: u64) {
        record_at(history, at(ms));
    }

    /// Each sample's wall clock time, relative to `START`, and whether it is a gap.
    fn times(history: &LinkHistory) -> Vec<(u64, bool)> {
        history
            .samples_since(None)
            .iter()
            .map(|sample| (sample.at_ms - START, sample.gap))
            .collect()
    }

    /// Each sample's number and its monotonic time since the first one's.
    fn steps(history: &LinkHistory) -> Vec<(u64, u64, bool)> {
        let samples = history.samples_since(None);
        let first = samples.first().map_or(0, |sample| sample.mono_ms);
        samples
            .iter()
            .map(|sample| (sample.seq, sample.mono_ms - first, sample.gap))
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

        history.record_report(&report, &command, at(0));

        let sample = history.latest().expect("a sample");
        assert_eq!(sample.seq, 1, "the first sample");
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
        record(&mut history, 0);

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
        record(&mut history, 0);
        // Merely late: no gap.
        history.fill_gaps(at(1_999));
        record(&mut history, 1_900);
        assert_eq!(times(&history), vec![(0, false), (1_900, false)]);

        // Two seconds with nothing: the second after the last sample is a gap.
        history.fill_gaps(at(3_900));
        assert_eq!(history.latest().map(|sample| sample.gap), Some(true));
        assert_eq!(
            history.latest().map(|sample| sample.at_ms),
            Some(START + 2_900)
        );

        // A report long after fills every second before it, then is a sample of its own.
        record(&mut history, 6_900);
        assert_eq!(
            times(&history),
            vec![
                (0, false),
                (1_900, false),
                (2_900, true),
                (3_900, true),
                (4_900, true),
                (5_900, true),
                (6_900, false),
            ]
        );
        assert_eq!(
            steps(&history),
            vec![
                (1, 0, false),
                (2, 1_900, false),
                (3, 2_900, true),
                (4, 3_900, true),
                (5, 4_900, true),
                (6, 5_900, true),
                (7, 6_900, false),
            ],
            "numbered in order, on the monotonic clock"
        );
    }

    #[test]
    fn a_gap_carries_no_numbers_but_the_round_trip() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);
        history.record_round_trip(Duration::from_millis(8));

        history.fill_gaps(at(2_000));

        let gap = history.latest().expect("a gap");
        assert!(gap.gap);
        assert_eq!(gap.seq, 2);
        assert_eq!(gap.at_ms, START + 1_000);
        assert_eq!(gap.rtt_ms, Some(8.0));
        assert_eq!(gap.health, LinkHealth::Unknown);
        assert_eq!(gap.loss_percent, None);
        assert_eq!(gap.packets_received, None);
    }

    #[test]
    fn no_gaps_before_a_sessions_first_report_or_after_it_ends() {
        let mut history = LinkHistory::default();
        history.begin_session();
        history.fill_gaps(at(60_000));
        assert!(
            history.samples_since(None).is_empty(),
            "a phone that never reports has no history"
        );

        record(&mut history, 60_000);
        history.end_session();
        history.fill_gaps(at(120_000));
        assert_eq!(times(&history), vec![(60_000, false)], "gone: no gaps");
    }

    #[test]
    fn the_ring_keeps_the_last_ten_minutes_and_never_grows() {
        let mut history = LinkHistory::default();
        let capacity = history.samples.capacity();
        assert!(capacity >= HISTORY_SAMPLES, "allocated once, up front");
        history.begin_session();
        for second in 0..(HISTORY_SAMPLES as u64 + 50) {
            record(&mut history, second * SAMPLE_PERIOD_MS);
        }

        let samples = history.samples_since(None);
        assert_eq!(samples.len(), HISTORY_SAMPLES);
        assert_eq!(
            samples[0].at_ms,
            START + 50 * SAMPLE_PERIOD_MS,
            "the oldest went"
        );
        assert_eq!(samples[0].seq, 51, "numbers are never reused");
        assert_eq!(history.samples.capacity(), capacity);
    }

    #[test]
    fn a_very_long_silence_fills_at_most_the_ring() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);

        history.fill_gaps(at(100 * 24 * 3_600 * 1_000));

        let samples = history.samples_since(None);
        assert_eq!(samples.len(), HISTORY_SAMPLES);
        assert!(samples.iter().all(|sample| sample.gap));
    }

    /// The wall clock stepped back an hour (a time sync): the samples stay in order and spaced by
    /// the time that really passed, and the cursor still finds what is new.
    #[test]
    fn a_wall_clock_stepped_back_keeps_the_order_and_the_cursor() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);
        record(&mut history, 1_000);
        let hour = 3_600_000;
        let cursor = history.latest().expect("a sample").seq;

        record_at(&mut history, at_wall(2_000, START + 2_000 - hour));
        history.fill_gaps(at_wall(2_500, START + 2_500 - hour));
        record_at(&mut history, at_wall(3_000, START + 3_000 - hour));

        assert_eq!(
            steps(&history),
            vec![
                (1, 0, false),
                (2, 1_000, false),
                (3, 2_000, false),
                (4, 3_000, false)
            ],
            "no gaps, in order"
        );
        let newer = history.samples_since(Some(cursor));
        assert_eq!(
            newer.iter().map(|sample| sample.seq).collect::<Vec<_>>(),
            vec![3, 4],
            "the reader gets what came after the step"
        );
        assert_eq!(
            newer[0].at_ms,
            START + 2_000 - hour,
            "shown on the wall clock"
        );
    }

    /// The wall clock jumped forward twenty minutes: no gap stands for time that never passed, so
    /// the ring keeps its ten minutes of real samples.
    #[test]
    fn a_wall_clock_stepped_forward_adds_no_gaps() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);
        let jump = 20 * 60_000;

        history.fill_gaps(at_wall(500, START + 500 + jump));
        record_at(&mut history, at_wall(1_000, START + 1_000 + jump));
        history.fill_gaps(at_wall(1_500, START + 1_500 + jump));

        assert_eq!(
            steps(&history),
            vec![(1, 0, false), (2, 1_000, false)],
            "two samples, no gaps"
        );
    }

    /// A silence of five real seconds while the wall clock jumps: four gaps, one for each second
    /// that really passed up to the gap delay, shown on the wall clock in force now.
    #[test]
    fn gaps_count_only_the_seconds_that_really_passed() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);
        let jump = 30 * 60_000;

        history.fill_gaps(at_wall(5_000, START + 5_000 + jump));

        assert_eq!(
            steps(&history),
            vec![
                (1, 0, false),
                (2, 1_000, true),
                (3, 2_000, true),
                (4, 3_000, true),
                (5, 4_000, true)
            ]
        );
        assert_eq!(
            history.latest().map(|sample| sample.at_ms),
            Some(START + 4_000 + jump)
        );
    }

    #[test]
    fn a_reconnect_keeps_the_samples_and_marks_the_first_of_the_new_session() {
        let mut history = LinkHistory::default();
        history.begin_session();
        record(&mut history, 0);
        history.record_round_trip(Duration::from_millis(30));
        history.end_session();

        history.begin_session();
        assert_eq!(
            history.round_trip(),
            RoundTrip::default(),
            "the round trip starts over"
        );
        // The time away is no gap: the session had ended.
        history.fill_gaps(at(20_000));
        record(&mut history, 20_000);
        record(&mut history, 21_000);

        let samples = history.samples_since(None);
        assert_eq!(
            samples
                .iter()
                .map(|sample| (sample.at_ms - START, sample.reconnected))
                .collect::<Vec<_>>(),
            vec![(0, false), (20_000, true), (21_000, false)]
        );
    }

    #[test]
    fn a_report_without_a_registration_starts_a_session() {
        let mut history = LinkHistory::default();
        record(&mut history, 0);
        history.fill_gaps(at(2_000));

        assert_eq!(times(&history), vec![(0, false), (1_000, true)]);
    }

    /// A phone on a configured target never registers: when it comes back on a new connection, its
    /// first report starts the new session, marked as a reconnect, with no gaps for the time away
    /// and no round trip left from the connection that closed.
    #[test]
    fn a_report_after_the_session_ended_starts_a_new_one_marked_as_a_reconnect() {
        let mut history = LinkHistory::default();
        record(&mut history, 0);
        history.record_round_trip(Duration::from_millis(30));
        history.end_session();
        assert_eq!(
            history.round_trip(),
            RoundTrip::default(),
            "no connection, no round trip"
        );

        // The new connection measures its own before its first report.
        history.record_round_trip(Duration::from_millis(5));
        record(&mut history, 20_000);
        history.fill_gaps(at(22_000));

        let samples = history.samples_since(None);
        assert_eq!(
            samples
                .iter()
                .map(|sample| (sample.at_ms - START, sample.gap, sample.reconnected))
                .collect::<Vec<_>>(),
            vec![
                (0, false, false),
                (20_000, false, true),
                (21_000, true, false)
            ],
            "a reconnect, then gaps again in the new session"
        );
        assert_eq!(samples[1].rtt_ms, Some(5.0), "the new connection's");
    }

    #[test]
    fn samples_since_answers_only_the_newer_ones() {
        let mut history = LinkHistory::default();
        history.begin_session();
        for second in 0..5 {
            record(&mut history, second * SAMPLE_PERIOD_MS);
        }

        let newer = history.samples_since(Some(3));

        assert_eq!(
            newer.iter().map(|sample| sample.seq).collect::<Vec<_>>(),
            vec![4, 5]
        );
        assert_eq!(history.samples_since(Some(5)), Vec::new());
    }

    #[test]
    fn each_history_has_its_own_id() {
        let first = LinkHistory::default();
        let second = LinkHistory::default();

        assert_ne!(first.read(None).history_id, second.read(None).history_id);
        assert_eq!(first.read(None).history_id, first.read(Some(1)).history_id);
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
