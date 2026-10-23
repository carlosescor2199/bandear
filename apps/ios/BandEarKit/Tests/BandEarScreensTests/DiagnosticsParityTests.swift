import BandEarPlayback
import Testing

@testable import BandEarScreens

/// The Android reducer's tests for the diagnostics, ported from
/// `apps/android/app/src/test/java/com/bandear/android/ui/state/MonitorReducerTest.kt`.
///
/// Android reduces a `StatsUpdated` event into its diagnostics. Here the session holds the
/// receiver's latest metrics (``MonitorAudio/receiverStats``), so each case builds the diagnostics
/// of those metrics.
@Suite("Diagnostics: Android reducer parity")
struct DiagnosticsParityTests {
    private func stats(packets: Int64 = 990, lost: Int64 = 10) -> ReceiverStats {
        ReceiverStats(
            packets: packets,
            lost: lost,
            sampleRate: 48_000,
            packetsPerSecond: 100.0,
            interArrivalMilliseconds: 10.0,
            jitterMilliseconds: 0.4,
            discardedSamples: 2,
            packetsWithDiscardedSamples: 1,
            playbackBacklogMilliseconds: 12.5,
            outputPlaying: true)
    }

    // MonitorReducerTest.kt:1363
    @Test func statsFillTheDiagnostics() {
        let diagnostics = Diagnostics(
            stats: stats(packets: 990, lost: 10), linkReport: nil, underruns: 0, receiverError: nil,
            deskError: nil, connectionError: nil)

        #expect(diagnostics.packetsReceived == 990)
        #expect(diagnostics.packetsLost == 10)
        #expect(abs(diagnostics.lossPercent - 1.0) <= 1e-9)
        #expect(diagnostics.sampleRateHz == 48_000)
        #expect(diagnostics.packetsPerSecond == 100.0)
        #expect(diagnostics.interArrivalMilliseconds == 10.0)
        #expect(diagnostics.jitterMilliseconds == 0.4)
        #expect(diagnostics.playbackBufferMilliseconds == 12.5)
        #expect(diagnostics.discardedSamples == 2)
        #expect(diagnostics.packetsWithDiscardedSamples == 1)
    }

    // MonitorReducerTest.kt:1379
    @Test func lossIsZeroBeforeAnyPacket() {
        #expect(Diagnostics().lossPercent == 0.0)
    }
}
