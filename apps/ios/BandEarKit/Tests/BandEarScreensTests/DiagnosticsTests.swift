import BandEarPlayback
import BandEarProtocol
import Testing

@testable import BandEarScreens

/// What only the iOS port has to answer about the diagnostics: how this session's optional values
/// and two control problems read, the rounding Java's formatter does and `String(format:)` does
/// not, and the card's copy, which Android does not test.
@Suite("Diagnostics")
struct DiagnosticsTests {
    private struct ReceiverProblem: Error, CustomStringConvertible {
        var description: String { "UDP socket error: closed" }
    }

    private func stats(
        packets: Int64 = 100, lost: Int64 = 0, packetsPerSecond: Double = 100,
        jitter: Double? = 0.4, interArrival: Double? = 10, backlog: Double? = 12.5
    ) -> ReceiverStats {
        ReceiverStats(
            packets: packets, lost: lost, sampleRate: 44_100, packetsPerSecond: packetsPerSecond,
            interArrivalMilliseconds: interArrival, jitterMilliseconds: jitter, discardedSamples: 0,
            packetsWithDiscardedSamples: 0, playbackBacklogMilliseconds: backlog, outputPlaying: true)
    }

    private func diagnostics(
        stats: ReceiverStats? = nil, underruns: Int64 = 0, receiverError: (any Error)? = nil,
        deskError: DeskError? = nil, connectionError: String? = nil
    ) -> Diagnostics {
        Diagnostics(
            stats: stats, linkReport: nil, underruns: underruns, receiverError: receiverError,
            deskError: deskError, connectionError: connectionError)
    }

    @Test func nothingReceivedYetIsAFreshSession() {
        #expect(diagnostics() == Diagnostics())
    }

    @Test func anEstimateTheReceiverHasNotMadeYetReadsZeroAsOnAndroid() {
        let readings = diagnostics(stats: stats(jitter: nil, interArrival: nil, backlog: nil))
            .allReadings(controlStatus: "connecting...")
        let values = Dictionary(uniqueKeysWithValues: readings.map { ($0.label, $0.value) })

        #expect(values["Inter-arrival"] == "0.0 ms")
        #expect(values["Jitter"] == "0.0 ms")
        #expect(values["Playback buffer"] == "0 ms")
        #expect(values["Sample rate"] == "44.1 kHz")
    }

    @Test func theSessionsUnderrunTotalIsShown() {
        let summary = diagnostics(stats: stats(), underruns: 3).summary
        #expect(summary.first { $0.label == "Underruns" }?.value == "3")
    }

    @Test func numbersRoundHalfUpAsJavasFormatterDoes() throws {
        // 1 lost of 800 is exactly 0.125 %, which String(format:) would write as "0.12".
        let summary = diagnostics(stats: stats(packets: 799, lost: 1, packetsPerSecond: 2.5))
            .summary
        try #require(summary.count == 6)

        #expect(summary[0] == Diagnostics.Reading(label: "Packets / s", value: "3"))
        #expect(summary[1] == Diagnostics.Reading(label: "Loss", value: "0.13 %"))
        #expect(DiagnosticsWords.milliseconds(0.25, places: 1) == "0.3 ms")
        #expect(DiagnosticsWords.sampleRate(44_150) == "44.2 kHz")
    }

    @Test func aValueThatIsNotANumberReadsAsJavaWritesIt() {
        #expect(DiagnosticsWords.decimal(.nan, places: 1) == "NaN")
        #expect(DiagnosticsWords.decimal(.infinity, places: 1) == "Infinity")
    }

    @Test func theReceiversErrorIsTheLastProblem() {
        #expect(
            diagnostics(receiverError: ReceiverProblem()).recentErrors == [
                "Last problem: UDP socket error: closed"
            ])
    }

    @Test func anOutagesReasonComesBeforeTheDesksError() {
        let deskError = DeskError(message: "unknown mix", sequence: 3)

        #expect(
            diagnostics(deskError: deskError, connectionError: "Connection refused").controlError
                == "Connection refused")
        #expect(diagnostics(deskError: deskError).controlError == "unknown mix")
        #expect(diagnostics().controlError == nil)
    }

    // The words of `SettingsScreen.kt:254-349` and `SettingsText.kt:14`.
    @Test func theDiagnosticsCardUsesAndroidsWords() {
        #expect(DiagnosticsWords.title == "DIAGNOSTICS")
        #expect(DiagnosticsWords.unknownValue == "—")
        #expect(DiagnosticsWords.recentErrorsTitle == "RECENT ERRORS")
        #expect(DiagnosticsWords.noRecentErrors == "None")
        #expect(
            DiagnosticsWords.expanderText(expanded: false) == "Show all counters and recent errors")
        #expect(
            DiagnosticsWords.expanderText(expanded: true) == "Hide all counters and recent errors")
        #expect(DiagnosticsWords.expanderState(expanded: false) == "Collapsed")
        #expect(DiagnosticsWords.expanderState(expanded: true) == "Expanded")
    }
}
