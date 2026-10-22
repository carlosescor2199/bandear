import Testing

@testable import BandEarPlayback

/// The report a poll answered, or the test stops there. `#require` evaluates a method call inside
/// a closure, where a mutating `poll` cannot run, so the poll runs as this argument instead and the
/// failure is still reported at the caller's line.
func due(
    _ report: LinkStatsReport?, sourceLocation: SourceLocation = #_sourceLocation
) throws -> LinkStatsReport {
    try #require(report, "no report was due", sourceLocation: sourceLocation)
}

/// The Android client's own tests for the once-a-second link report, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/LinkStatsTest.kt`: when a report is due,
/// what interval it claims and which counts it carries. Same calls, same clock readings, same
/// expected reports. Time is injected, so nothing here waits.
///
/// One translation, deliberate: Android marks an extra it did not measure as `NaN`; here it is nil.
/// Android's `assertEquals` on two reports holds `NaN` equal to `NaN` (a data class compares its
/// doubles with `Double.compare`), so its whole-report assertions read here as the same report with
/// nil extras.
///
/// Kept apart from `LinkStatsAccumulatorTests`, which pins what only Swift has to answer (nil and
/// non-finite measurements, the interval clamp at both ends) and one rule Android leaves untested.
@Suite("Link stats: Android parity")
struct LinkStatsParityTests {
    private typealias Accumulator = LinkStatsAccumulator

    private var grace: Int64 { Accumulator.startupGraceMilliseconds }

    // LinkStatsTest.kt:15
    @Test func noReportIsDueBeforeTheIntervalHasPassed() {
        var stats = Accumulator(startedAtMilliseconds: 10_000)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 10_000)

        #expect(stats.isDue(nowMilliseconds: 10_999) == false)
        #expect(stats.poll(nowMilliseconds: 10_999, cumulativeUnderruns: 0) == nil)
    }

    // LinkStatsTest.kt:24
    @Test func reportsTheIntervalCountsOnceTheIntervalHasPassed() {
        var stats = Accumulator(startedAtMilliseconds: 10_000)
        for _ in 0..<3 { stats.onPacket(lostBefore: 0, nowMilliseconds: 10_000) }
        stats.onPacket(lostBefore: 2, nowMilliseconds: 10_500)
        stats.onReceiveTimeout()

        #expect(stats.isDue(nowMilliseconds: 11_000))
        #expect(
            stats.poll(nowMilliseconds: 11_000, cumulativeUnderruns: 0)
                == LinkStatsReport(
                    intervalMilliseconds: 1_000, packetsReceived: 4, packetsLost: 2, underruns: 0,
                    receiveTimeouts: 1))
    }

    // LinkStatsTest.kt:44
    @Test func noReportBeforeTheStreamStartsWithinTheStartupGrace() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onReceiveTimeout()

        // No audio yet, and the desk is probably still adding this phone as a target: a report now
        // would carry no packets and show a healthy phone as red.
        #expect(stats.isDue(nowMilliseconds: 1_000) == false)
        #expect(stats.isDue(nowMilliseconds: grace - 1) == false)
        #expect(stats.poll(nowMilliseconds: grace - 1, cumulativeUnderruns: 0) == nil)
    }

    // LinkStatsTest.kt:56
    @Test func theFirstReportComesOneIntervalAfterTheStreamStartsAndCoversOnlyTheStream() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 400)

        // The silence before the first packet was startup, not the link, so it is not reported.
        #expect(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0) == nil)
        #expect(
            stats.poll(nowMilliseconds: 1_400, cumulativeUnderruns: 0)
                == LinkStatsReport(
                    intervalMilliseconds: 1_000, packetsReceived: 1, packetsLost: 0, underruns: 0,
                    receiveTimeouts: 0))
    }

    // LinkStatsTest.kt:75
    @Test func withNoAudioTheFirstReportComesAfterTheGraceAndCoversAllOfIt() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        for _ in 0..<12 { stats.onReceiveTimeout() }

        #expect(stats.poll(nowMilliseconds: grace - 1, cumulativeUnderruns: 0) == nil)
        #expect(
            stats.poll(nowMilliseconds: grace, cumulativeUnderruns: 0)
                == LinkStatsReport(
                    intervalMilliseconds: grace, packetsReceived: 0, packetsLost: 0, underruns: 0,
                    receiveTimeouts: 0))
    }

    // LinkStatsTest.kt:93
    @Test func afterTheGraceReportsFlowEveryIntervalAndAStreamThatStartsLateKeepsTheCadence() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        _ = stats.poll(nowMilliseconds: 3_000, cumulativeUnderruns: 0)

        #expect(stats.poll(nowMilliseconds: 3_999, cumulativeUnderruns: 0) == nil)
        #expect(stats.poll(nowMilliseconds: 4_000, cumulativeUnderruns: 0)?.packetsReceived == 0)

        // The audio finally arrives: reports are already flowing, so the interval runs on.
        stats.onPacket(lostBefore: 0, nowMilliseconds: 4_500)
        let report = stats.poll(nowMilliseconds: 5_000, cumulativeUnderruns: 0)
        #expect(report?.intervalMilliseconds == 1_000)
        #expect(report?.packetsReceived == 1)
    }

    // LinkStatsTest.kt:108
    @Test func aTimeoutBeforeTheFirstPacketIsNotCounted() {
        var stats = Accumulator(startedAtMilliseconds: 0)

        // The receiver starts before the desk knows where to send audio.
        stats.onReceiveTimeout()
        stats.onReceiveTimeout()

        #expect(stats.poll(nowMilliseconds: grace, cumulativeUnderruns: 0)?.receiveTimeouts == 0)
    }

    // LinkStatsTest.kt:122
    @Test func aTimeoutAfterTheFirstPacketIsCounted() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onReceiveTimeout()
        stats.onPacket(lostBefore: 0, nowMilliseconds: 200)

        stats.onReceiveTimeout()

        let report = stats.poll(nowMilliseconds: 1_200, cumulativeUnderruns: 0)
        #expect(report?.receiveTimeouts == 1)
        #expect(report?.packetsReceived == 1)
    }

    // LinkStatsTest.kt:135
    @Test func theStreamStaysStartedAcrossReports() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)
        _ = stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0)

        // Audio stopped after the stream started: every wait now says something about the link.
        stats.onReceiveTimeout()

        #expect(stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0)?.receiveTimeouts == 1)
    }

    // LinkStatsTest.kt:147
    @Test func aLatePollReportsTheRealElapsedInterval() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)

        // The loop can only poll between receives, up to one receive timeout late.
        let report = stats.poll(nowMilliseconds: 1_250, cumulativeUnderruns: 0)
        #expect(report?.intervalMilliseconds == 1_250)
    }

    // LinkStatsTest.kt:156
    @Test func theCountsStartAgainAfterAReport() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        for _ in 0..<100 { stats.onPacket(lostBefore: 0, nowMilliseconds: 0) }
        stats.onPacket(lostBefore: 5, nowMilliseconds: 500)
        stats.onReceiveTimeout()
        _ = stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0)

        stats.onPacket(lostBefore: 0, nowMilliseconds: 1_500)

        // The next interval runs from the last report, not from the start.
        #expect(stats.poll(nowMilliseconds: 1_999, cumulativeUnderruns: 0) == nil)
        #expect(
            stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0)
                == LinkStatsReport(
                    intervalMilliseconds: 1_000, packetsReceived: 1, packetsLost: 0, underruns: 0,
                    receiveTimeouts: 0))
    }

    // LinkStatsTest.kt:180
    @Test func underrunsAreTheDifferenceOfTheTracksCumulativeCount() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)

        #expect(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 3)?.underruns == 3)
        #expect(stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 3)?.underruns == 0)
        #expect(stats.poll(nowMilliseconds: 3_000, cumulativeUnderruns: 7)?.underruns == 4)
    }

    // LinkStatsTest.kt:190
    @Test func recoveredIsTheDifferenceOfTheTrackersCumulativeCount() {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)

        #expect(
            stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0, cumulativeRecovered: 3)?
                .recovered == 3)
        #expect(
            stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0, cumulativeRecovered: 3)?
                .recovered == 0)
        #expect(
            stats.poll(nowMilliseconds: 3_000, cumulativeUnderruns: 0, cumulativeRecovered: 5)?
                .recovered == 2)
        // A count that went down belongs to a new tracker, which starts from zero.
        #expect(
            stats.poll(nowMilliseconds: 4_000, cumulativeUnderruns: 0, cumulativeRecovered: 1)?
                .recovered == 1)
    }

    // LinkStatsTest.kt:202
    @Test func theJitterIsTheMeanArrivalDeviationOverTheInterval() throws {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)
        stats.onArrivalDeviation(milliseconds: 1.0)
        stats.onArrivalDeviation(milliseconds: -3.0)
        stats.onArrivalDeviation(milliseconds: 2.0)

        // An early packet varies the arrival as much as a late one.
        let report = try due(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0))
        let jitter = try #require(report.jitterMilliseconds)
        #expect(abs(jitter - 2.0) <= 1e-9)
    }

    // LinkStatsTest.kt:214
    @Test func theJitterStartsAgainAfterAReportAndIsNotMeasuredWithoutArrivals() throws {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)
        stats.onArrivalDeviation(milliseconds: 8.0)
        _ = stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0)

        // No two packets arrived in this interval: there is no spacing to measure.
        let quiet = try due(stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0))
        #expect(quiet.jitterMilliseconds == nil)

        stats.onArrivalDeviation(milliseconds: 0.5)
        let next = try due(stats.poll(nowMilliseconds: 3_000, cumulativeUnderruns: 0))
        let jitter = try #require(next.jitterMilliseconds)
        #expect(abs(jitter - 0.5) <= 1e-9)
    }

    // LinkStatsTest.kt:228
    @Test func theCushionAndTheBacklogAreTheValuesAtTheIntervalsEnd() throws {
        var stats = Accumulator(startedAtMilliseconds: 0)
        stats.onPacket(lostBefore: 0, nowMilliseconds: 0)

        let report = try due(
            stats.poll(
                nowMilliseconds: 1_000, cumulativeUnderruns: 0, cushionMilliseconds: 10.0,
                backlogMilliseconds: 37.5))

        #expect(report.cushionMilliseconds == 10.0)
        #expect(report.backlogMilliseconds == 37.5)
    }

    // LinkStatsTest.kt:239
    @Test func aReportWithoutTheExtrasMarksThemNotMeasured() throws {
        var stats = Accumulator(startedAtMilliseconds: 0)

        let report = try due(stats.poll(nowMilliseconds: grace, cumulativeUnderruns: 0))

        #expect(report.jitterMilliseconds == nil)
        #expect(report.cushionMilliseconds == nil)
        #expect(report.backlogMilliseconds == nil)
    }

    // LinkStatsTest.kt:250
    @Test func theIntervalIsClampedToWhatTheServerAccepts() {
        var stats = Accumulator(startedAtMilliseconds: 0)

        // A phone that slept for two minutes still sends an interval the desk accepts.
        #expect(
            stats.poll(nowMilliseconds: 120_000, cumulativeUnderruns: 0)?.intervalMilliseconds
                == Accumulator.maximumIntervalMilliseconds)
    }
}
