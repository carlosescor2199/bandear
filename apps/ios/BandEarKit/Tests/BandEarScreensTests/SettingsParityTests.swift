import BandEarDiscovery
import Testing

@testable import BandEarScreens

/// The Android client's own tests for the Settings text, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/screens/SettingsTextTest.kt`: the
/// diagnostics readings, the recent errors, the link summary and the server card.
@Suite("Settings text: Android parity")
struct SettingsParityTests {
    private let busy = Diagnostics(
        packetsReceived: 120_000,
        packetsLost: 24,
        packetsPerSecond: 99.6,
        interArrivalMilliseconds: 10.04,
        jitterMilliseconds: 1.43,
        playbackBufferMilliseconds: 34.4,
        sampleRateHz: 48_000,
        discardedSamples: 96,
        packetsWithDiscardedSamples: 2,
        underruns: 1,
        linkHealth: .warning)

    private func reading(_ label: String, _ value: String) -> Diagnostics.Reading {
        Diagnostics.Reading(label: label, value: value)
    }

    // SettingsTextTest.kt:26
    @Test func theCompactDiagnosticsShowSixReadings() {
        #expect(
            busy.summary == [
                reading("Packets / s", "100"),
                reading("Loss", "0.02 %"),
                reading("Underruns", "1"),
                reading("Jitter", "1.4 ms"),
                reading("Playback buffer", "34 ms"),
                reading("Sample rate", "48 kHz"),
            ])
    }

    // SettingsTextTest.kt:41
    @Test func theReadingsOfAFreshSessionAreZeroAndTheRateIsNotKnownYet() {
        #expect(
            Diagnostics().summary == [
                reading("Packets / s", "0"),
                reading("Loss", "0.00 %"),
                reading("Underruns", "0"),
                reading("Jitter", "0.0 ms"),
                reading("Playback buffer", "0 ms"),
                reading("Sample rate", "—"),
            ])
    }

    // SettingsTextTest.kt:56
    @Test func aSampleRateThatIsNotWholeKilohertzKeepsOneDecimal() {
        #expect(DiagnosticsWords.sampleRate(44_100) == "44.1 kHz")
        #expect(DiagnosticsWords.sampleRate(96_000) == "96 kHz")
        #expect(DiagnosticsWords.sampleRate(nil) == "—")
    }

    // SettingsTextTest.kt:63
    @Test func theExpanderListsEveryCounterAndTheControlChannel() {
        #expect(
            busy.allReadings(controlStatus: "connected - mix applied on the server") == [
                reading("Control channel", "connected - mix applied on the server"),
                reading("Link health", "Warning"),
                reading("Packets received", "120000"),
                reading("Packets lost", "24"),
                reading("Loss", "0.02 %"),
                reading("Packets / s", "99.6"),
                reading("Inter-arrival", "10.0 ms"),
                reading("Jitter", "1.4 ms"),
                reading("Playback buffer", "34 ms"),
                reading("Sample rate", "48 kHz"),
                reading("Underruns", "1"),
                reading("Discarded samples", "96"),
                reading("Packets with discarded samples", "2"),
            ])
    }

    // SettingsTextTest.kt:85
    @Test func recentErrorsNameWhereEachProblemCameFrom() {
        let errors = Diagnostics(lastError: "Socket closed", controlError: "Handshake refused")
        #expect(
            errors.recentErrors == [
                "Last problem: Socket closed", "Control channel: Handshake refused",
            ])
        #expect(Diagnostics().recentErrors == [])
    }

    // SettingsTextTest.kt:92
    @Test func theLinkSummaryNamesTheLevelInWords() {
        #expect(LinkHealth.good.summaryText == "Link good")
        #expect(LinkHealth.unknown.summaryText == "Link unknown")
    }

    // SettingsTextTest.kt:98
    @Test func theServerCardSaysWhereThePhoneIsConnected() {
        let connected = ConnectionStatus(
            connection: .connected,
            sessionRunning: true,
            desk: .manual(host: "192.168.1.34", controlPort: 50_001))
        #expect(SettingsWords.serverCardTitle(connected) == "BandEar server")
        #expect(SettingsWords.serverCardDetail(connected) == "192.168.1.34:50001 · connected")
    }

    // SettingsTextTest.kt:109
    @Test func theServerCardWithoutASessionPointsToTheForm() {
        let idle = ConnectionStatus()
        #expect(SettingsWords.serverCardTitle(idle) == "Not connected")
        #expect(
            SettingsWords.serverCardDetail(idle) == "Pick a server, or enter its address below.")
    }
}
