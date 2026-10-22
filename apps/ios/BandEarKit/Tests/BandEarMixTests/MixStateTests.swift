import BandEarProtocol
import Testing

@testable import BandEarMix

/// The derived views of a mix that the Android suite only reaches through its reducer, pinned
/// directly here.
@Suite("Mix state")
struct MixStateTests {

    @Test("pans even channels left and odd ones right until told otherwise")
    func defaultPan() {
        #expect([0, 1, 2, 31].map(ChannelLayout.defaultPan(channel:)) == [0, 100, 0, 100])
        #expect(ChannelLayout.defaultName(channel: 0) == "CH 1")
    }

    /// Android matches a strip by its first channel only, so an edit reported for a pair's second
    /// side writes that side alone. The screens only ever report the first.
    @Test("writes both sides for a pair's first channel and one side otherwise")
    func stripSides() {
        let mix = firstConfig(channels: 5, details: band)

        #expect(mix.stripSides(1) == [1, 2])
        #expect(mix.stripSides(2) == [2])
        #expect(mix.stripSides(0) == [0])
        #expect(mix.stripSides(3) == [3])
        #expect(mix.stripSides(99) == [99])
    }

    @Test("reads a pair's pan as the mean of its two, and a missing pan as the default")
    func stripPans() {
        var mix = firstConfig(channels: 5, details: band)
        mix.pans = [30, 10, 50]

        #expect(mix.strips.map(\.pan) == [30, 30, 0])
    }

    @Test("reads each strip's level, mute and solo from its first channel")
    func stripValues() {
        var mix = firstConfig(channels: 5, details: band)
        mix.levels = [10, 20, 30, 40, 50]
        mix.mutes = [false, true, false, false, true]
        mix.solos = [true, false, false, false, false]

        #expect(mix.strips.map(\.level) == [10, 20, 50])
        #expect(mix.strips.map(\.isMuted) == [false, true, true])
        #expect(mix.strips.map(\.isSoloed) == [true, false, false])
    }

    @Test("tags a strip with the first group that lists its channel")
    func groupTags() {
        let mix = firstConfig(channels: 3, groups: [group("A", [2]), group("B", [0, 2])])

        #expect(
            mix.strips.map(\.group) == [
                GroupTag(index: 1, name: "B"), nil, GroupTag(index: 0, name: "A"),
            ])
    }

    @Test("reads a group with no level yet as neutral")
    func groupLevelFallback() {
        var mix = firstConfig(channels: 2, groups: [group("A", [0]), group("B", [1])])
        mix.groupLevels = [40]

        #expect(mix.groupFaders.map(\.level) == [40, 100])
    }

    @Test("offers no channel to call one's own before the desk announced any")
    func noChoicesBeforeConfig() {
        var mix = MixState()
        mix.levels = [100, 100]

        #expect(mix.musicianChoices.isEmpty)
    }

    @Test("names a channel past the details as untouched, and shows no kind without details")
    func kindsPastDetails() {
        #expect(firstConfig(channels: 3, details: [vox]).strips.map(\.kind) == [.voice, .other, .other])
        #expect(firstConfig(channels: 2).strips.map(\.kind) == [nil, nil])
    }

    @Test("locks a pair as one strip when either side is locked")
    func pairLocks() {
        var mix = firstConfig(channels: 5, details: band)
        mix.engineer = EngineerControls(lockedChannels: [2])

        #expect(mix.isStripLocked(1))
        #expect(mix.isStripLocked(2))
        #expect(!mix.isStripLocked(0))
        #expect(mix.lockedSides == [1, 2])
        #expect(mix.strips.map(\.isLocked) == [false, true, false])
    }

    @Test("marks every strip and group fader while the mix follows someone")
    func following() {
        var mix = firstConfig(channels: 2, groups: [group("A", [0])])
        mix.engineer = EngineerControls(following: "Ana")

        #expect(mix.following == "Ana")
        #expect(mix.strips.map(\.following) == ["Ana", "Ana"])
        #expect(mix.groupFaders.map(\.following) == ["Ana"])
    }

    @Test("caps what the musician hears at the lower of their cap and the engineer's limit")
    func effectiveCap() {
        var mix = MixState()
        mix.levelCap = 80
        mix.engineer = EngineerControls(limitPercent: 60)
        #expect(mix.effectiveCap == 60)

        mix.levelCap = 90
        mix.engineer = EngineerControls()
        #expect(mix.effectiveCap == 90)
    }
}
