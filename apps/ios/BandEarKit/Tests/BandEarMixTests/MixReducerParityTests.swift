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

    @Test func onlyShownStripsCanBeChosenAndAPairIsChosenByItsFirstChannel() {
        let state = withBand()

        #expect(state.mix.musicianChoices.map(\.index) == [0, 1, 4])
        #expect(
            state.mix.musicianChoices == [
                ChannelChoice(index: 0, label: "Vox"), ChannelChoice(index: 1, label: "Keys"),
                ChannelChoice(index: 4, label: "CH 5"),
            ])
        // The hidden spare and the pair's second side are not offered, so they cannot be chosen.
        #expect(state.after(.musicianChannelChosen(3), .musicianChannelChosen(2)) == state)
        #expect(state.after(.musicianChannelChosen(1)).moreOfMe.musicianChannel == 1)
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

    // MARK: T5c, reconciliation with the desk

    // Server reports.

    @Test func aPushedMixTakesTheServersValuesForTheAnnouncedChannelsAndGroups() {
        let groups = [group("Band", [0, 1])]
        let state = connectedWith(2, groups)

        let pushed = state.after(.mixPushed(serverMix))

        #expect(pushed.mix.sourceChannels == 2)
        #expect(pushed.mix.levels == [80, 70])
        #expect(pushed.mix.pans == [25, 100])
        #expect(pushed.mix.mutes == [true, false])
        #expect(pushed.mix.solos == [false, true])
        #expect(pushed.mix.groups == groups)
        #expect(pushed.mix.groupLevels == [60])
        #expect(pushed.mix.masterVolume == 50)
        #expect(pushed.mix.levelCap == 90)
        #expect(pushed.mix.masterMuted)
    }

    @Test func aPushedMixIsClampedAndKeepsWhatTheServerDidNotSend() {
        let state = connectedWith(2, [group("Band", [0, 1])]).after(
            .channelLevelChanged(channel: 1, level: 40))

        let pushed = state.after(
            .mixPushed(report(gains: [140], pans: [-5], volume: 120, maxLevel: -1)))

        #expect(pushed.mix.levels == [100, 40])
        #expect(pushed.mix.pans == [0, 100])
        #expect(pushed.mix.groupLevels == [100])
        #expect(pushed.mix.masterVolume == 100)
        #expect(pushed.mix.levelCap == 0)
    }

    /// Android reaches the strips before the first `config` through a recalled mix (T9, mix banks);
    /// the recalled levels are set directly here, which is the state that recall leaves.
    @Test func aPushedMixBeforeTheFirstConfigKeepsTheStripsThePhoneShows() {
        var recalled = running()
        recalled.mix.levels = [30, 30, 30]
        recalled.mix.groupLevels = [20]

        let pushed = recalled.after(.mixPushed(serverMix))

        #expect(pushed.mix.levels == [80, 70, 100])
        #expect(pushed.mix.groupLevels == [60])
    }

    @Test func aPushedMixChangesNothingOutsideASession() {
        let idleWithChannels = withChannels(2)
        let ended = connectedWith(2).after(.sessionEnded)

        #expect(idleWithChannels.after(.mixPushed(serverMix)) == idleWithChannels)
        #expect(ended.after(.mixPushed(serverMix)) == ended)
    }

    @Test func aPushedMixTurnsMoreOfMeOffAndKeepsTheMixAsTheServerPlaysIt() {
        let on = connectedWith(2).after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))

        let pushed = on.after(.mixPushed(serverMix))

        #expect(!pushed.moreOfMe.isEnabled)
        #expect(pushed.moreOfMe.musicianChannel == 1)
        #expect(pushed.moreOfMe.notice == .mixChangedElsewhere)
        #expect(pushed.mix.levels == [80, 70])
    }

    @Test func aPushOfTheMixThePhoneAlreadyPlaysChangesNothing() throws {
        let plain = connectedWith(2).after(.channelLevelChanged(channel: 0, level: 80))
        let lifted = plain.after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        // Not in Android's test: without it, a reducer that ignored More of me would pass vacuously.
        try #require(lifted.moreOfMe.isEnabled)

        #expect(plain.after(.mixPushed(report(of: plain.mix))) == plain)
        // With More of me on the desk plays the lifted mix; hearing it back keeps More of me on.
        let played = MoreOfMe.liftedLevels(lifted.mix.levels, musicianChannels: [1], lockedChannels: [])
        #expect(lifted.after(.mixPushed(report(of: lifted.mix, gains: played))) == lifted)
    }

    // Seq: the phone's mixes, numbered, and the desk's answers to them.

    @Test func aChangeThatIsSentIsNumberedAsTheNewestMixAndOneThatIsNotKeepsTheNumber() {
        let state = connectedWith(2)

        let sent = state.sent(.channelLevelChanged(channel: 0, level: 40))
        let kept = sent.reducedSending(.channelLevelChanged(channel: 0, level: 40)) { before, after in
            before.mix != after.mix
        }

        #expect(sent.sync == MixSync(latest: 1, answered: 0))
        #expect(sent.mix.levels == [40, 100])
        #expect(kept == sent)
        #expect(!sent.sync.isSettled)
    }

    @Test func aPushWhileAMixOfThePhoneIsUnansweredIsDropped() {
        let outstanding = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))

        #expect(outstanding.after(.mixPushed(serverMix)) == outstanding)
    }

    @Test func onlyTheAnswerToTheNewestMixIsShown() {
        let twoInFlight = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))
            .sent(.channelLevelChanged(channel: 0, level: 50))

        let olderAnswered = twoInFlight.after(.mixAnswered(sequence: 1, report: serverMix))

        #expect(olderAnswered.mix.levels == [50, 100])
        #expect(olderAnswered.sync == MixSync(latest: 2, answered: 1))
        #expect(olderAnswered.after(.mixPushed(serverMix)) == olderAnswered)

        let newestAnswered = olderAnswered.after(.mixAnswered(sequence: 2, report: serverMix))

        #expect(newestAnswered.mix.levels == [80, 70])
        #expect(newestAnswered.sync.isSettled)
    }

    @Test func aRefusedNewestMixSettlesItSoTheNextPushIsShown() {
        let refused = connectedWith(2).sent(.channelLevelChanged(channel: 0, level: 40))
            .after(.mixAnswered(sequence: 1, report: nil))

        #expect(refused.mix.levels == [40, 100])
        #expect(refused.sync.isSettled)
        #expect(refused.after(.mixPushed(serverMix)).mix.levels == [80, 70])
    }

    // Engineer: the engineer's limit and locks.

    @Test func theReportedLimitAndLocksAreKeptAndShowOnTheStrips() {
        let state = lockedBy(engineer(limit: 60, locked: [1]))

        #expect(state.mix.engineer == engineer(limit: 60, locked: [1]))
        #expect(state.mix.strips.map(\.isLocked) == [false, true, false])
        #expect(state.mix.isStripLocked(1))
        #expect(!state.mix.isStripLocked(0))
    }

    @Test func anOlderServerLeavesEverythingOpenAndUnlimited() {
        let state = connectedWith(3).after(.mixPushed(serverMix))

        #expect(state.mix.engineer == EngineerControls())
        #expect(state.mix.strips.map(\.isLocked) == [false, false, false])
        #expect(state.mix.effectiveCap == state.mix.levelCap)
    }

    /// Android also reads the phone's cap back off the mix it sends (`toSnapshot().maxLevelPercent`);
    /// the outgoing mix is T5e, which ports that assertion.
    @Test func theMusicianHearsTheLowerOfTheirCapAndTheEngineersLimit() {
        let limited = lockedBy(engineer(limit: 60))

        #expect(limited.mix.levelCap == 100)
        #expect(limited.mix.effectiveCap == 60)
        // The phone's own cap still moves, and wins when it is lower.
        let lowerCap = limited.after(.levelCapChanged(level: 50))
        #expect(lowerCap.mix.levelCap == 50)
        #expect(lowerCap.mix.effectiveCap == 50)
    }

    @Test func aLockedChannelCannotBeChangedFromThePhone() {
        let state = lockedBy(engineer(locked: [1]))

        let tried = state.after(
            .channelLevelChanged(channel: 1, level: 40), .channelPanChanged(channel: 1, pan: 50),
            .channelMuteChanged(channel: 1, muted: true), .channelSoloChanged(channel: 1, soloed: true))

        #expect(tried == state)
        // The other channels still move.
        #expect(tried.after(.channelLevelChanged(channel: 0, level: 30)).mix.levels == [30, 100, 100])
    }

    @Test func aPairIsLockedAsOneStripWhenEitherSideIs() {
        let pair = [info("Keys", .keys, link: true), info(), info("Vox")]
        let state = lockedBy(engineer(locked: [1]), pair)

        #expect(state.mix.strips.map(\.isLocked) == [true, false])
        #expect(state.mix.isStripLocked(0))
        #expect(
            state.after(
                .channelLevelChanged(channel: 0, level: 40), .channelPanChanged(channel: 0, pan: 25),
                .channelMuteChanged(channel: 0, muted: true)) == state)
    }

    @Test func aLockArrivingWhileAMixIsOutstandingHoldsAtOnceAndSettlesWithTheAnswer() {
        let outstanding = connectedWith(3).sent(.channelLevelChanged(channel: 0, level: 40))
        let kept = neutralMix(gains: (0..<32).map { $0 == 0 ? 70 : 100 })

        // The push is overtaken by the phone's mix still on its way, but the lock holds at once.
        let pushed = outstanding.after(.mixPushed(kept.with(engineer(locked: [0]))))
        #expect(pushed.mix.levels == [40, 100, 100])
        #expect(pushed.mix.isStripLocked(0))
        #expect(pushed.after(.channelLevelChanged(channel: 0, level: 20)) == pushed)

        // The answer to that mix carries the value the desk kept, and settles it.
        let answered = pushed.after(.mixAnswered(sequence: 1, report: kept.with(engineer(locked: [0]))))
        #expect(answered.mix.levels == [70, 100, 100])
        #expect(answered.sync.isSettled)
        #expect(answered.mix.isStripLocked(0))
    }

    @Test func aRefusedMixLeavesTheEngineersControlsAsTheyWere() {
        let state = lockedBy(engineer(limit: 60, locked: [1])).sent(
            .channelLevelChanged(channel: 0, level: 40))

        #expect(
            state.after(.mixAnswered(sequence: 1, report: nil)).mix.engineer
                == engineer(limit: 60, locked: [1]))
    }

    @Test func aNewSessionStartsUnlockedAndReportsOutsideOneChangeNothing() {
        let ended = lockedBy(engineer(limit: 60, locked: [1])).after(.sessionEnded)

        #expect(ended.after(.mixPushed(serverMix.with(engineer(limit: 40, locked: [0])))) == ended)
        #expect(ended.after(.sessionStarted).mix.engineer == EngineerControls())
    }

    // Following another musician's mix (a mirror the engineer set).

    @Test func aReportedFollowingHoldsAndAnOlderServerFollowsNobody() {
        let state = lockedBy(following("Ana"))

        #expect(state.mix.following == "Ana")
        #expect(connectedWith(3).after(.mixPushed(serverMix)).mix.following == nil)
        #expect(state.after(.mixPushed(neutralMix().with(following(nil)))).mix.following == nil)
    }

    @Test func followingThatStartsWhileAMixIsOutstandingHoldsAtOnceAndTheAnswerBringsTheLeadersMix() {
        let outstanding = connectedWith(3).sent(.channelLevelChanged(channel: 0, level: 40))

        let pushed = outstanding.after(.mixPushed(leadersMix(60).with(following("Ana"))))

        // The push is overtaken by the phone's mix still on its way; whom it follows holds at once.
        #expect(pushed.mix.following == "Ana")
        #expect(pushed.mix.levels == [40, 100, 100])

        let answered = pushed.after(
            .mixAnswered(sequence: 1, report: leadersMix(60).with(following("Ana"))))

        #expect(answered.mix.levels == [60, 60, 60])
        #expect(answered.sync.isSettled)
    }

    @Test func whileFollowingChannelAndGroupEditsChangeNothing() {
        let state = running().after(
            .configReceived(config(3, [group("Band", [0, 1])])),
            .mixPushed(neutralMix().with(following("Ana"))))

        let tried = state.after(
            .channelLevelChanged(channel: 0, level: 40), .channelPanChanged(channel: 1, pan: 20),
            .channelMuteChanged(channel: 2, muted: true), .channelSoloChanged(channel: 0, soloed: true),
            .groupLevelChanged(group: 0, level: 50))

        #expect(tried == state)
        #expect(state.mix.strips.map(\.following) == ["Ana", "Ana", "Ana"])
        #expect(state.mix.groupFaders.map(\.following) == ["Ana"])
    }

    @Test func whileFollowingTheMasterTheCapAndTheMuteAreStillTheMusicians() {
        let state = lockedBy(following("Ana")).after(
            .masterVolumeChanged(level: 70), .levelCapChanged(level: 80), .masterMuteChanged(muted: true))

        #expect(state.mix.masterVolume == 70)
        #expect(state.mix.levelCap == 80)
        #expect(state.mix.masterMuted)
    }

    @Test func anEditAfterFollowingStartsWithAMixOutstandingChangesNothing() {
        let pushed = connectedWith(3).sent(.channelLevelChanged(channel: 0, level: 40))
            .after(.mixPushed(leadersMix(60).with(following("Ana"))))

        #expect(
            pushed.after(
                .channelLevelChanged(channel: 1, level: 20), .channelMuteChanged(channel: 0, muted: true))
                == pushed)
    }

    @Test func whenFollowingStopsTheControlsAreTheMusiciansAgainAndTheMixStaysAsItWas() {
        let followed = connectedWith(3, [group("Band", [0])]).after(
            .musicianChannelChosen(1), .moreOfMeToggled(on: true),
            .mixPushed(leadersMix(60).with(following("Ana"))))

        let free = followed.after(.mixPushed(leadersMix(60).with(following(nil))))

        #expect(free.mix.following == nil)
        #expect(free.mix.levels == [60, 60, 60])
        #expect(free.moreOfMe.notice == nil)
        #expect(!free.moreOfMe.isEnabled)
        #expect(free.after(.channelLevelChanged(channel: 0, level: 30)).mix.levels == [30, 60, 60])
        #expect(free.after(.groupLevelChanged(group: 0, level: 50)).mix.groupLevels == [50])
        #expect(free.after(.moreOfMeToggled(on: true)).moreOfMe.isEnabled)
        #expect(free.mix.strips.map(\.following) == [nil, nil, nil])
    }

    @Test func aLockedChannelOfAFollowerIsBothLockedAndFollowing() throws {
        let strips = lockedBy(following("Ana", locked: [1])).mix.strips
        try #require(strips.count == 3)

        #expect(strips[1].isLocked)
        #expect(strips[1].following == "Ana")
    }

    @Test func aNewSessionFollowsNobodyUntilItsServerSays() {
        let ended = lockedBy(following("Ana")).after(.sessionEnded)

        #expect(ended.mix.following == "Ana")
        #expect(ended.after(.sessionStarted).mix.following == nil)
    }

    // MARK: T5d, More of me

    @Test func moreOfMeNeedsAChannelBeforeItTurnsOn() {
        let state = withChannels(2).after(.moreOfMeToggled(on: true))

        #expect(!state.moreOfMe.isEnabled)
        #expect(state.moreOfMe.notice == .needsChannel)
    }

    @Test func moreOfMeTurnsOnAndOffOnceAChannelIsChosen() throws {
        let on = withChannels(2).after(
            .moreOfMeToggled(on: true), .musicianChannelChosen(1), .moreOfMeToggled(on: true))

        #expect(on.moreOfMe.isEnabled)
        #expect(on.moreOfMe.notice == nil)
        let channel = try #require(on.moreOfMe.musicianChannel)
        #expect(on.mix.label(of: channel) == "CH 2")
        #expect(!on.after(.moreOfMeToggled(on: false)).moreOfMe.isEnabled)
    }

    @Test func moreOfMeIsNeverWrittenIntoTheMix() {
        let on = withChannels(2).after(
            .channelLevelChanged(channel: 0, level: 80), .musicianChannelChosen(1),
            .moreOfMeToggled(on: true))

        #expect(on.mix.levels == [80, 100])
    }

    @Test func choosingNoChannelTurnsMoreOfMeOff() {
        let off = withChannels(2).after(
            .musicianChannelChosen(0), .moreOfMeToggled(on: true), .musicianChannelChosen(nil))

        #expect(off.moreOfMe.musicianChannel == nil)
        #expect(!off.moreOfMe.isEnabled)
        #expect(off.moreOfMe.notice == .noChannelSelected)
    }

    @Test func choosingAnotherChannelKeepsMoreOfMeOn() {
        let moved = withChannels(3).after(
            .musicianChannelChosen(0), .moreOfMeToggled(on: true), .musicianChannelChosen(2))

        #expect(moved.moreOfMe.isEnabled)
        #expect(moved.moreOfMe.musicianChannel == 2)
    }

    @Test func onlyAnAnnouncedChannelCanBeChosen() throws {
        let state = withChannels(2).after(.musicianChannelChosen(0))
        // Not in Android's test: without it, a reducer that ignored every choice would pass.
        try #require(state.moreOfMe.musicianChannel == 0)

        #expect(state.after(.musicianChannelChosen(2), .musicianChannelChosen(-1)) == state)
        #expect(MixSession().after(.musicianChannelChosen(0)) == MixSession())
    }

    @Test func moreOfMeLiftsBothSidesOfAPair() {
        let lifted = running().after(
            .configReceived(config(5, [], band)), .musicianChannelChosen(1), .moreOfMeToggled(on: true))
        let played = lifted.played()

        #expect(played == [50, 100, 100, 50, 50])
        // The desk plays both sides lifted; hearing that back keeps More of me on.
        #expect(lifted.after(.mixPushed(report(of: lifted.mix, gains: played))) == lifted)
    }

    // A `config`: the musician's channel fitted to the announced strips, and pairs the engineer links.

    @Test func aConfigThatDropsTheMusiciansChannelTurnsMoreOfMeOff() {
        let on = withChannels(4).after(.musicianChannelChosen(3), .moreOfMeToggled(on: true))

        let dropped = on.after(.configReceived(config(2)))

        #expect(dropped.moreOfMe.musicianChannel == nil)
        #expect(!dropped.moreOfMe.isEnabled)
        #expect(dropped.moreOfMe.notice == .channelOutOfRange)
    }

    /// Android builds the remembered channel into the state directly, as the app does with the
    /// channel it kept from the last launch (T6); so does this.
    @Test func aRememberedChannelOutsideTheFirstConfigIsForgottenQuietly() {
        var remembered = MixSession()
        remembered.moreOfMe.musicianChannel = 6

        let state = remembered.after(.configReceived(config(4)))

        #expect(state.moreOfMe.musicianChannel == nil)
        #expect(state.moreOfMe.notice == nil)
    }

    @Test func aRememberedChannelInsideTheConfigIsKept() {
        var remembered = MixSession()
        remembered.moreOfMe.musicianChannel = 3

        #expect(remembered.after(.configReceived(config(4))).moreOfMe.musicianChannel == 3)
    }

    @Test func theMusiciansChannelOnAPairsSecondSideMovesToThePair() {
        let remembered = withChannels(5).after(.musicianChannelChosen(2))

        let paired = remembered.after(.configReceived(config(5, [], band)))

        #expect(paired.moreOfMe.musicianChannel == 1)
        #expect(paired.moreOfMe.notice == nil)
        #expect(paired.moreOfMeChannels == [1, 2])
    }

    @Test func aConfigThatPairsTheMusiciansChannelTurnsMoreOfMeOffAndSaysWhy() {
        // The musician's channel on what becomes the pair's second side: it moves to the first, and
        // the lift the desk plays (one side) no longer matches the strip's (both sides).
        let second = withChannels(5).after(.musicianChannelChosen(2), .moreOfMeToggled(on: true))
            .after(.configReceived(config(5, [], band)))

        #expect(second.moreOfMe.musicianChannel == 1)
        #expect(!second.moreOfMe.isEnabled)
        #expect(second.moreOfMe.notice == .channelPaired)

        // The musician's channel on what becomes the pair's first side: the same.
        let first = withChannels(5).after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
            .after(.configReceived(config(5, [], band)))

        #expect(first.moreOfMe.musicianChannel == 1)
        #expect(!first.moreOfMe.isEnabled)
        #expect(first.moreOfMe.notice == .channelPaired)

        // Turned on again it lifts the pair, and a config that keeps the pair as it is leaves it on.
        let again = second.after(.moreOfMeToggled(on: true), .configReceived(config(5, [], band)))

        #expect(again.moreOfMe.isEnabled)
        #expect(again.moreOfMeChannels == [1, 2])
        // A pair elsewhere leaves it alone.
        let elsewhere = withChannels(5).after(.musicianChannelChosen(0), .moreOfMeToggled(on: true))
            .after(.configReceived(config(5, [], band)))

        #expect(elsewhere.moreOfMe.isEnabled)
        #expect(elsewhere.moreOfMe.notice == nil)
    }

    @Test func afterALinkTurnsMoreOfMeOffTheMusiciansOwnLevelsGoBackWithTheMovedPans() {
        let on = running().after(
            .configReceived(config(5)),
            .channelLevelChanged(channel: 0, level: 80), .channelLevelChanged(channel: 1, level: 60),
            .channelLevelChanged(channel: 2, level: 40), .channelLevelChanged(channel: 3, level: 70),
            .channelLevelChanged(channel: 4, level: 90),
            .musicianChannelChosen(2), .moreOfMeToggled(on: true))
        // The desk plays the lift the phone sent: CH 3 at 100, the rest halved.
        #expect(
            MoreOfMe.liftedLevels(on.mix.levels, musicianChannels: [2], lockedChannels: [])
                == [40, 30, 100, 35, 45])

        let paired = on.after(.configReceived(config(5, [], band)))

        #expect(!paired.moreOfMe.isEnabled)
        #expect(paired.sync == on.sync)

        // The link's `mix_state`: the pair's second side took the first's (lifted) level, and its
        // pans moved apart. The phone keeps those pans and puts its own levels back, once.
        let restored = paired.after(.mixPushed(linkedMix))

        #expect(restored.mix.levels == [80, 60, 60, 70, 90])
        #expect(restored.mix.pans == [0, 0, 100, 100, 0])
        #expect(!restored.moreOfMe.isEnabled)
        #expect(restored.moreOfMe.notice == .channelPaired)
        #expect(restored.sync.latest == paired.sync.latest + 1)

        // Its answer changes nothing more, and More of me turned on again lifts once.
        let answered = restored.after(
            .mixAnswered(sequence: restored.sync.latest, report: report(of: restored.mix)))
        #expect(answered.mix == restored.mix)
        #expect(answered.sync.isSettled)
        let again = answered.after(.moreOfMeToggled(on: true))
        #expect(again.played(locked: again.mix.lockedSides) == [40, 100, 100, 35, 45])
    }

    @Test func anEditMadeBeforeTheLinksMixArrivesStandsAndNoOlderLevelsAreRestored() throws {
        let paired = running().after(
            .configReceived(config(5)),
            .channelLevelChanged(channel: 0, level: 80), .channelLevelChanged(channel: 1, level: 60),
            .channelLevelChanged(channel: 2, level: 40), .channelLevelChanged(channel: 3, level: 70),
            .channelLevelChanged(channel: 4, level: 90),
            .musicianChannelChosen(2), .moreOfMeToggled(on: true),
            .configReceived(config(5, [], band)))
        // Not in Android's test: without levels waiting, "none restored" would hold vacuously.
        try #require(paired.ownLevelsToRestore != nil)

        // The musician moves a fader before the link's `mix_state` arrives: that send carries their
        // own levels (More of me is off), so nothing waits to be put back any more.
        let edited = paired.sent(.channelLevelChanged(channel: 0, level: 55))
        #expect(edited.ownLevelsToRestore == nil)

        // The desk took that whole mix (the pair's sides made equal, the first winning).
        let answer = report(
            gains: [55, 60, 60, 70, 90], pans: [0, 100, 0, 100, 0],
            mutes: Array(repeating: false, count: 5), solos: Array(repeating: false, count: 5))
        let answered = edited.after(
            .mixPushed(linkedMix), .mixAnswered(sequence: edited.sync.latest, report: answer))

        #expect(answered.mix.levels == [55, 60, 60, 70, 90])
        #expect(answered.sync.latest == edited.sync.latest)
        #expect(answered.sync.isSettled)
    }

    @Test func aHiddenMusicianChannelIsForgottenAndMoreOfMeSaysWhy() {
        let on = withChannels(5).after(.musicianChannelChosen(3), .moreOfMeToggled(on: true))
        let remembered = withChannels(5).after(.musicianChannelChosen(3))

        let hiddenOn = on.after(.configReceived(config(5, [], band)))
        let hiddenRemembered = remembered.after(.configReceived(config(5, [], band)))

        #expect(hiddenOn.moreOfMe.musicianChannel == nil)
        #expect(!hiddenOn.moreOfMe.isEnabled)
        #expect(hiddenOn.moreOfMe.notice == .channelOutOfRange)
        #expect(hiddenRemembered.moreOfMe.musicianChannel == nil)
        #expect(hiddenRemembered.moreOfMe.notice == nil)
    }

    // What the engineer set: locks and following.

    @Test func moreOfMeLeavesAPairWholeWhenOnlyOneSideIsLocked() {
        let on = lockedBy(engineer(locked: [1]), keysPair).after(
            .musicianChannelChosen(2), .moreOfMeToggled(on: true))
        let played = on.played(locked: on.mix.lockedSides)

        #expect(on.moreOfMe.isEnabled)
        // Neither side of the locked pair is lowered; the voice is lifted.
        #expect(played == [100, 100, 100])
        // The desk keeps the pair as it was; hearing that back keeps More of me on.
        #expect(on.after(.mixPushed(report(of: on.mix, gains: played).with(on.mix.engineer))) == on)
    }

    @Test func moreOfMeNeitherLiftsNorLowersALockedChannel() {
        let on = lockedBy(engineer(locked: [2])).after(
            .channelLevelChanged(channel: 0, level: 80), .musicianChannelChosen(1),
            .moreOfMeToggled(on: true))
        let played = on.played(locked: on.mix.engineer.lockedChannels)

        #expect(on.moreOfMe.isEnabled)
        #expect(played == [40, 100, 100])
        // The desk plays exactly that; hearing it back keeps More of me on.
        #expect(on.after(.mixPushed(report(of: on.mix, gains: played).with(on.mix.engineer))) == on)
    }

    @Test func moreOfMeIsOffWhileTheEngineerLocksTheMusiciansChannel() {
        let on = connectedWith(3).after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        let lifted = MoreOfMe.liftedLevels(on.mix.levels, musicianChannels: [1], lockedChannels: [])

        let locked = on.after(
            .mixPushed(report(of: on.mix, gains: lifted).with(engineer(locked: [1]))))

        #expect(!locked.moreOfMe.isEnabled)
        #expect(locked.moreOfMe.musicianChannel == 1)
        #expect(locked.moreOfMe.notice == .channelLocked)
        // Turning it on again is refused while the channel is locked, and says why.
        let asked = locked.after(.moreOfMeToggled(on: false), .moreOfMeToggled(on: true))
        #expect(!asked.moreOfMe.isEnabled)
        #expect(asked.moreOfMe.notice == .channelLocked)
        // Choosing a locked channel while More of me is on turns it off too.
        let moved = lockedBy(engineer(locked: [2])).after(
            .musicianChannelChosen(0), .moreOfMeToggled(on: true), .musicianChannelChosen(2))
        #expect(!moved.moreOfMe.isEnabled)
        #expect(moved.moreOfMe.musicianChannel == 2)
        #expect(moved.moreOfMe.notice == .channelLocked)
    }

    @Test func moreOfMeIsOffWhileFollowingAndSaysWhy() {
        let on = connectedWith(3).after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))

        let followed = on.after(.mixPushed(leadersMix(60).with(following("Ana"))))

        #expect(!followed.moreOfMe.isEnabled)
        #expect(followed.moreOfMe.notice == .following)
        #expect(followed.mix.levels == [60, 60, 60])
        // Turning it on is refused while it lasts; the musician's channel can still be chosen.
        let asked = followed.after(.moreOfMeToggled(on: true))
        #expect(!asked.moreOfMe.isEnabled)
        #expect(asked.moreOfMe.notice == .following)
        #expect(asked.after(.musicianChannelChosen(2)).moreOfMe.musicianChannel == 2)
    }
}

// MARK: - T5c fixtures, as `MonitorReducerTest.kt` names them

/// The desk's whole mix, as `mix_state` carries it: full 32-channel and 16-group tables.
private let serverMix = report(
    gains: (0..<32).map { $0 < 2 ? 80 - 10 * $0 : 100 },
    pans: (0..<32).map { $0 == 0 ? 25 : 100 },
    mutes: (0..<32).map { $0 == 0 },
    solos: (0..<32).map { $0 == 1 },
    groupLevels: (0..<16).map { $0 == 0 ? 60 : 100 },
    volume: 50, maxLevel: 90, muted: true)

/// The desk's whole mix at the neutral values a fresh three-channel session already shows, with
/// `gains` in place of the neutral levels.
private func neutralMix(gains: [Int] = Array(repeating: 100, count: 32)) -> MixReport {
    report(
        gains: gains, pans: (0..<32).map { $0 % 2 == 0 ? 0 : 100 },
        mutes: Array(repeating: false, count: 32), solos: Array(repeating: false, count: 32),
        groupLevels: Array(repeating: 100, count: 16))
}

/// The leader's whole mix, every channel at `level`.
private func leadersMix(_ level: Int) -> MixReport {
    neutralMix(gains: Array(repeating: level, count: 32))
}

/// The engineer's controls: a limit of `limit` and `locked` channels.
private func engineer(limit: Int = 100, locked: Set<Int> = []) -> EngineerControls {
    EngineerControls(limitPercent: limit, lockedChannels: locked)
}

/// The engineer's controls with the musician following `name`'s mix, or nobody's.
private func following(_ name: String?, locked: Set<Int> = []) -> EngineerControls {
    EngineerControls(lockedChannels: locked, following: name)
}

/// A session of three channels whose desk reported the engineer's `controls` with its mix.
private func lockedBy(_ controls: EngineerControls, _ details: [SourceChannel]? = nil) -> MixSession {
    running().after(
        .configReceived(config(3, [], details)), .mixPushed(neutralMix().with(controls)))
}

// MARK: - T5d fixtures, as `MonitorReducerTest.kt` names them

/// A keys pair on channels 0 and 1 and a voice on channel 2.
private let keysPair = [info("Keys", .keys, link: true), info(), info("Vox")]

/// The `mix_state` the engineer's link of channels 1 and 2 sends while More of me lifted channel 2:
/// the pair's second side took the first's (lifted) level, and its pans moved apart.
private let linkedMix = report(
    gains: [40, 30, 30, 35, 45], pans: [0, 0, 100, 100, 0],
    mutes: Array(repeating: false, count: 5), solos: Array(repeating: false, count: 5))
