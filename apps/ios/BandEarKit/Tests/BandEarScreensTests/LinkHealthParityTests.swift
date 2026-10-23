import BandEarPlayback
import Testing

@testable import BandEarScreens

/// The Android client's own tests for the link-health levels, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/components/LinkHealthTest.kt`.
@Suite("Link health words: Android parity")
struct LinkHealthParityTests {

    // LinkHealthTest.kt:8
    @Test func goodIsGreen() {
        #expect(LinkHealth.good.tone == .green)
        #expect(LinkHealth.good.words == "Good")
    }

    // LinkHealthTest.kt:14
    @Test func warningIsAmber() {
        #expect(LinkHealth.warning.tone == .amber)
        #expect(LinkHealth.warning.words == "Warning")
    }

    // LinkHealthTest.kt:20
    @Test func badSilentAndLostAreRedButSayWhichOne() {
        #expect(LinkHealth.bad.tone == .red)
        #expect(LinkHealth.silent.tone == .red)
        #expect(LinkHealth.lost.tone == .red)
        #expect(LinkHealth.bad.words == "Bad")
        #expect(LinkHealth.silent.words == "Silent")
        #expect(LinkHealth.lost.words == "Lost")
    }

    // LinkHealthTest.kt:30
    @Test func unknownIsGrey() {
        #expect(LinkHealth.unknown.tone == .neutral)
        #expect(LinkHealth.unknown.words == "Unknown")
    }

    // LinkHealthTest.kt:36
    @Test func descriptionGivesTheWordAndItsMeaning() {
        #expect(LinkHealth.silent.spokenDescription == "Link health: Silent. No audio received.")
        #expect(LinkHealth.unknown.spokenDescription == "Link health: Unknown. No report yet.")
    }

    // LinkHealthTest.kt:42
    @Test func everyLevelHasItsOwnWordsAndMeaning() {
        let levels = LinkHealth.allCases
        #expect(Set(levels.map(\.words)).count == levels.count)
        #expect(Set(levels.map(\.meaning)).count == levels.count)
    }
}

/// The Android client's own tests for how the phone judges its own link, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/state/PhoneLinkHealthTest.kt`: with the
/// desk's thresholds (`apps/server/src/health.rs`), nothing received is silent, a timeout, 2
/// underruns a second or 2 % loss is bad, and any underrun or 0.5 % loss is a warning. Boundaries
/// land exactly, as the desk compares in integers.
@Suite("Phone link health: Android parity")
struct PhoneLinkHealthParityTests {
    private func report(
        intervalMs: Int64 = 1_000, received: Int64 = 100, lost: Int64 = 0, underruns: Int64 = 0,
        timeouts: Int64 = 0
    ) -> LinkStatsReport {
        LinkStatsReport(
            intervalMilliseconds: intervalMs, packetsReceived: received, packetsLost: lost,
            underruns: underruns, receiveTimeouts: timeouts)
    }

    private func health(_ report: LinkStatsReport) -> LinkHealth {
        LinkHealth(phoneReport: report)
    }

    // PhoneLinkHealthTest.kt:22
    @Test func aCleanIntervalIsGood() {
        #expect(health(report()) == .good)
    }

    // PhoneLinkHealthTest.kt:27
    @Test func nothingReceivedIsSilentWhateverElseHappened() {
        #expect(health(report(received: 0)) == .silent)
        #expect(health(report(received: 0, lost: 5, timeouts: 3)) == .silent)
    }

    // PhoneLinkHealthTest.kt:33
    @Test func anyReceiveTimeoutIsBad() {
        #expect(health(report(timeouts: 1)) == .bad)
    }

    // PhoneLinkHealthTest.kt:38
    @Test func oneUnderrunIsAWarningAndTwoASecondAreBad() {
        #expect(health(report(underruns: 1)) == .warning)
        #expect(health(report(underruns: 2)) == .bad)
    }

    // PhoneLinkHealthTest.kt:44
    @Test func underrunsAreJudgedPerSecondOfTheInterval() {
        #expect(health(report(intervalMs: 2_000, underruns: 3)) == .warning)
        #expect(health(report(intervalMs: 2_000, underruns: 4)) == .bad)
    }

    // PhoneLinkHealthTest.kt:50
    @Test func lossThresholdsLandExactly() {
        // 1 lost of 200 expected is exactly 0.5 %; 1 of 201 is just under.
        #expect(health(report(received: 199, lost: 1)) == .warning)
        #expect(health(report(received: 200, lost: 1)) == .good)
        // 2 lost of 100 expected is exactly 2 %; 2 of 101 is just under.
        #expect(health(report(received: 98, lost: 2)) == .bad)
        #expect(health(report(received: 99, lost: 2)) == .warning)
    }
}

/// The Android reducer's test for the link health a report sets, ported from
/// `apps/android/app/src/test/java/com/bandear/android/ui/state/MonitorReducerTest.kt`.
///
/// Here the latest report is what the session holds (``MonitorAudio/linkReport``), so each step
/// builds the diagnostics of that report. The running underrun total the same Android test checks is
/// kept by the session, not here, and is not asserted.
@Suite("Link health: Android reducer parity")
struct LinkHealthReducerParityTests {
    private func report(received: Int64 = 100, underruns: Int64 = 0) -> LinkStatsReport {
        LinkStatsReport(
            intervalMilliseconds: 1_000, packetsReceived: received, packetsLost: 0,
            underruns: underruns, receiveTimeouts: 0)
    }

    private func diagnostics(after report: LinkStatsReport) -> Diagnostics {
        Diagnostics(
            stats: nil, linkReport: report, underruns: 0, receiverError: nil, deskError: nil,
            connectionError: nil)
    }

    // MonitorReducerTest.kt:1384, the link health only.
    @Test func linkReportsSetTheLinkHealthAndAddUpUnderruns() {
        let warned = diagnostics(after: report(underruns: 1))
        let recovered = diagnostics(after: report(underruns: 0))
        let silent = diagnostics(after: report(received: 0))

        #expect(warned.linkHealth == .warning)
        #expect(recovered.linkHealth == .good)
        #expect(silent.linkHealth == .silent)
    }
}
