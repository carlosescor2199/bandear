import Foundation
import Testing

@testable import BandEarProtocol

/// Each packet carries its own sequence, so what is played reads as plainly as the sequence order.
private typealias Hold = ReorderHold<UInt64>

/// Everything due now, in the order it plays.
@discardableResult
private func drain(_ hold: Hold) -> [UInt64] {
    var played: [UInt64] = []
    while let packet = hold.next() { played.append(packet) }
    return played
}

/// Offers `sequence` the way the receive loop does, then plays everything that became due.
@discardableResult
private func offerAndDrain(_ hold: Hold, _ sequence: UInt64) -> [UInt64] {
    _ = hold.offer(sequence: sequence, packet: sequence)
    return drain(hold)
}

/// SplitMix64: a seeded generator, so a failing property run reproduces from its seed alone.
private struct SplitMix64: RandomNumberGenerator {
    private var state: UInt64

    init(seed: UInt64) { state = seed }

    mutating func next() -> UInt64 {
        state &+= 0x9E37_79B9_7F4A_7C15
        var mixed = state
        mixed = (mixed ^ (mixed >> 30)) &* 0xBF58_476D_1CE4_E5B9
        mixed = (mixed ^ (mixed >> 27)) &* 0x94D0_49BB_1331_11EB
        return mixed ^ (mixed >> 31)
    }
}

/// Behaviour ported from `apps/android/app/src/main/java/com/bandear/android/ReorderHold.kt`. Every
/// expectation here was traced by hand through the Kotlin first, so the two clients agree on the
/// same input rather than this one agreeing with a reading of the comments.
@Suite("Reorder hold")
struct ReorderHoldTests {

    @Test("keeps the bounds the Android client uses")
    func bounds() {
        #expect(Hold.streamRestartPackets == 256)
        #expect(Hold.maximumHoldPackets == 4)
    }

    @Test("holds the hold to zero through four packets", arguments: [(-1, 0), (0, 0), (3, 3), (4, 4), (9, 4)])
    func clampsHoldPackets(requested: Int, kept: Int) {
        let hold = Hold()
        hold.holdPackets = requested

        #expect(hold.holdPackets == kept)
    }

    // MARK: No hold, as the phone always played

    @Test("starts the stream at the first packet, wherever it joins")
    func joinsAnywhere() {
        #expect(offerAndDrain(Hold(), 1_000) == [1_000])
    }

    @Test("plays packets in order as they arrive")
    func playsInOrderAtOnce() {
        let hold = Hold()

        #expect(offerAndDrain(hold, 10) == [10])
        #expect(offerAndDrain(hold, 11) == [11])
        #expect(offerAndDrain(hold, 12) == [12])
        #expect(hold.lost == 0)
        #expect(hold.recovered == 0)
    }

    @Test("counts a gap once, against the packet played after it")
    func countsGapOnce() {
        let hold = Hold()
        offerAndDrain(hold, 10)

        #expect(offerAndDrain(hold, 12) == [12])
        #expect(hold.lostBefore == 1)
        #expect(hold.lost == 1)

        #expect(offerAndDrain(hold, 13) == [13])
        #expect(hold.lostBefore == 0)
        #expect(hold.lost == 1)
    }

    @Test("drops a packet that arrives after the stream has passed it")
    func dropsLatePacket() {
        let hold = Hold()
        offerAndDrain(hold, 10)
        offerAndDrain(hold, 12)

        #expect(hold.offer(sequence: 11, packet: 11) == false)
        #expect(drain(hold).isEmpty)
        #expect(hold.dropped == 1)
    }

    @Test("drops a second copy of a packet already played")
    func dropsDuplicateOfPlayed() {
        let hold = Hold()
        offerAndDrain(hold, 10)

        #expect(hold.offer(sequence: 10, packet: 10) == false)
        #expect(hold.dropped == 1)
    }

    // MARK: Holding back for a delayed copy

    @Test("holds each packet until the newest is the hold ahead of it")
    func holdsUntilDue() {
        let hold = Hold()
        hold.holdPackets = 2

        #expect(offerAndDrain(hold, 10) == [])
        #expect(offerAndDrain(hold, 11) == [])
        #expect(offerAndDrain(hold, 12) == [10])
        #expect(offerAndDrain(hold, 13) == [11])
    }

    @Test("plays a late copy in its lost original's place and counts it recovered")
    func recoversLateCopy() {
        let hold = Hold()
        hold.holdPackets = 2

        #expect(offerAndDrain(hold, 10) == [])
        #expect(offerAndDrain(hold, 12) == [10])
        #expect(offerAndDrain(hold, 11) == [])
        #expect(offerAndDrain(hold, 13) == [11])
        #expect(hold.lastRecovered == true)
        #expect(offerAndDrain(hold, 14) == [12])
        #expect(hold.lastRecovered == false)

        #expect(hold.recovered == 1)
        #expect(hold.lost == 0)
    }

    @Test("gives a missing packet up once its copy can no longer come in time")
    func givesUpOnMissingPacket() {
        let hold = Hold()
        hold.holdPackets = 2

        #expect(offerAndDrain(hold, 10) == [])
        #expect(offerAndDrain(hold, 12) == [10])
        // Newest is two ahead of the gap at 11: its copy could still come.
        #expect(offerAndDrain(hold, 13) == [])
        // Newest is three ahead: no copy sent two late can arrive in time now.
        #expect(offerAndDrain(hold, 14) == [12])

        #expect(hold.lostBefore == 1)
        #expect(hold.lost == 1)
    }

    @Test("drops a second copy of a packet it is still holding")
    func dropsDuplicateOfHeld() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 10)
        offerAndDrain(hold, 11)

        #expect(hold.offer(sequence: 11, packet: 11) == false)
        #expect(hold.dropped == 1)
    }

    // MARK: Release

    @Test("plays everything held when nothing came for a while")
    func releasePlaysEverythingHeld() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 10)
        offerAndDrain(hold, 11)

        hold.release()

        #expect(drain(hold) == [10, 11])
    }

    @Test("goes back to holding once what was released has played")
    func releaseLastsOneDrain() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 10)
        hold.release()
        drain(hold)

        #expect(offerAndDrain(hold, 11) == [])
    }

    @Test("skips straight to the next packet held when released over a gap")
    func releaseSkipsGap() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 10)
        #expect(offerAndDrain(hold, 12) == [10])

        hold.release()

        #expect(drain(hold) == [12])
        #expect(hold.lost == 1)
    }

    // MARK: A server that restarted

    @Test("starts a new stream when a packet is the restart distance behind, and counts no loss")
    func restartsStream() {
        let hold = Hold()
        offerAndDrain(hold, 1_000)
        offerAndDrain(hold, 1_001)

        #expect(offerAndDrain(hold, 5) == [5])
        #expect(hold.lost == 0)
        #expect(hold.lostBefore == 0)
    }

    @Test("lets go of what the old stream left held when the server restarts")
    func restartForgetsHeldPackets() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 1_000)
        offerAndDrain(hold, 1_001)

        #expect(offerAndDrain(hold, 5) == [])
        #expect(offerAndDrain(hold, 6) == [])
        #expect(offerAndDrain(hold, 7) == [5])
        #expect(hold.dropped == 0)
        #expect(hold.lost == 0)
    }

    @Test("treats a packet one short of the restart distance as merely late")
    func restartBoundary() {
        let hold = Hold()
        offerAndDrain(hold, 1_000)

        // 255 behind: late, dropped.
        #expect(hold.offer(sequence: 745, packet: 745) == false)
        // 256 behind: the server restarted.
        #expect(offerAndDrain(hold, 744) == [744])
    }

    // MARK: A packet far ahead

    @Test("plays a packet far ahead once nothing is held, counting the gap before it")
    func playsFarPacket() {
        let hold = Hold()
        offerAndDrain(hold, 10)

        #expect(offerAndDrain(hold, 20) == [20])
        #expect(hold.lostBefore == 9)
        #expect(hold.lost == 9)
    }

    @Test("plays what it holds before a packet that arrived far ahead")
    func farPacketWaitsForHeld() {
        let hold = Hold()
        hold.holdPackets = 2
        offerAndDrain(hold, 10)
        offerAndDrain(hold, 11)

        #expect(offerAndDrain(hold, 20) == [10, 11])
        #expect(offerAndDrain(hold, 21) == [])
        #expect(offerAndDrain(hold, 22) == [20])
        #expect(hold.lostBefore == 8)
    }

    @Test("gives up the first of two far jumps made before it was settled")
    func secondFarJumpDropsFirst() {
        let hold = Hold()
        hold.holdPackets = 2
        _ = hold.offer(sequence: 10, packet: 10)
        _ = hold.offer(sequence: 20, packet: 20)
        _ = hold.offer(sequence: 30, packet: 30)

        #expect(hold.dropped == 1)
        #expect(drain(hold) == [10])
    }

    @Test("drops a second copy of the packet waiting far ahead")
    func dropsDuplicateOfWaiting() {
        let hold = Hold()
        offerAndDrain(hold, 10)
        _ = hold.offer(sequence: 20, packet: 20)

        #expect(hold.offer(sequence: 20, packet: 20) == false)
        #expect(hold.dropped == 1)
    }

    @Test("drops a packet behind the far one and too far from what is held to hold")
    func dropsPacketBetweenHeldAndFar() {
        let hold = Hold()
        offerAndDrain(hold, 10)
        _ = hold.offer(sequence: 30, packet: 30)

        #expect(hold.offer(sequence: 25, packet: 25) == false)
        #expect(hold.dropped == 1)
    }

    // MARK: Beyond the Android client

    /// Kotlin reads the eight sequence bytes as a signed `Long` and its arithmetic wraps. Swift's
    /// traps, so a corrupt packet with the top bit set would crash the receive loop. No real stream
    /// reaches 2^63 blocks, so refusing such a sequence changes nothing a desk can produce.
    @Test(
        "refuses a sequence too large to be a real block count, without starting the stream",
        arguments: [UInt64(Int64.max) + 1, .max])
    func refusesOutOfRangeSequence(sequence: UInt64) {
        let hold = Hold()

        #expect(hold.offer(sequence: sequence, packet: sequence) == false)
        #expect(hold.dropped == 1)
        #expect(offerAndDrain(hold, 10) == [10])
    }

    @Test("does not trap on a garbage sequence arriving mid-stream")
    func survivesGarbageMidStream() {
        let hold = Hold()
        offerAndDrain(hold, 10)

        #expect(hold.offer(sequence: .max, packet: .max) == false)
        #expect(offerAndDrain(hold, 11) == [11])
    }
    // MARK: Whatever Wi-Fi does

    /// The cases above pin each rule; this pins how they combine. Every run mixes reordering,
    /// duplicates, loss, far jumps, hold changes and releases, and never steps back far enough to
    /// look like a restart, so within it the stream must only ever move forward.
    ///
    /// Order and uniqueness are largely structural (``ReorderHold`` never moves `head` back), so
    /// the failure this mostly guards against is the one that would be silent on a phone: the
    /// receive loop hanging. `next()` answering forever is caught by counting what it returns.
    /// `next()` never returning at all is caught by the watchdog in ``completes(within:_:)``,
    /// because Swift Testing's `.timeLimit` cannot do it: verified against a mutation that made
    /// `head` stop advancing, where the time limit never fired and the run had to be killed.
    @Test("never plays a packet out of order or twice, and always settles")
    func neverOutOfOrderOrTwice() {
        let seeds: Range<UInt64> = 0..<50
        let log = PropertyLog()

        let finished = completes(within: 20) {
            for seed in seeds { log.record(propertyRun(seed: seed), seed: seed) }
        }

        let runs = log.runs
        let stuck = seeds.first { runs[$0] == nil }
        #expect(finished, "next() never returned, first in the run for seed \(stuck ?? 0)")
        for (seed, run) in runs.sorted(by: { $0.key < $1.key }) {
            #expect(!run.runaway, "seed \(seed): next() kept answering after one offer")
            #expect(run.played == run.played.sorted(), "seed \(seed): played out of order")
            #expect(Set(run.played).count == run.played.count, "seed \(seed): played a packet twice")
            #expect(!run.played.isEmpty, "seed \(seed): nothing played")
        }
    }
}

// MARK: - Property run plumbing

/// What one seeded run played, and whether `next()` ran away in it.
private struct PropertyRun: Sendable {
    var played: [UInt64] = []
    var runaway = false
}

private func propertyRun(seed: UInt64) -> PropertyRun {
    var generator = SplitMix64(seed: seed)
    let hold = Hold()
    var run = PropertyRun()
    var newest: Int64 = 1_000

    for _ in 0..<500 {
        switch Int.random(in: 0..<10, using: &generator) {
        case 0:
            hold.holdPackets = Int.random(in: 0...Hold.maximumHoldPackets, using: &generator)
        case 1:
            hold.release()
        default:
            // Mostly forward, sometimes a skip (loss), sometimes a few back (reorder, copy).
            let step = Int64.random(in: -3...4, using: &generator)
            let sequence = max(newest + step, 0)
            newest = max(newest, sequence)
            _ = hold.offer(sequence: UInt64(sequence), packet: UInt64(sequence))
        }

        var drained = 0
        while let packet = hold.next() {
            run.played.append(packet)
            drained += 1
            if drained > 64 {
                run.runaway = true
                return run
            }
        }
    }
    return run
}

/// The runs a watchdog thread produced, handed back across the thread boundary.
private final class PropertyLog: @unchecked Sendable {
    private let lock = NSLock()
    private var stored: [UInt64: PropertyRun] = [:]

    func record(_ run: PropertyRun, seed: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        stored[seed] = run
    }

    var runs: [UInt64: PropertyRun] {
        lock.lock()
        defer { lock.unlock() }
        return stored
    }
}

/// Runs `body` on a thread of its own and waits at most `seconds` for it.
///
/// Swift Testing cancels cooperatively, so a synchronous loop that never yields ignores its time
/// limit and stalls the whole run. Waiting from outside turns that hang into a failed expectation;
/// the stuck thread is left behind and dies when the test process exits.
private func completes(within seconds: Double, _ body: @escaping @Sendable () -> Void) -> Bool {
    let done = DispatchSemaphore(value: 0)
    Thread {
        body()
        done.signal()
    }.start()
    return done.wait(timeout: .now() + seconds) == .success
}
