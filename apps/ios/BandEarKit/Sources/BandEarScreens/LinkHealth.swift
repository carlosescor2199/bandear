import BandEarPlayback

/// How the audio link to this phone is doing, with the same levels and words as the desktop: the
/// desk classifies a link as good, warning or bad; a bad link that receives no packets is silent,
/// one whose reports stopped is lost, and a link with no report yet is unknown.
///
/// Ported from the Android client's `LinkHealth` and its words (`ui/components/LinkHealth.kt`), its
/// judgement of the phone's own link (`phoneLinkHealth`, `ui/state/PhoneLinkHealth.kt`), and the
/// diagnostics header's summary (`linkSummaryText`, `ui/screens/SettingsText.kt:59-60`).
public enum LinkHealth: CaseIterable, Equatable, Sendable {
    case good
    case warning
    case bad
    case silent
    case lost
    case unknown

    /// Judges the phone's own link from one `report`, the one it also sends to the desk, with the
    /// desk's own thresholds (`apps/server/src/health.rs`), so the phone shows its link with the
    /// light the engineer sees for it (Android's `phoneLinkHealth`, `PhoneLinkHealth.kt:23-42`).
    ///
    /// Nothing received over the interval is ``silent``, whatever else the report says, which is
    /// what the desktop calls a bad link that hears no audio. Otherwise a receive timeout, two
    /// underruns a second or 2 % loss is ``bad``, and any underrun or 0.5 % loss is a ``warning``.
    /// The comparisons are made in integers, like the desk's, so a count exactly on a threshold is
    /// judged exactly. The phone never judges its link ``lost``: it is the one making the reports.
    public init(phoneReport report: LinkStatsReport) {
        if report.packetsReceived == 0 {
            self = .silent
            return
        }
        // Wrapping arithmetic, as Kotlin's `Long`, so no report can trap.
        let intervalMilliseconds = max(report.intervalMilliseconds, 1)
        let expected = report.packetsReceived &+ report.packetsLost
        func lossReaches(_ basisPoints: Int64) -> Bool {
            report.packetsLost &* Self.basisPointsPerWhole >= basisPoints &* expected
        }
        let underrunsBad =
            report.underruns &* Self.millisecondsPerSecond
            >= Self.badUnderrunsPerSecond &* intervalMilliseconds
        if report.receiveTimeouts > 0 || underrunsBad || lossReaches(Self.badLossBasisPoints) {
            self = .bad
        } else if report.underruns > 0 || lossReaches(Self.warningLossBasisPoints) {
            self = .warning
        } else {
            self = .good
        }
    }

    /// The short word for the level, as the desktop shows it.
    public var words: String {
        switch self {
        case .good: "Good"
        case .warning: "Warning"
        case .bad: "Bad"
        case .silent: "Silent"
        case .lost: "Lost"
        case .unknown: "Unknown"
        }
    }

    /// What the level means for the musician, in one short sentence.
    public var meaning: String {
        switch self {
        case .good: "Clean audio."
        case .warning: "An underrun or a little loss."
        case .bad: "Heavy loss or dropouts."
        case .silent: "No audio received."
        case .lost: "Reports stopped arriving."
        case .unknown: "No report yet."
        }
    }

    /// The colour of the link-health light: green, amber, red for every kind of bad, grey for
    /// unknown.
    public var tone: ConnectionTone {
        switch self {
        case .good: .green
        case .warning: .amber
        case .bad, .silent, .lost: .red
        case .unknown: .neutral
        }
    }

    /// What a screen reader says for the light: the word and its meaning, never the colour.
    public var spokenDescription: String {
        "Link health: \(words). \(meaning)"
    }

    /// The light's words in the diagnostics header, such as "Link good".
    public var summaryText: String {
        "Link \(words.lowercased())"
    }

    // The desk's link thresholds (`apps/server/src/health.rs`, `PhoneLinkHealth.kt:6-21`).

    /// Underruns per second at or above which the link is bad.
    private static let badUnderrunsPerSecond: Int64 = 2
    /// Loss, in basis points (hundredths of a percent), at or above which the link is bad: 2 %.
    private static let badLossBasisPoints: Int64 = 200
    /// Loss, in basis points, at or above which the link needs attention: 0.5 %.
    private static let warningLossBasisPoints: Int64 = 50
    private static let basisPointsPerWhole: Int64 = 10_000
    private static let millisecondsPerSecond: Int64 = 1_000
}
