import Testing

@testable import BandEarProtocol

/// The Android client's own tests for `ReorderHold`, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ReorderHoldTest.kt`: same arrivals, same
/// expected plays, same counters. This is the cross-client contract. A failure here means the two
/// phones no longer treat the same Wi-Fi the same way, whatever the rest of the suite says.
///
/// Kept apart from `ReorderHoldTests`, which pins the same rules with its own inputs plus the
/// cases that exist only in Swift (a sequence beyond `Int64.max`, the property run).
@Suite("Reorder hold: Android parity")
struct ReorderHoldParityTests {
    private func hold(_ packets: Int) -> ReorderHold<UInt64> {
        let hold = ReorderHold<UInt64>()
        hold.holdPackets = packets
        return hold
    }

    /// One arrival: offers `sequence` and answers the sequences played because of it, in order.
    @discardableResult
    private func arrive(_ hold: ReorderHold<UInt64>, _ sequence: UInt64) -> [UInt64] {
        _ = hold.offer(sequence: sequence, packet: sequence)
        return drain(hold)
    }

    @discardableResult
    private func drain(_ hold: ReorderHold<UInt64>) -> [UInt64] {
        var played: [UInt64] = []
        while let packet = hold.next() { played.append(packet) }
        return played
    }

    // MARK: With no hold the phone plays as it always did.

    @Test func withNoHoldPacketsInOrderPlayAsTheyArrive() {
        let hold = hold(0)

        #expect(arrive(hold, 0) == [0])
        #expect(arrive(hold, 1) == [1])
        #expect(arrive(hold, 2) == [2])
        #expect(hold.lost == 0)
        #expect(hold.dropped == 0)
    }

    @Test func theFirstPacketStartsTheStreamWhereverItJoins() {
        let hold = hold(0)

        // A phone that joins a stream already running has lost nothing before its first packet.
        #expect(arrive(hold, 500) == [500])
        #expect(hold.lostBefore == 0)
        #expect(arrive(hold, 501) == [501])
        #expect(hold.lost == 0)
    }

    @Test func withNoHoldAGapIsCountedOnceAndThePacketAfterItPlays() {
        let hold = hold(0)
        arrive(hold, 0)

        #expect(arrive(hold, 3) == [3])
        #expect(hold.lostBefore == 2)
        #expect(arrive(hold, 4) == [4])
        #expect(hold.lostBefore == 0)
        #expect(hold.lost == 2)
    }

    @Test func withNoHoldADuplicateIsDropped() {
        let hold = hold(0)
        arrive(hold, 0)
        arrive(hold, 1)

        #expect(hold.offer(sequence: 1, packet: 1) == false)
        #expect(arrive(hold, 2) == [2])
        #expect(hold.lost == 0)
        #expect(hold.dropped == 1)
    }

    @Test func withNoHoldALatePacketIsDroppedWithoutRewindingTheStream() {
        let hold = hold(0)
        arrive(hold, 0)
        arrive(hold, 3)

        // 1 and 2 arrive after 3 was played: too late to play, and their gap is already counted.
        #expect(arrive(hold, 1) == [])
        #expect(arrive(hold, 2) == [])

        #expect(arrive(hold, 4) == [4])
        #expect(hold.lost == 2)
        #expect(hold.dropped == 2)
        #expect(hold.recovered == 0, "a packet never played is never recovered")
    }

    @Test func aServerRestartStartsANewStreamWithoutCountingLoss() {
        let hold = hold(0)
        arrive(hold, 5_000)

        // The server restarted and its sequence began again at 0.
        #expect(arrive(hold, 0) == [0])
        #expect(arrive(hold, 1) == [1])
        #expect(hold.lost == 0)
        #expect(hold.dropped == 0)
    }

    @Test func onlyAPacketAtLeastTheRestartDistanceBehindStartsANewStream() {
        let hold = hold(0)
        arrive(hold, 1_000)
        let restart = UInt64(ReorderHold<UInt64>.streamRestartPackets)

        #expect(arrive(hold, 1_000 - restart + 1) == [])
        #expect(arrive(hold, 1_000 - restart) == [1_000 - restart])
        #expect(hold.lost == 0)
        #expect(hold.dropped == 1)
    }

    @Test func withNoHoldAnImmediateCopyFillsItsGapInOrder() {
        let hold = hold(0)
        // The server sends each block, then its copy right away. Original 2 is lost: its copy
        // arrives before 3, so it is simply played in its place, and the second copies are dropped.
        var arrivals: [UInt64] = []
        for block: UInt64 in 0...4 {
            if block != 2 { arrivals.append(block) }
            arrivals.append(block)
        }
        let played = arrivals.flatMap { arrive(hold, $0) }

        #expect(played == [0, 1, 2, 3, 4])
        #expect(hold.lost == 0)
        #expect(hold.dropped == 4)
        #expect(hold.recovered == 0, "it cannot be told from its original")
    }

    // MARK: With a hold the phone plays a few packets late, so a late copy can fill its gap.

    @Test func withAHoldPacketsInOrderPlayThatManyPacketsLate() {
        let hold = hold(2)

        #expect(arrive(hold, 0) == [])
        #expect(arrive(hold, 1) == [])
        #expect(arrive(hold, 2) == [0])
        #expect(arrive(hold, 3) == [1])
        #expect(arrive(hold, 4) == [2])
        #expect(hold.lost == 0)
    }

    @Test func aDelayedCopyFillsTheGapOfItsLostOriginalAndIsCountedAsRecovered() {
        let hold = hold(2)
        // The server sends each block, then the copy of the one two blocks before it. Original 5 is
        // lost; its copy arrives right after original 7.
        var arrivals: [UInt64] = []
        for block: UInt64 in 0...10 {
            if block != 5 { arrivals.append(block) }
            if block >= 2 { arrivals.append(block - 2) }
        }
        var played = arrivals.flatMap { arrive(hold, $0) }
        hold.release()
        played += drain(hold)

        #expect(played == Array(0...10))
        #expect(hold.lost == 0)
        #expect(hold.recovered == 1)
        #expect(hold.dropped == 8, "every other copy is a duplicate or too late")
    }

    @Test func theRecoveredPacketIsMarkedWhenItIsPlayed() {
        let hold = hold(1)
        #expect(arrive(hold, 0) == [])
        #expect(arrive(hold, 2) == [0], "0 is due, 1 may still come")

        #expect(hold.offer(sequence: 1, packet: 1) == true)
        #expect(hold.next() == 1)
        #expect(hold.lastRecovered == true)
        #expect(hold.lostBefore == 0)
        #expect(hold.next() == nil)
        #expect(arrive(hold, 3) == [2])
        #expect(hold.lastRecovered == false)
    }

    @Test func aGapNotFilledBeforeItsDeadlineIsSkippedAndCountedLost() {
        let hold = hold(2)
        for sequence: UInt64 in [0, 1, 2] { arrive(hold, sequence) }

        // 3 is lost, and its copy too: once 6 arrives it can no longer come in time.
        #expect(arrive(hold, 4) == [1, 2])
        #expect(arrive(hold, 5) == [], "3 may still come as a copy")
        #expect(arrive(hold, 6) == [4])
        #expect(hold.lostBefore == 1)
        // Its copy, arriving now, is too late to play.
        #expect(arrive(hold, 3) == [])
        #expect(hold.lost == 1)
        #expect(hold.recovered == 0)
    }

    @Test func aDuplicateOfAHeldPacketIsDropped() {
        let hold = hold(2)
        arrive(hold, 0)
        arrive(hold, 1)

        #expect(hold.offer(sequence: 1, packet: 1) == false)
        #expect(hold.offer(sequence: 0, packet: 0) == false)
        #expect(hold.dropped == 2)
        #expect(arrive(hold, 2) == [0])
    }

    @Test func aServerRestartDropsWhatTheOldStreamLeftHeld() {
        let hold = hold(2)
        arrive(hold, 5_000)
        arrive(hold, 5_001)

        // The server restarted: its new stream starts over, held as any other.
        #expect(arrive(hold, 0) == [])
        #expect(arrive(hold, 1) == [])
        #expect(arrive(hold, 2) == [0])
        #expect(hold.lostBefore == 0)
        #expect(hold.lost == 0)
    }

    @Test func aLongJumpForwardPlaysWhatWasHeldThenCountsTheGapOnce() {
        let hold = hold(2)
        arrive(hold, 0)
        arrive(hold, 1)

        #expect(arrive(hold, 1_000) == [0, 1])
        #expect(arrive(hold, 1_001) == [])
        #expect(arrive(hold, 1_002) == [1_000])
        #expect(hold.lostBefore == 998)
        #expect(hold.lost == 998)
    }

    @Test func aPacketThatArrivedFarAheadDuringABurstIsPlayedInItsPlace() {
        let hold = hold(4)
        // The server sends each block, then the copy of the one four blocks before it. Originals 6
        // and 7 are lost, so 8 arrives too far ahead of what is held to be held with it at once.
        var arrivals: [UInt64] = []
        for block: UInt64 in 0...15 {
            if block != 6 && block != 7 { arrivals.append(block) }
            if block >= 4 { arrivals.append(block - 4) }
        }
        var played: [UInt64] = []
        func drainChecked() {
            while let packet = hold.next() {
                played.append(packet)
                #expect(hold.lostBefore >= 0, "lostBefore \(hold.lostBefore)")
            }
        }
        for sequence in arrivals {
            _ = hold.offer(sequence: sequence, packet: sequence)
            drainChecked()
        }
        hold.release()
        drainChecked()

        #expect(played == Array(0...15), "every packet once, in order, 8 included")
        #expect(hold.lost == 0)
        #expect(hold.recovered == 2, "the copies of 6 and 7")

        // Nothing is left behind to rewind the stream later.
        hold.release()
        hold.holdPackets = 0
        drainChecked()
        #expect(arrive(hold, 16) == [16])
        #expect(played == Array(0...15))
        #expect(hold.lostBefore == 0)
        #expect(hold.lost == 0)
    }

    @Test func releasingPlaysEverythingHeldInOrder() {
        let hold = hold(2)
        for sequence: UInt64 in [0, 1, 2] { arrive(hold, sequence) }

        // No packet for a while: what is held is played rather than kept stale.
        hold.release()
        #expect(drain(hold) == [1, 2])
        #expect(arrive(hold, 3) == [])
        #expect(arrive(hold, 4) == [])
        #expect(arrive(hold, 5) == [3])
    }

    @Test func aShorterHoldPlaysWhatIsNowDue() {
        let hold = hold(3)
        for sequence: UInt64 in [0, 1, 2] { arrive(hold, sequence) }

        hold.holdPackets = 0
        #expect(drain(hold) == [0, 1, 2])
        #expect(arrive(hold, 3) == [3])
    }

    @Test func theHoldStaysWithinWhatThePhoneKeepsRoomFor() {
        let hold = ReorderHold<UInt64>()
        #expect(hold.holdPackets == 0)
        hold.holdPackets = 9
        #expect(hold.holdPackets == ReorderHold<UInt64>.maximumHoldPackets)
        hold.holdPackets = -1
        #expect(hold.holdPackets == 0)
    }
}
