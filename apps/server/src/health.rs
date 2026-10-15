//! Link health: what a musician's phone reports about the audio it receives, and the traffic-light
//! level the engineer sees for it.
//!
//! The phone knows what went wrong on its side (lost packets, playback underruns, silences with no
//! packet at all); the server only knows what it sent. The phone therefore reports its counts for
//! each interval and the server classifies them here, in one pure function, so the thresholds are
//! tested and change in exactly one place.
//!
//! The audio path never touches this module: the network thread does not read or lock the reports.
//! Only the control connection writes a report and only the status snapshot reads one.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::protocol::StatsCommand;

/// How long a report stays meaningful. A phone reports about once a second, so a report older than
/// this means the phone stopped reporting (it left, or it predates stats support) and its last
/// numbers no longer describe what the musician hears.
pub(crate) const REPORT_STALE_AFTER: Duration = Duration::from_secs(3);

/// Underruns per second at or above which the link is bad: the phone is audibly starving.
pub(crate) const BAD_UNDERRUNS_PER_SECOND: u64 = 2;

/// Packet loss, in basis points (hundredths of a percent), at or above which the link is bad: 2 %.
pub(crate) const BAD_LOSS_BASIS_POINTS: u64 = 200;

/// Packet loss, in basis points, at or above which the link needs attention: 0.5 %.
pub(crate) const WARNING_LOSS_BASIS_POINTS: u64 = 50;

/// One basis point is a ten-thousandth of the whole.
const BASIS_POINTS_PER_WHOLE: u64 = 10_000;

const MILLISECONDS_PER_SECOND: u64 = 1_000;

/// The traffic-light level of one musician's link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LinkHealth {
    /// No report, or the last one is too old to describe the link.
    #[default]
    Unknown,
    Good,
    Warning,
    Bad,
}

/// The latest report a phone sent, as the server keeps it: one interval's counts and when they
/// arrived.
///
/// `interval_ms` is never 0 for a report built from the wire: the `stats` parser rejects it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LinkReport {
    pub(crate) interval_ms: u32,
    pub(crate) packets_received: u64,
    pub(crate) packets_lost: u64,
    pub(crate) underruns: u64,
    pub(crate) receive_timeouts: u64,
    pub(crate) received_at: Instant,
}

impl LinkReport {
    /// Stamps a parsed `stats` message with the moment it arrived.
    pub(crate) fn from_command(command: &StatsCommand, received_at: Instant) -> Self {
        Self {
            interval_ms: command.interval_ms,
            packets_received: command.packets_received,
            packets_lost: command.packets_lost,
            underruns: command.underruns,
            receive_timeouts: command.receive_timeouts,
            received_at,
        }
    }

    /// The report when it still describes the link at `now`, or `None` once it is stale.
    fn fresh(report: Option<&Self>, now: Instant) -> Option<&Self> {
        report.filter(|report| {
            now.saturating_duration_since(report.received_at) <= REPORT_STALE_AFTER
        })
    }

    /// The packets the phone expected over the interval: those that arrived plus those that did not.
    fn packets_expected(&self) -> u128 {
        u128::from(self.packets_received) + u128::from(self.packets_lost)
    }

    /// Whether `count` over the interval reaches `per_second`, compared without division.
    fn rate_reaches(&self, count: u64, per_second: u64) -> bool {
        u128::from(count) * u128::from(MILLISECONDS_PER_SECOND)
            >= u128::from(per_second) * u128::from(self.interval_ms)
    }

    /// Whether the loss reaches `basis_points`. Nothing expected means nothing lost, never 0/0.
    fn loss_reaches(&self, basis_points: u64) -> bool {
        let expected = self.packets_expected();
        expected > 0
            && u128::from(self.packets_lost) * u128::from(BASIS_POINTS_PER_WHOLE)
                >= u128::from(basis_points) * expected
    }

    /// `count` over the interval, as a rate per second.
    fn per_second(&self, count: u64) -> f64 {
        count as f64 * MILLISECONDS_PER_SECOND as f64 / f64::from(self.interval_ms)
    }

    /// Lost packets as a percentage of the expected ones; 0 when nothing was expected.
    fn loss_percent(&self) -> f64 {
        let expected = self.packets_expected();
        if expected == 0 {
            return 0.0;
        }
        self.packets_lost as f64 * 100.0 / expected as f64
    }
}

/// The latest report for one target, shared between the control connection that writes it and the
/// status snapshot that reads it. The lock is only ever held for a copy, never across an `.await`.
pub(crate) type LatestLinkReport = Arc<Mutex<Option<LinkReport>>>;

/// One musician's link as the status reports it: the level plus the rates it was judged on.
///
/// The rates are `None` when there is no fresh report, so a UI never shows numbers that no longer
/// describe the link.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LinkStatus {
    pub health: LinkHealth,
    pub underruns_per_second: Option<f64>,
    /// Lost packets as a percentage of the packets the phone expected (received plus lost).
    pub loss_percent: Option<f64>,
    pub receive_timeouts_per_second: Option<f64>,
}

/// Classifies a link from its latest report.
///
/// Rates are normalised by the report's interval, so a phone reporting every 500 ms and one
/// reporting every 2 s are judged on the same scale. The comparisons are done in integers so a
/// count that lands exactly on a threshold is classified exactly, with no rounding at the boundary.
pub(crate) fn classify(report: Option<&LinkReport>, now: Instant) -> LinkHealth {
    let Some(report) = LinkReport::fresh(report, now) else {
        return LinkHealth::Unknown;
    };
    if report.receive_timeouts > 0
        || report.rate_reaches(report.underruns, BAD_UNDERRUNS_PER_SECOND)
        || report.loss_reaches(BAD_LOSS_BASIS_POINTS)
    {
        return LinkHealth::Bad;
    }
    if report.underruns > 0 || report.loss_reaches(WARNING_LOSS_BASIS_POINTS) {
        return LinkHealth::Warning;
    }
    LinkHealth::Good
}

/// The level together with the rates it was judged on, for the status snapshot.
pub(crate) fn link_status(report: Option<&LinkReport>, now: Instant) -> LinkStatus {
    let Some(report) = LinkReport::fresh(report, now) else {
        return LinkStatus::default();
    };
    LinkStatus {
        health: classify(Some(report), now),
        underruns_per_second: Some(report.per_second(report.underruns)),
        loss_percent: Some(report.loss_percent()),
        receive_timeouts_per_second: Some(report.per_second(report.receive_timeouts)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(
        interval_ms: u32,
        packets_received: u64,
        packets_lost: u64,
        underruns: u64,
        receive_timeouts: u64,
        received_at: Instant,
    ) -> LinkReport {
        LinkReport {
            interval_ms,
            packets_received,
            packets_lost,
            underruns,
            receive_timeouts,
            received_at,
        }
    }

    /// A one-second report with the given loss and underruns and no timeouts, received `now`.
    fn one_second(received: u64, lost: u64, underruns: u64, now: Instant) -> LinkReport {
        report(1_000, received, lost, underruns, 0, now)
    }

    #[test]
    fn no_report_is_unknown() {
        assert_eq!(classify(None, Instant::now()), LinkHealth::Unknown);
    }

    #[test]
    fn a_clean_report_is_good() {
        let now = Instant::now();

        assert_eq!(
            classify(Some(&one_second(200, 0, 0, now)), now),
            LinkHealth::Good
        );
    }

    #[test]
    fn a_report_is_fresh_up_to_the_stale_limit_and_unknown_just_after_it() {
        let received_at = Instant::now();
        let clean = one_second(200, 0, 0, received_at);

        assert_eq!(
            classify(Some(&clean), received_at + REPORT_STALE_AFTER),
            LinkHealth::Good,
            "a report exactly at the limit still describes the link"
        );
        assert_eq!(
            classify(
                Some(&clean),
                received_at + REPORT_STALE_AFTER + Duration::from_millis(1)
            ),
            LinkHealth::Unknown,
            "a report older than the limit no longer describes the link"
        );
    }

    #[test]
    fn a_stale_bad_report_is_unknown_rather_than_bad() {
        let received_at = Instant::now();
        let bad = report(1_000, 0, 0, 0, 3, received_at);

        assert_eq!(
            classify(Some(&bad), received_at + Duration::from_secs(10)),
            LinkHealth::Unknown
        );
    }

    #[test]
    fn nothing_received_and_nothing_lost_counts_as_no_loss() {
        let now = Instant::now();

        assert_eq!(
            classify(Some(&one_second(0, 0, 0, now)), now),
            LinkHealth::Good
        );
        assert_eq!(
            link_status(Some(&one_second(0, 0, 0, now)), now).loss_percent,
            Some(0.0)
        );
    }

    #[test]
    fn any_receive_timeout_is_bad() {
        let now = Instant::now();

        assert_eq!(
            classify(Some(&report(1_000, 200, 0, 0, 1, now)), now),
            LinkHealth::Bad
        );
    }

    #[test]
    fn two_underruns_per_second_are_bad_and_fewer_are_a_warning() {
        let now = Instant::now();

        assert_eq!(
            classify(Some(&one_second(200, 0, 2, now)), now),
            LinkHealth::Bad
        );
        assert_eq!(
            classify(Some(&one_second(200, 0, 1, now)), now),
            LinkHealth::Warning,
            "any underrun is worth a look"
        );
    }

    #[test]
    fn underruns_are_normalised_by_the_interval() {
        let now = Instant::now();

        // Three underruns over two seconds is 1.5 per second: below the bad rate.
        assert_eq!(
            classify(Some(&report(2_000, 400, 0, 3, 0, now)), now),
            LinkHealth::Warning
        );
        // Four over two seconds is exactly the bad rate.
        assert_eq!(
            classify(Some(&report(2_000, 400, 0, 4, 0, now)), now),
            LinkHealth::Bad
        );
        // Two in half a second is four per second.
        assert_eq!(
            classify(Some(&report(500, 100, 0, 2, 0, now)), now),
            LinkHealth::Bad
        );
    }

    #[test]
    fn two_percent_loss_is_bad_and_just_below_is_a_warning() {
        let now = Instant::now();

        // 4 of 200 expected packets lost: exactly 2 %.
        assert_eq!(
            classify(Some(&one_second(196, 4, 0, now)), now),
            LinkHealth::Bad
        );
        // 3 of 200: 1.5 %.
        assert_eq!(
            classify(Some(&one_second(197, 3, 0, now)), now),
            LinkHealth::Warning
        );
    }

    #[test]
    fn half_a_percent_loss_is_a_warning_and_just_below_is_good() {
        let now = Instant::now();

        // 1 of 200 expected packets lost: exactly 0.5 %.
        assert_eq!(
            classify(Some(&one_second(199, 1, 0, now)), now),
            LinkHealth::Warning
        );
        // 1 of 400: 0.25 %.
        assert_eq!(
            classify(Some(&one_second(399, 1, 0, now)), now),
            LinkHealth::Good
        );
    }

    #[test]
    fn huge_counts_do_not_overflow_the_classification() {
        let now = Instant::now();

        assert_eq!(
            classify(Some(&report(1, u64::MAX, u64::MAX, u64::MAX, 0, now)), now),
            LinkHealth::Bad
        );
    }

    #[test]
    fn link_status_reports_the_rates_normalised_by_the_interval() {
        let now = Instant::now();

        let status = link_status(Some(&report(2_000, 990, 10, 1, 0, now)), now);

        assert_eq!(status.health, LinkHealth::Warning);
        assert_eq!(status.underruns_per_second, Some(0.5));
        assert_eq!(status.loss_percent, Some(1.0));
        assert_eq!(status.receive_timeouts_per_second, Some(0.0));
    }

    #[test]
    fn link_status_has_no_rates_without_a_fresh_report() {
        let received_at = Instant::now();
        let stale = one_second(200, 0, 0, received_at);

        assert_eq!(link_status(None, received_at), LinkStatus::default());
        assert_eq!(
            link_status(Some(&stale), received_at + Duration::from_secs(4)),
            LinkStatus::default()
        );
        assert_eq!(LinkStatus::default().health, LinkHealth::Unknown);
    }

    #[test]
    fn a_report_keeps_the_counts_of_the_stats_message_and_its_arrival_time() {
        let command = crate::protocol::parse_stats_command(
            r#"{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":3,"underruns":1,"receive_timeouts":2}"#,
        )
        .expect("a stats report should parse");
        let now = Instant::now();

        assert_eq!(
            LinkReport::from_command(&command, now),
            report(1_000, 200, 3, 1, 2, now)
        );
    }
}
