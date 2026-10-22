import Testing

@testable import BandEarPlayback

/// Each packet carries its own sequence, so what is played reads as plainly as the sequence order.
extension HoldPlayout where Packet == UInt64 {
    /// One wake-up with a packet, as the loop runs it: the hold, the offer, then every packet due.
    @discardableResult
    func arrive(_ sequence: UInt64, nowMilliseconds: Int64, holdPackets: Int) -> [UInt64] {
        wake(holdPackets: holdPackets)
        guard offer(sequence: sequence, packet: sequence) else { return [] }
        return drain(nowMilliseconds: nowMilliseconds)
    }

    /// One wake-up with no packet: the timeout, the hold, then every packet now due.
    @discardableResult
    func timeOut(nowMilliseconds: Int64, holdPackets: Int) -> [UInt64] {
        onTimeout()
        wake(holdPackets: holdPackets)
        return drain(nowMilliseconds: nowMilliseconds)
    }

    func drain(nowMilliseconds: Int64) -> [UInt64] {
        var played: [UInt64] = []
        while let packet = nextToPlay(nowMilliseconds: nowMilliseconds) { played.append(packet) }
        return played
    }
}

/// The Android client's own tests for the receive loop's wiring of the hold and the link report,
/// ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/HoldPlayoutTest.kt`: wake-up by wake-up as
/// the player drives it, a packet a delayed copy brought is played, counted in `packets_received`
/// and in `recovered`, never in `packets_lost`. Same wake-ups, same plays, same reports.
///
/// One assertion is left for the unit that sends `stats`: Android's first case also turns its
/// report into the message (`buildStatsMessage`) and compares it with the JSON the server's own
/// protocol test reads (`protocol::tests::the_stats_a_phone_sends_after_a_copy_filled_a_gap`,
/// `HoldPlayoutTest.kt:113-114`). That conversion does not exist here yet. Its input does: the case
/// pins the whole report, every field the message carries, with the extras absent.
@Suite("Hold playout: Android parity")
struct HoldPlayoutParityTests {
    private func playout() -> HoldPlayout<UInt64> {
        HoldPlayout(
            hold: ReorderHold<UInt64>(), linkStats: LinkStatsAccumulator(startedAtMilliseconds: 0))
    }

    // HoldPlayoutTest.kt:40
    @Test func aDelayedCopyIsPlayedInItsPlaceAndReportedAsReceivedAndRecoveredNotLost() throws {
        let playout = playout()
        #expect(playout.arrive(10, nowMilliseconds: 100, holdPackets: 1) == [])
        #expect(playout.arrive(11, nowMilliseconds: 110, holdPackets: 1) == [10])
        // 12 is lost on the way; 13 comes, and 12's copy after it, still in time.
        #expect(playout.arrive(13, nowMilliseconds: 120, holdPackets: 1) == [11])
        #expect(playout.arrive(12, nowMilliseconds: 130, holdPackets: 1) == [12])
        // Nothing else comes: the timeout plays what is held.
        #expect(playout.timeOut(nowMilliseconds: 1_000, holdPackets: 1) == [13])

        let report = try due(playout.poll(nowMilliseconds: 1_110, cumulativeUnderruns: 0))
        #expect(report.packetsReceived == 4)
        #expect(report.packetsLost == 0)
        #expect(report.recovered == 1)
        #expect(report.receiveTimeouts == 1)
        #expect(playout.lost == 0)

        // Everything Android's JSON comparison reads (see the suite's note).
        #expect(
            report
                == LinkStatsReport(
                    intervalMilliseconds: 1_000, packetsReceived: 4, packetsLost: 0, underruns: 0,
                    receiveTimeouts: 1, recovered: 1))
    }

    // HoldPlayoutTest.kt:66
    @Test func aCopyTooLateForTheHoldIsDroppedAndItsOriginalCountedLostOnce() throws {
        let playout = playout()
        playout.arrive(0, nowMilliseconds: 100, holdPackets: 1)
        #expect(playout.arrive(2, nowMilliseconds: 110, holdPackets: 1) == [0])
        // 3 makes 1 past its deadline: given up on, and 2 plays with the gap before it.
        #expect(playout.arrive(3, nowMilliseconds: 120, holdPackets: 1) == [2])
        #expect(playout.arrive(1, nowMilliseconds: 130, holdPackets: 1) == [])

        let report = try due(playout.poll(nowMilliseconds: 1_110, cumulativeUnderruns: 0))
        #expect(report.packetsReceived == 2)
        #expect(report.packetsLost == 1)
        #expect(report.recovered == 0, "a copy past the hold is not played, so not recovered")
        #expect(playout.lost == 1)
    }

    // HoldPlayoutTest.kt:82
    @Test func theHoldTheDeskAsksForAppliesFromTheNextWakeUp() {
        let playout = playout()
        #expect(playout.arrive(0, nowMilliseconds: 100, holdPackets: 2) == [])
        #expect(playout.arrive(1, nowMilliseconds: 110, holdPackets: 2) == [])
        #expect(playout.holdPackets == 2)
        // The desk turns the delay off: what is held is now due, and the next packet plays at once.
        #expect(playout.arrive(2, nowMilliseconds: 120, holdPackets: 0) == [0, 1, 2])
        #expect(playout.holdPackets == 0)
        #expect(playout.arrive(3, nowMilliseconds: 130, holdPackets: 0) == [3])
    }

    // HoldPlayoutTest.kt:94
    @Test func recoveredIsReportedPerIntervalAndNeverMoreThanThePacketsPlayed() throws {
        let playout = playout()
        playout.arrive(0, nowMilliseconds: 100, holdPackets: 1)
        playout.arrive(2, nowMilliseconds: 110, holdPackets: 1)
        playout.arrive(1, nowMilliseconds: 120, holdPackets: 1)
        playout.arrive(3, nowMilliseconds: 130, holdPackets: 1)
        let first = try due(playout.poll(nowMilliseconds: 1_110, cumulativeUnderruns: 0))
        #expect(first.recovered == 1)
        #expect(first.recovered <= first.packetsReceived)

        playout.arrive(4, nowMilliseconds: 1_200, holdPackets: 1)
        let second = try due(playout.poll(nowMilliseconds: 2_110, cumulativeUnderruns: 0))
        #expect(second.recovered == 0, "the difference since the last report")
        #expect(playout.poll(nowMilliseconds: 2_120, cumulativeUnderruns: 0) == nil)
        #expect(!(second.packetsLost > 0))
    }
}
