import BandEarProtocol
import Testing

@testable import BandEarMix

/// The Android client's `MonitorReducerTest.kt` tests of the mix, ported case for case
/// (`apps/android/app/src/test/java/com/bandear/android/ui/state/`), unit by unit as the mix is
/// built. Every state here is reached through events, as Android's are.
@Suite("Mix: Android parity")
struct MixReducerParityTests {

    // MARK: T5a, the channel layout

    @Test func announcedDetailsNameTheStripsShowTheirKindsAndJoinPairs() throws {
        let strips = withBand().mix.strips
        // Required before indexing, so a short list fails this test instead of crashing the run.
        try #require(strips.count == 3)

        #expect(strips.map(\.label) == ["Vox", "Keys", "CH 5"])
        #expect(strips.map(\.kind) == [.voice, .keys, .other])
        #expect(strips.map(\.index) == [0, 1, 4])
        #expect(strips.map(\.pairedWith) == [nil, 2, nil])
        #expect(strips[1].isStereo)
        #expect(!strips[0].isStereo)
    }

    @Test func withoutDetailsEveryChannelShowsByNumberWithNoKindAsBefore() {
        let mix = withChannels(3, [group("Band", [0, 1])]).mix

        #expect(mix.channelDetails == nil)
        #expect(mix.strips.map(\.label) == ["CH 1", "CH 2", "CH 3"])
        #expect(mix.strips.map(\.kind) == [nil, nil, nil])
        #expect(mix.strips.map(\.pairedWith) == [nil, nil, nil])
        #expect(mix.groupFaders.map(\.members) == [["CH 1", "CH 2"]])
    }

    /// Android goes on to choose channels through `MusicianChannelChosen`; that half is More of me
    /// and is ported with it in T5d.
    @Test func onlyShownStripsCanBeChosenAndAPairIsChosenByItsFirstChannel() {
        let mix = withBand().mix

        #expect(mix.musicianChoices.map(\.index) == [0, 1, 4])
        #expect(
            mix.musicianChoices == [
                ChannelChoice(index: 0, label: "Vox"), ChannelChoice(index: 1, label: "Keys"),
                ChannelChoice(index: 4, label: "CH 5"),
            ])
    }

    @Test func aGroupNamesItsShownStripsAPairOnce() {
        let mix = withBand([group("Band", [0, 1, 2, 3])]).mix

        #expect(mix.groupFaders.map(\.members) == [["Vox", "Keys"]])
    }

    // MARK: T5b, edits and config

    @Test func theFirstConfigBuildsNeutralStripsWithTheCapturedPanLayout() {
        let mix = withChannels(3).mix

        #expect(mix.sourceChannels == 3)
        #expect(mix.levels == [100, 100, 100])
        #expect(mix.pans == [0, 100, 0])
        #expect(mix.mutes == [false, false, false])
        #expect(mix.solos == [false, false, false])
        #expect(mix.strips.map(\.label) == ["CH 1", "CH 2", "CH 3"])
        #expect(mix.musicianChoices.map(\.index) == [0, 1, 2])
    }

    @Test func aConfigWithANewCountKeepsTheChannelsThatStillExist() {
        let set = withChannels(3).after(
            .channelLevelChanged(channel: 0, level: 40),
            .channelPanChanged(channel: 1, pan: 30),
            .channelMuteChanged(channel: 2, muted: true))

        let grown = set.after(.configReceived(config(4)))
        let shrunk = set.after(.configReceived(config(2)))

        #expect(grown.mix.levels == [40, 100, 100, 100])
        #expect(grown.mix.pans == [0, 30, 0, 100])
        #expect(grown.mix.mutes == [false, false, true, false])
        #expect(shrunk.mix.levels == [40, 100])
        #expect(shrunk.mix.pans == [0, 30])
        #expect(shrunk.mix.sourceChannels == 2)
    }

    @Test func aRepeatedConfigLeavesTheStripsAlone() {
        let set = withChannels(2).after(
            .channelLevelChanged(channel: 1, level: 70), .channelSoloChanged(channel: 0, soloed: true))

        #expect(set.after(.configReceived(config(2))).mix == set.mix)
    }

    @Test func announcedGroupsTagTheirChannelsAndKeepTheirLevels() throws {
        let groups = [group("Drums", [0, 1]), group("Keys", [2])]
        let state = withChannels(4, groups).after(.groupLevelChanged(group: 1, level: 60))

        #expect(
            state.mix.strips.map(\.group) == [
                GroupTag(index: 0, name: "Drums"), GroupTag(index: 0, name: "Drums"),
                GroupTag(index: 1, name: "Keys"), nil,
            ])
        #expect(
            state.mix.groupFaders == [
                GroupFader(
                    index: 0, name: "Drums", channels: [0, 1], level: 100, members: ["CH 1", "CH 2"],
                    following: nil),
                GroupFader(
                    index: 1, name: "Keys", channels: [2], level: 60, members: ["CH 3"], following: nil),
            ])

        let reannounced = state.after(.configReceived(config(4, groups + [group("Vox", [3])])))

        #expect(reannounced.mix.groupLevels == [100, 60, 100])
        try #require(reannounced.mix.strips.count == 4)
        #expect(reannounced.mix.strips[3].group == GroupTag(index: 2, name: "Vox"))
    }

    @Test func aPairsLevelMuteAndSoloWriteBothChannels() throws {
        let state = withBand().after(
            .channelLevelChanged(channel: 1, level: 40), .channelMuteChanged(channel: 1, muted: true),
            .channelSoloChanged(channel: 1, soloed: true))

        #expect(state.mix.levels == [100, 40, 40, 100, 100])
        #expect(state.mix.mutes == [false, true, true, false, false])
        #expect(state.mix.solos == [false, true, true, false, false])
        try #require(state.mix.strips.count == 3)
        let pair = state.mix.strips[1]
        #expect(pair.level == 40)
        #expect(pair.isMuted)
        #expect(pair.isSoloed)
    }

    @Test func aSingleStripsEditWritesOnlyItsChannel() {
        let state = withBand().after(
            .channelLevelChanged(channel: 0, level: 30), .channelPanChanged(channel: 4, pan: 70),
            .channelMuteChanged(channel: 4, muted: true))

        #expect(state.mix.levels == [30, 100, 100, 100, 100])
        #expect(state.mix.pans == [0, 100, 0, 100, 70])
        #expect(state.mix.mutes == [false, false, false, false, true])
    }

    @Test func aPairsPanIsABalanceWrittenAsBothPansAndReadBackAsTheirMean() throws {
        let left = withBand().after(.channelPanChanged(channel: 1, pan: 25))
        let right = left.after(.channelPanChanged(channel: 1, pan: 80))

        #expect(left.mix.pans == [0, 0, 50, 100, 0])
        #expect(right.mix.pans == [0, 60, 100, 100, 0])
        try #require(left.mix.strips.count == 3 && right.mix.strips.count == 3)
        #expect(left.mix.strips[1].pan == 25)
        #expect(right.mix.strips[1].pan == 80)
        // Pans set elsewhere read as their mean.
        var elsewhere = withBand().mix
        elsewhere.pans = [0, 10, 50, 100, 0]
        #expect(elsewhere.strips[1].pan == 30)
    }

    @Test func aNewConfigMidSessionRebuildsTheStripsAndKeepsTheMix() {
        let set = withBand().after(
            .channelLevelChanged(channel: 4, level: 60), .channelLevelChanged(channel: 1, level: 40))
        let unlinked = [
            vox, info("Keys L", .keys), keysRight, info("Spare"), info("Click", .click, hidden: true),
        ]

        let rebuilt = set.after(.configReceived(config(5, [], unlinked)))

        #expect(rebuilt.mix.strips.map(\.label) == ["Vox", "Keys L", "Keys R", "Spare"])
        #expect(rebuilt.mix.strips.map(\.pairedWith) == [nil, nil, nil, nil])
        #expect(rebuilt.mix.levels == set.mix.levels)
        #expect(rebuilt.mix.pans == set.mix.pans)
    }

    @Test func channelControlsChangeOnlyTheirChannel() throws {
        let state = withChannels(3).after(
            .channelLevelChanged(channel: 1, level: 25), .channelPanChanged(channel: 1, pan: 50),
            .channelMuteChanged(channel: 1, muted: true), .channelSoloChanged(channel: 2, soloed: true))

        try #require(state.mix.strips.count == 3)
        #expect(
            state.mix.strips[1]
                == ChannelStrip(
                    index: 1, label: "CH 2", group: nil, level: 25, pan: 50, isMuted: true,
                    isSoloed: false, kind: nil, pairedWith: nil, isLocked: false, following: nil))
        #expect(
            state.mix.strips[0]
                == ChannelStrip(
                    index: 0, label: "CH 1", group: nil, level: 100, pan: 0, isMuted: false,
                    isSoloed: false, kind: nil, pairedWith: nil, isLocked: false, following: nil))
        #expect(state.mix.strips[2].isSoloed)
        #expect(state.mix.levels == [100, 25, 100])
    }

    @Test func levelsAndPansStayBetweenZeroAndAHundred() {
        let state = withChannels(1, [group("Band", [0])]).after(
            .channelLevelChanged(channel: 0, level: 140), .channelPanChanged(channel: 0, pan: -5),
            .groupLevelChanged(group: 0, level: 101), .masterVolumeChanged(level: 120),
            .levelCapChanged(level: -1))

        #expect(state.mix.levels == [100])
        #expect(state.mix.pans == [0])
        #expect(state.mix.groupLevels == [100])
        #expect(state.mix.masterVolume == 100)
        #expect(state.mix.levelCap == 0)
    }

    @Test func aControlForAChannelOrGroupThatDoesNotExistChangesNothing() {
        let state = withChannels(2)

        #expect(
            state.after(
                .channelLevelChanged(channel: 5, level: 10), .channelMuteChanged(channel: -1, muted: true),
                .channelPanChanged(channel: 2, pan: 10), .channelSoloChanged(channel: 9, soloed: true),
                .groupLevelChanged(group: 0, level: 50)) == state)
    }

    /// Android reaches a group list shorter than the groups through a recalled mix (T9, mix banks);
    /// the one-entry list is set directly here, which is the state that recall leaves.
    @Test func aGroupLevelFillsTheOtherGroupsAsTheyRead() {
        let groups = [group("A", [0]), group("B", [1]), group("C", [1])]
        var recalledShort = withChannels(2)
        recalledShort.mix.groupLevels = [20]
        let announced = recalledShort.after(.configReceived(config(2, groups)))

        let state = announced.after(.groupLevelChanged(group: 2, level: 70))

        #expect(state.mix.groupLevels == [20, 100, 70])
    }

    @Test func masterControlsChangeTheMaster() {
        let state = withChannels(2).after(
            .masterVolumeChanged(level: 80), .masterMuteChanged(muted: true), .levelCapChanged(level: 90))

        #expect(state.mix.masterVolume == 80)
        #expect(state.mix.masterMuted)
        #expect(state.mix.levelCap == 90)
    }
}
