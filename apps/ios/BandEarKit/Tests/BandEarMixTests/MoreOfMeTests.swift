import Testing

@testable import BandEarMix

/// The lift's rules beyond the Android parity suite: its rounding, and the pair and lock rules of
/// Android's `moreOfMeFor` (`ClientMixState.kt:77-83`), which Android only reaches through its
/// reducer tests.
@Suite("More of me")
struct MoreOfMeTests {

    /// Android halves with `(gain * 0.5).roundToInt()`, and Kotlin rounds a half toward positive
    /// infinity, so half of an odd level is its upper half. Swift's `rounded()` takes a half away
    /// from zero instead; on every level a mix can hold the two agree, which this pins too.
    @Test("halves every level a mix can hold, 0 to 100, a half rounded up")
    func roundingTable() throws {
        let levels = Array(0...100) + [0]

        let lifted = MoreOfMe.liftedLevels(levels, musicianChannels: [101], lockedChannels: [])
        try #require(lifted.count == 102)

        let kotlin = (0...100).map { ($0 + 1) / 2 }
        #expect(Array(lifted[0...100]) == kotlin)
        #expect((0...100).map { Int((Double($0) * 0.5).rounded()) } == kotlin)
        #expect([lifted[1], lifted[3], lifted[99], lifted[100]] == [1, 2, 50, 50])
        #expect(lifted[101] == 100)
    }

    /// No level in a mix is negative, but the lift takes any list, and on a negative half Swift's
    /// own rounding (-0.5 to -1) and Kotlin's (-0.5 to 0) disagree. This holds the lift to Kotlin's.
    @Test("rounds a negative half up, as Kotlin does")
    func negativeHalves() {
        #expect(
            MoreOfMe.liftedLevels([-1, -3, -4, 0], musicianChannels: [3], lockedChannels: [])
                == [0, -1, -2, 100])
    }

    @Test("lifts both sides of a pair alike and halves every other channel")
    func liftsBothSides() {
        #expect(
            MoreOfMe.liftedLevels([80, 60, 40, 20], musicianChannels: [1, 2], lockedChannels: [])
                == [40, 100, 100, 10])
    }

    @Test("leaves a locked channel as stored, the musician's own included")
    func lockedChannelsStayAsStored() {
        #expect(
            MoreOfMe.liftedLevels([80, 60, 40, 20], musicianChannels: [1, 2], lockedChannels: [0, 2])
                == [80, 100, 40, 10])
    }

    @Test("ignores a second side or a locked channel the levels do not have")
    func extrasOutsideTheLevels() {
        #expect(
            MoreOfMe.liftedLevels([80, 60], musicianChannels: [1, 2], lockedChannels: [5, -1])
                == [40, 100])
        #expect(MoreOfMe.liftedLevels([], musicianChannels: [0], lockedChannels: []) == [])
    }

    /// Android's `moreOfMeFor` lifts by the first channel and gives up when it has none in the mix,
    /// whatever the second side says.
    @Test("lifts nothing when the first channel is outside the levels")
    func firstChannelDecides() {
        #expect(
            MoreOfMe.liftedLevels([80, 60], musicianChannels: [5, 1], lockedChannels: []) == [80, 60])
    }
}
