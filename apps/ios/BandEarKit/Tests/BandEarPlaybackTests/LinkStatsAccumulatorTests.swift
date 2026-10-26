import Testing

import BandEarProtocol
@testable import BandEarPlayback

/// What only the Swift port has to answer, beside the Android cases in `LinkStatsParityTests`:
/// - A measurement not taken is nil, never `NaN`, so a non-finite arrival deviation must not reach
///   the report as a number.
/// - The interval clamp at both ends, which Android pins only at the top.
/// - A track's underrun count that went down, a rule `LinkStats.kt:153-155` states and no Android
///   test exercises.
@Suite("Link stats accumulator")
struct LinkStatsAccumulatorTests {
    private typealias Accumulator = LinkStatsAccumulator

    /// An accumulator whose stream started at `startedAt`, with its first packet played then.
    private func streaming(from startedAt: Int64) -> Accumulator {
        var stats = Accumulator(startedAtMilliseconds: startedAt)
        stats.onPacket(lostBefore: 0, nowMilliseconds: startedAt)
        return stats
    }

    @Test("keeps the timing the Android client uses, and the desk's own interval limit")
    func timing() {
        #expect(Accumulator.intervalMilliseconds == 1_000)
        #expect(Accumulator.maximumIntervalMilliseconds == 60_000)
        #expect(
            Accumulator.maximumIntervalMilliseconds == Int64(LinkStats.maximumIntervalMilliseconds))
        #expect(Accumulator.startupGraceMilliseconds == 3_000)
    }

    // MARK: Nil, never NaN

    @Test("carries the cushion and the backlog as given, each nil on its own when not measured")
    func extrasPassThrough() throws {
        var stats = streaming(from: 0)

        let first = try due(
            stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0, cushionMilliseconds: 12.5))
        #expect(first.cushionMilliseconds == 12.5)
        #expect(first.backlogMilliseconds == nil)

        let second = try due(
            stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0, backlogMilliseconds: 40))
        #expect(second.cushionMilliseconds == nil, "the cushion is the value at the interval's end")
        #expect(second.backlogMilliseconds == 40)
    }

    @Test("measures a steady arrival as no jitter at all, which is not the same as unmeasured")
    func zeroJitterIsMeasured() throws {
        var stats = streaming(from: 0)
        stats.onArrivalDeviation(milliseconds: 0)
        stats.onArrivalDeviation(milliseconds: -0.0)

        let report = try due(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0))
        #expect(report.jitterMilliseconds == 0)
    }

    /// Android's mean turns `NaN` or infinite, and its message writer leaves it out
    /// (`ControlChannel.kt:586-591`). Here the report itself says unmeasured, for that interval
    /// only.
    @Test(
        "leaves the jitter unmeasured for an interval with a deviation that is not a number",
        arguments: [Double.nan, .infinity, -.infinity])
    func nonFiniteDeviation(deviation: Double) throws {
        var stats = streaming(from: 0)
        stats.onArrivalDeviation(milliseconds: 2)
        stats.onArrivalDeviation(milliseconds: deviation)

        let spoiled = try due(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 0))
        #expect(spoiled.jitterMilliseconds == nil)
        #expect(spoiled.packetsReceived == 1, "the counts are reported all the same")

        stats.onArrivalDeviation(milliseconds: 1.5)
        let next = try due(stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 0))
        #expect(next.jitterMilliseconds == 1.5, "the next interval measures afresh")
    }

    // MARK: The interval clamp, both ends

    @Test("reports up to the desk's limit as it was, and anything longer as the limit")
    func clampsTheLongestInterval() throws {
        var stats = streaming(from: 0)

        let atLimit = try due(stats.poll(nowMilliseconds: 60_000, cumulativeUnderruns: 0))
        #expect(atLimit.intervalMilliseconds == 60_000)

        let pastLimit = try due(stats.poll(nowMilliseconds: 120_001, cumulativeUnderruns: 0))
        #expect(pastLimit.intervalMilliseconds == 60_000)

        // The clamp shapes the report, not the clock: the next interval runs from that poll.
        #expect(stats.poll(nowMilliseconds: 121_000, cumulativeUnderruns: 0) == nil)
        let next = try due(stats.poll(nowMilliseconds: 121_001, cumulativeUnderruns: 0))
        #expect(next.intervalMilliseconds == 1_000)
    }

    /// Android also clamps from below, to 1 ms. No due report can reach that clamp, since a report
    /// is due only a whole interval on, so what is pinned is what the clamp exists for: no report
    /// ever claims less than an interval, and a clock that went backwards reports nothing.
    @Test("never reports less than one interval, nor anything for a clock that went backwards")
    func clampsTheShortestInterval() throws {
        var stats = streaming(from: 5_000)

        #expect(stats.poll(nowMilliseconds: 5_999, cumulativeUnderruns: 0) == nil)
        let first = try due(stats.poll(nowMilliseconds: 6_000, cumulativeUnderruns: 0))
        #expect(first.intervalMilliseconds == Accumulator.intervalMilliseconds)

        #expect(stats.isDue(nowMilliseconds: 5_000) == false)
        #expect(stats.poll(nowMilliseconds: 5_000, cumulativeUnderruns: 0) == nil)
        let next = try due(stats.poll(nowMilliseconds: 7_000, cumulativeUnderruns: 0))
        #expect(next.intervalMilliseconds == 1_000, "measured from the last report")
    }

    // MARK: Underruns

    @Test("reports no underruns for a new track's lower count, then counts from it")
    func underrunsOfANewTrack() {
        var stats = streaming(from: 0)

        #expect(stats.poll(nowMilliseconds: 1_000, cumulativeUnderruns: 5)?.underruns == 5)
        // A count that went down belongs to a new track, which starts from zero.
        #expect(stats.poll(nowMilliseconds: 2_000, cumulativeUnderruns: 2)?.underruns == 0)
        #expect(stats.poll(nowMilliseconds: 3_000, cumulativeUnderruns: 6)?.underruns == 4)
    }
}
