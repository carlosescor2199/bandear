import Testing

@testable import BandEarPlayback

/// What only the Swift playout has, beside the Android cases in `HoldPlayoutParityTests`.
///
/// On Android the player holds the same accumulator the playout does, and asks it directly whether
/// a report is due and what each arrival's deviation was. Here the playout owns the accumulator as
/// a value, so it carries those two calls itself; these tests pin that they reach the report. They
/// also pin the arguments Android's playout tests always leave at their defaults: the underrun
/// count and the playback extras.
@Suite("Hold playout")
struct HoldPlayoutTests {
    /// A playout with no hold, whose stream starts with packet 0 played at 100 ms.
    private func streaming() -> HoldPlayout<UInt64> {
        let playout = HoldPlayout(
            hold: ReorderHold<UInt64>(), linkStats: LinkStatsAccumulator(startedAtMilliseconds: 0))
        playout.arrive(0, nowMilliseconds: 100, holdPackets: 0)
        return playout
    }

    @Test("says through its link stats when a report is due, without taking it")
    func reportIsDue() {
        let playout = streaming()

        #expect(playout.linkStats.isDue(nowMilliseconds: 1_099) == false)
        #expect(playout.linkStats.isDue(nowMilliseconds: 1_100))
        #expect(playout.poll(nowMilliseconds: 1_100, cumulativeUnderruns: 0)?.packetsReceived == 1)
        #expect(playout.linkStats.isDue(nowMilliseconds: 1_100) == false)
    }

    @Test("hands each arrival's deviation to the report's jitter")
    func arrivalDeviation() throws {
        let playout = streaming()
        playout.onArrivalDeviation(milliseconds: 4)
        playout.onArrivalDeviation(milliseconds: -2)

        let report = try due(playout.poll(nowMilliseconds: 1_100, cumulativeUnderruns: 0))
        #expect(report.jitterMilliseconds == 3)
    }

    @Test("reports the track's underruns and the playback extras given, nil when not measured")
    func pollArguments() throws {
        let playout = streaming()

        let first = try due(
            playout.poll(
                nowMilliseconds: 1_100, cumulativeUnderruns: 2, cushionMilliseconds: 20,
                backlogMilliseconds: 33.25))
        #expect(first.underruns == 2)
        #expect(first.cushionMilliseconds == 20)
        #expect(first.backlogMilliseconds == 33.25)

        let second = try due(playout.poll(nowMilliseconds: 2_100, cumulativeUnderruns: 5))
        #expect(second.underruns == 3)
        #expect(second.cushionMilliseconds == nil)
        #expect(second.backlogMilliseconds == nil)
        #expect(second.jitterMilliseconds == nil)
    }
}
