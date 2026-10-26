import BandEarPlayback
import BandEarProtocol
import Foundation

/// What the receiver and the control connection report, for the diagnostics in Settings.
///
/// Ported from the Android client's `DiagnosticsUiState` (`ui/state/MonitorUiState.kt:385-421`) and
/// the readings Settings makes of it (`diagnosticsSummary`, `allReadings`, `recentErrors`,
/// `ui/screens/SettingsText.kt:10-50`). Counters are for the running session and start again at
/// every connect. ``linkHealth`` is judged on the phone from the same report the desk judges, with
/// the same thresholds.
public struct Diagnostics: Equatable, Sendable {
    /// One labelled number in the diagnostics.
    public struct Reading: Equatable, Sendable {
        public let label: String
        public let value: String

        public init(label: String, value: String) {
            self.label = label
            self.value = value
        }
    }

    public var packetsReceived: Int64
    /// Packets the stream skipped, from the sequence numbers.
    public var packetsLost: Int64
    public var packetsPerSecond: Double
    public var interArrivalMilliseconds: Double
    public var jitterMilliseconds: Double
    /// How far playback runs behind the newest sample written, smoothed.
    public var playbackBufferMilliseconds: Double
    /// The sample rate of the last packet; nil before the first one.
    public var sampleRateHz: Int?
    public var discardedSamples: Int64
    public var packetsWithDiscardedSamples: Int64
    /// Playback underruns reported in this session's link reports.
    public var underruns: Int64
    public var linkHealth: LinkHealth
    /// The last receiver or connection problem; nil when there was none.
    public var lastError: String?
    /// The last control connection problem; nil when there was none.
    public var controlError: String?

    /// The diagnostics of a session with nothing to report yet: zero counts, the rate not known.
    public init(
        packetsReceived: Int64 = 0, packetsLost: Int64 = 0, packetsPerSecond: Double = 0,
        interArrivalMilliseconds: Double = 0, jitterMilliseconds: Double = 0,
        playbackBufferMilliseconds: Double = 0, sampleRateHz: Int? = nil, discardedSamples: Int64 = 0,
        packetsWithDiscardedSamples: Int64 = 0, underruns: Int64 = 0, linkHealth: LinkHealth = .unknown,
        lastError: String? = nil, controlError: String? = nil
    ) {
        self.packetsReceived = packetsReceived
        self.packetsLost = packetsLost
        self.packetsPerSecond = packetsPerSecond
        self.interArrivalMilliseconds = interArrivalMilliseconds
        self.jitterMilliseconds = jitterMilliseconds
        self.playbackBufferMilliseconds = playbackBufferMilliseconds
        self.sampleRateHz = sampleRateHz
        self.discardedSamples = discardedSamples
        self.packetsWithDiscardedSamples = packetsWithDiscardedSamples
        self.underruns = underruns
        self.linkHealth = linkHealth
        self.lastError = lastError
        self.controlError = controlError
    }

    /// The diagnostics of what the session holds, as Android's reducer fills them from its events
    /// (`StatsUpdated`, `LinkStatsReported`, `AudioError`, `ControlError`,
    /// `ui/state/MonitorReducer.kt:164-184`).
    ///
    /// - `stats` is ``MonitorAudio/receiverStats``. A spacing, jitter or buffer the receiver has no
    ///   estimate for yet (nil) reads 0, as Android's do: its estimates start at 0.0 and it shows them
    ///   so until the first one. Before the first packet the rate is not known.
    /// - `linkReport` is ``MonitorAudio/linkReport``; nil, before the first report or once the
    ///   session ended, is ``LinkHealth/unknown``.
    /// - `underruns` is the session's running total of the reports' underruns, which the session
    ///   keeps.
    /// - `receiverError` is ``MonitorAudio/receiverError``, described as the error describes itself.
    /// - Android keeps one control problem, the latest. This session keeps two: why the connection was
    ///   lost (``ControlSession/connectionError``) and the desk's latest `error`
    ///   (``ControlSession/lastError``). The outage's reason is shown when there is one, since it
    ///   clears once the desk is heard from again, and the desk's error otherwise.
    public init(
        stats: ReceiverStats?, linkReport: LinkStatsReport?, underruns: Int64,
        receiverError: (any Error)?, deskError: DeskError?, connectionError: String?
    ) {
        self.init(
            packetsReceived: stats?.packets ?? 0,
            packetsLost: stats?.lost ?? 0,
            packetsPerSecond: stats?.packetsPerSecond ?? 0,
            interArrivalMilliseconds: stats?.interArrivalMilliseconds ?? 0,
            jitterMilliseconds: stats?.jitterMilliseconds ?? 0,
            playbackBufferMilliseconds: stats?.playbackBacklogMilliseconds ?? 0,
            sampleRateHz: stats?.sampleRate,
            discardedSamples: stats?.discardedSamples ?? 0,
            packetsWithDiscardedSamples: stats?.packetsWithDiscardedSamples ?? 0,
            underruns: underruns,
            linkHealth: linkReport.map(LinkHealth.init(phoneReport:)) ?? .unknown,
            lastError: receiverError.map { String(describing: $0) },
            controlError: connectionError ?? deskError?.message)
    }

    /// Lost packets as a percentage of those expected (received plus lost); 0 before any
    /// (`MonitorUiState.kt:414-420`).
    public var lossPercent: Double {
        let expected = packetsReceived &+ packetsLost
        return expected == 0 ? 0 : Double(packetsLost) * 100 / Double(expected)
    }

    /// The six readings of the compact diagnostics: the ones that tell whether the audio is healthy
    /// (`diagnosticsSummary`, `SettingsText.kt:16-27`). Numbers use a fixed format, whatever the
    /// phone's language, so they read the same as the desktop.
    public var summary: [Reading] {
        [
            Reading(label: "Packets / s", value: DiagnosticsWords.decimal(packetsPerSecond, places: 0)),
            Reading(label: "Loss", value: DiagnosticsWords.percent(lossPercent)),
            Reading(label: "Underruns", value: String(underruns)),
            Reading(label: "Jitter", value: DiagnosticsWords.milliseconds(jitterMilliseconds, places: 1)),
            Reading(
                label: "Playback buffer",
                value: DiagnosticsWords.milliseconds(playbackBufferMilliseconds, places: 0)),
            Reading(label: "Sample rate", value: DiagnosticsWords.sampleRate(sampleRateHz)),
        ]
    }

    /// Every counter, for the expander under the compact diagnostics, led by the control
    /// connection's `controlStatus` (``ConnectionStatus/statusDetail``; `allReadings`,
    /// `SettingsText.kt:29-44`).
    public func allReadings(controlStatus: String) -> [Reading] {
        [
            Reading(label: "Control channel", value: controlStatus),
            Reading(label: "Link health", value: linkHealth.words),
            Reading(label: "Packets received", value: String(packetsReceived)),
            Reading(label: "Packets lost", value: String(packetsLost)),
            Reading(label: "Loss", value: DiagnosticsWords.percent(lossPercent)),
            Reading(label: "Packets / s", value: DiagnosticsWords.decimal(packetsPerSecond, places: 1)),
            Reading(
                label: "Inter-arrival",
                value: DiagnosticsWords.milliseconds(interArrivalMilliseconds, places: 1)),
            Reading(label: "Jitter", value: DiagnosticsWords.milliseconds(jitterMilliseconds, places: 1)),
            Reading(
                label: "Playback buffer",
                value: DiagnosticsWords.milliseconds(playbackBufferMilliseconds, places: 0)),
            Reading(label: "Sample rate", value: DiagnosticsWords.sampleRate(sampleRateHz)),
            Reading(label: "Underruns", value: String(underruns)),
            Reading(label: "Discarded samples", value: String(discardedSamples)),
            Reading(label: "Packets with discarded samples", value: String(packetsWithDiscardedSamples)),
        ]
    }

    /// The last problems of the session, each named by where it came from; empty when there were
    /// none (`recentErrors`, `SettingsText.kt:46-50`).
    public var recentErrors: [String] {
        [lastError.map { "Last problem: \($0)" }, controlError.map { "Control channel: \($0)" }]
            .compactMap { $0 }
    }
}

/// The diagnostics card's fixed words and number formats (`ui/screens/SettingsScreen.kt:244-352`,
/// `ui/screens/SettingsText.kt:13-14,52-77`).
public enum DiagnosticsWords {
    /// The card's title.
    public static let title = "DIAGNOSTICS"
    /// Shown for a value the phone does not know yet.
    public static let unknownValue = "—"
    /// The title over the recent errors, in the expander.
    public static let recentErrorsTitle = "RECENT ERRORS"
    /// Shown under ``recentErrorsTitle`` when the session had none.
    public static let noRecentErrors = "None"

    /// The expander's button: what tapping it does.
    public static func expanderText(expanded: Bool) -> String {
        expanded ? "Hide all counters and recent errors" : "Show all counters and recent errors"
    }

    /// What a screen reader says the expander is.
    public static func expanderState(expanded: Bool) -> String {
        expanded ? "Expanded" : "Collapsed"
    }

    /// A sample rate in kilohertz: whole rates without decimals, others with one; ``unknownValue``
    /// before the rate is known (`sampleRateText`, `SettingsText.kt:52-57`). Kilohertz, not a
    /// grouped number of hertz, which reads as "48.000 Hz" under a Spanish locale.
    public static func sampleRate(_ hertz: Int?) -> String {
        guard let hertz else { return unknownValue }
        if hertz % 1_000 == 0 { return "\(hertz / 1_000) kHz" }
        return "\(decimal(Double(hertz) / 1_000, places: 1)) kHz"
    }

    /// `value` in milliseconds with `places` decimals, such as "1.4 ms".
    static func milliseconds(_ value: Double, places: Int) -> String {
        "\(decimal(value, places: places)) ms"
    }

    /// `value` as a percentage with two decimals, such as "0.02 %".
    static func percent(_ value: Double) -> String {
        "\(decimal(value, places: 2)) %"
    }

    /// `value` with exactly `places` decimals, as Android's `String.format(Locale.ROOT, "%.Nf")`
    /// writes it, whatever the phone's language (`SettingsText.kt:77`).
    ///
    /// Java's formatter rounds the shortest decimal that names the double, half up: 0.125 is
    /// "0.13" and 2.5 is "3". `String(format:)` rounds the binary value half to even and writes
    /// "0.12" and "2", so one lost packet in 800 would read differently on the two phones. The
    /// shortest decimal is Swift's own description of the double, so the rounding is done on that.
    static func decimal(_ value: Double, places: Int) -> String {
        guard value.isFinite else {
            return value.isNaN ? "NaN" : (value < 0 ? "-Infinity" : "Infinity")
        }
        guard var shortest = Decimal(string: "\(value)", locale: posix) else {
            return String(format: "%.\(places)f", locale: posix, value)
        }
        var rounded = Decimal()
        NSDecimalRound(&rounded, &shortest, places, .plain)
        let digits = rounded.description
        guard places > 0 else { return digits }
        let parts = digits.split(separator: ".", omittingEmptySubsequences: false)
        let fraction = parts.count > 1 ? String(parts[1]) : ""
        return "\(parts[0]).\(fraction)\(String(repeating: "0", count: places - fraction.count))"
    }

    /// The fixed format's locale: a point for decimals, no grouping (Android's `Locale.ROOT`).
    private static let posix = Locale(identifier: "en_US_POSIX")
}
