import Testing

@testable import BandEarPlayback

/// The arrival spacing and jitter of the Android client's receive loop (`PmonPlayer.kt:222-233`),
/// which has no test of its own there. Every expected value below is worked out by hand from that
/// loop, so the order of its steps, its seed and its 0.1 weight are each pinned by a number.
@Suite("Arrival timing")
struct ArrivalTimingTests {
    /// Close to the hand-worked value: a tenth of a decimal is not exact in binary.
    private func expectMilliseconds(
        _ actual: Double?, _ expected: Double, sourceLocation: SourceLocation = #_sourceLocation
    ) throws {
        let value = try #require(actual, "not measured", sourceLocation: sourceLocation)
        #expect(
            abs(value - expected) <= 1e-9, "\(value) is not \(expected)",
            sourceLocation: sourceLocation)
    }

    @Test("measures nothing from the first packet: there is no interval yet")
    func firstPacket() {
        var timing = ArrivalTiming()

        #expect(timing.onArrival(nowNanoseconds: 1_000_000_000) == nil)
        #expect(timing.interArrivalMilliseconds == nil)
        #expect(timing.jitterMilliseconds == nil)
    }

    /// Unseeded, the spacing would start from 0 and move a tenth of the way to 10 ms: a spacing of
    /// 1 ms, a deviation of 9 ms and a jitter of 0.9 ms for a perfectly even stream.
    @Test("seeds the spacing with the first interval, so it deviates by nothing")
    func seed() {
        var timing = ArrivalTiming()
        _ = timing.onArrival(nowNanoseconds: 1_000_000_000)

        #expect(timing.onArrival(nowNanoseconds: 1_010_000_000) == 0)
        #expect(timing.interArrivalMilliseconds == 10)
        #expect(timing.jitterMilliseconds == 0)
    }

    /// Intervals of 10, 4 and 20 ms:
    /// - 4 ms: spacing 10 + 0.1 × (4 − 10) = 9.4, deviation 4 − 9.4 = −5.4, jitter 0.1 × 5.4 = 0.54.
    ///   Taken before the spacing moved, the deviation would be −6 and the jitter 0.6.
    /// - 20 ms: spacing 9.4 + 0.1 × (20 − 9.4) = 10.46, deviation 20 − 10.46 = 9.54,
    ///   jitter 0.54 + 0.1 × (9.54 − 0.54) = 1.44.
    @Test("moves the spacing a tenth of the way, then takes the deviation from it")
    func orderAndWeight() throws {
        var timing = ArrivalTiming()
        _ = timing.onArrival(nowNanoseconds: 0)
        _ = timing.onArrival(nowNanoseconds: 10_000_000)

        try expectMilliseconds(timing.onArrival(nowNanoseconds: 14_000_000), -5.4)
        try expectMilliseconds(timing.interArrivalMilliseconds, 9.4)
        try expectMilliseconds(timing.jitterMilliseconds, 0.54)

        try expectMilliseconds(timing.onArrival(nowNanoseconds: 34_000_000), 9.54)
        try expectMilliseconds(timing.interArrivalMilliseconds, 10.46)
        try expectMilliseconds(timing.jitterMilliseconds, 1.44)
    }

    /// 256-frame packets at 48 kHz are 5.333 ms apart. Read in whole milliseconds they would arrive
    /// 5, 5 and 6 ms apart, and that rounding alone would read as about 0.44 ms of jitter.
    @Test("keeps a sub-millisecond spacing: an even stream has no jitter")
    func subMillisecondSpacing() throws {
        var timing = ArrivalTiming()
        let start: UInt64 = 7_000_000_000
        #expect(timing.onArrival(nowNanoseconds: start) == nil)

        for packet in 1...20 {
            let deviation = timing.onArrival(nowNanoseconds: start + UInt64(packet) * 5_333_333)
            #expect(deviation == 0, "packet \(packet)")
        }
        try expectMilliseconds(timing.interArrivalMilliseconds, 5.333_333)
        #expect(timing.jitterMilliseconds == 0)
    }

    /// Android marks "no spacing yet" with a spacing of exactly 0.0, so a first interval of 0 (two
    /// readings in the same nanosecond) would seed it again on the next packet. Here the seed is the
    /// first interval whatever it is: after 0 then 10 ms the spacing is 0.1 × 10 = 1, the deviation
    /// 9 and the jitter 0.9, where re-seeding would give 10, 0 and 0.
    @Test("seeds the spacing once, even with a first interval of 0")
    func zeroFirstInterval() throws {
        var timing = ArrivalTiming()
        _ = timing.onArrival(nowNanoseconds: 5_000_000)

        #expect(timing.onArrival(nowNanoseconds: 5_000_000) == 0)
        #expect(timing.interArrivalMilliseconds == 0)
        #expect(timing.jitterMilliseconds == 0)

        try expectMilliseconds(timing.onArrival(nowNanoseconds: 15_000_000), 9)
        try expectMilliseconds(timing.interArrivalMilliseconds, 1)
        try expectMilliseconds(timing.jitterMilliseconds, 0.9)
    }

    /// The difference is signed, as Android's `Long` one is: a reading 5 ms behind the previous one
    /// is an interval of −5 ms, where an unsigned subtraction would trap on the receive thread.
    @Test("reads a clock that went backwards as a negative interval")
    func backwardsReading() throws {
        var timing = ArrivalTiming()
        _ = timing.onArrival(nowNanoseconds: 20_000_000)

        try expectMilliseconds(timing.onArrival(nowNanoseconds: 15_000_000), 0)
        try expectMilliseconds(timing.interArrivalMilliseconds, -5)
        try expectMilliseconds(timing.jitterMilliseconds, 0)
    }
}
