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

    // MARK: Reconciliation with the desk

    @Test("numbers each change as the newest and is settled once the desk answered the newest")
    func syncArithmetic() {
        let fresh = MixSync()
        #expect(fresh == MixSync(latest: 0, answered: 0))
        #expect(fresh.isSettled)

        let twoChanges = fresh.changed().changed()
        #expect(twoChanges == MixSync(latest: 2, answered: 0))
        #expect(!twoChanges.isSettled)
        #expect(!twoChanges.answeredUpTo(1).isSettled)
        #expect(twoChanges.answeredUpTo(2).isSettled)
    }

    @Test("never takes an answered number back: the desk answers in order")
    func answeredOnlyGrows() {
        #expect(MixSync(latest: 5, answered: 4).answeredUpTo(2) == MixSync(latest: 5, answered: 4))
        #expect(MixSync(latest: 5, answered: 4).answeredUpTo(5) == MixSync(latest: 5, answered: 5))
    }

    @Test("keeps its numbers across a reconnect, and forgets only what the engineer set")
    func reconnectKeepsNumbers() {
        var state = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))
        state = state.after(.mixPushed(report().with(EngineerControls(limitPercent: 50))))

        let reconnected = state.after(.sessionEnded, .sessionStarted)

        #expect(reconnected.sync == MixSync(latest: 1, answered: 0))
        #expect(reconnected.mix.engineer == EngineerControls())
        #expect(reconnected.mix.levels == [40, 100])
        #expect(reconnected.isSessionRunning)
    }

    /// Android's `answerMix` moves the answered number before it asks whether a session is running
    /// (`MonitorReducer.kt:428-437`): the numbers only grow, across reconnects too.
    @Test("counts a late answer as answered and changes nothing else outside a session")
    func lateAnswerOnlyCounts() {
        let ended = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))
            .after(.sessionEnded)
        let answer = report(gains: [10, 10]).with(EngineerControls(limitPercent: 30))

        let answered = ended.after(.mixAnswered(sequence: 1, report: answer))

        #expect(answered.sync == MixSync(latest: 1, answered: 1))
        #expect(answered.mix == ended.mix)
        #expect(!answered.isSessionRunning)
    }

    @Test("hands the predicate the session before and after the change, and numbers nothing it refuses")
    func reducedSendingSeesBothSides() {
        let state = connectedWith(2)
        var seen: [[Int]] = []

        let next = state.reducedSending(.channelLevelChanged(channel: 1, level: 30)) { before, after in
            seen += [before.mix.levels, after.mix.levels]
            return false
        }

        #expect(seen == [[100, 100], [100, 30]])
        #expect(next.sync == MixSync())
        #expect(next.mix.levels == [100, 30])
    }

    @Test("starts a session running and ends it")
    func sessionRunningFlag() {
        #expect(!MixSession().isSessionRunning)
        #expect(running().isSessionRunning)
        #expect(!running().after(.sessionEnded).isSessionRunning)
    }

    @Test("shows the answer to the newest mix even when an older answer never came")
    func newestAnswerAfterALostOne() {
        let twoInFlight = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))
            .sent(.channelLevelChanged(channel: 0, level: 50))

        let answered = twoInFlight.after(.mixAnswered(sequence: 2, report: report(gains: [60, 70])))

        #expect(answered.mix.levels == [60, 70])
        #expect(answered.sync == MixSync(latest: 2, answered: 2))
    }
}
