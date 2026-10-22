import BandEarProtocol
import Testing

@testable import BandEarMix

/// The reducer's edits and `config` beyond the Android parity suite.
@Suite("Mix session")
struct MixSessionTests {

    /// The bridge T5a relied on: the states built by hand there are the ones the reducer builds.
    @Test("builds from a first config exactly the mix the layout tests start from")
    func firstConfigBridge() {
        #expect(withBand().mix == firstConfig(channels: 5, details: band))
        let groups = [group("A", [0]), group("B", [1, 2])]
        #expect(withChannels(3, groups).mix == firstConfig(channels: 3, groups: groups))
    }

    /// Android's parser drops such a `config` before its reducer sees it
    /// (`ControlChannel.kt:160`); the mix holds 32 channels, so the rule lives with the mix here.
    @Test("ignores a config for no channels or more than the mix holds", arguments: [0, 33, 255])
    func ignoresConfigOutsideTheMix(count: Int) {
        let state = withChannels(2)

        #expect(state.after(.configReceived(config(count))) == state)
    }

    @Test("takes a config for all 32 channels the mix holds")
    func takesFullConfig() {
        #expect(withChannels(DeskConfiguration.maximumMixChannels).mix.levels.count == 32)
    }

    @Test("leaves a locked strip as it is, and a pair when either side is locked")
    func lockedStripsRefuseEdits() {
        var state = withBand()
        state.mix.engineer = EngineerControls(lockedChannels: [0, 2])

        let edited = state.after(
            .channelLevelChanged(channel: 0, level: 10), .channelLevelChanged(channel: 1, level: 10),
            .channelPanChanged(channel: 1, pan: 10), .channelMuteChanged(channel: 1, muted: true),
            .channelSoloChanged(channel: 0, soloed: true))

        #expect(edited == state)
        #expect(edited.after(.channelLevelChanged(channel: 4, level: 10)).mix.levels == [100, 100, 100, 100, 10])
    }

    @Test("leaves channels and groups to the leader while following, and the master to the musician")
    func followingRefusesChannelAndGroupEdits() {
        var state = withChannels(2, [group("A", [0])])
        state.mix.engineer = EngineerControls(following: "Ana")

        #expect(
            state.after(
                .channelLevelChanged(channel: 0, level: 10), .channelPanChanged(channel: 1, pan: 10),
                .channelMuteChanged(channel: 0, muted: true), .groupLevelChanged(group: 0, level: 10))
                == state)

        let master = state.after(
            .masterVolumeChanged(level: 70), .masterMuteChanged(muted: true), .levelCapChanged(level: 80))
        #expect(master.mix.masterVolume == 70)
        #expect(master.mix.masterMuted)
        #expect(master.mix.levelCap == 80)
    }

    @Test("fills the other groups when a group level is set while the levels list is short")
    func groupLevelFillsAtChangeTime() {
        var state = withChannels(2, [group("A", [0]), group("B", [1])])
        state.mix.groupLevels = []

        #expect(state.after(.groupLevelChanged(group: 1, level: 30)).mix.groupLevels == [100, 30])
    }

    /// Android writes into every list by index and would throw on one shorter than the levels. The
    /// lists are always resized together, so only a state built by hand gets here, but a reducer
    /// behind a screen must not be able to crash the app over it.
    @Test("writes only what a list has room for when one is shorter than the levels")
    func shortListNeverTraps() {
        var state = withChannels(3)
        state.mix.mutes = [false]

        let edited = state.after(.channelMuteChanged(channel: 2, muted: true))

        #expect(edited.mix.mutes == [false])
        #expect(edited.mix.levels == state.mix.levels)
    }
}
