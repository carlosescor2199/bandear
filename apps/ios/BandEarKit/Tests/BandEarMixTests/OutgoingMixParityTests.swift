import BandEarProtocol
import Foundation
import Testing

@testable import BandEarMix

/// The Android client's tests of the `mix` it sends and of when it sends one, ported case for case:
/// the two of `ControlChannelTest.kt` about the message, and the send tests of
/// `MonitorViewModelTest.kt` (`apps/android/app/src/test/java/com/bandear/android/`).
///
/// Android drives its ViewModel through a fake socket; here ``Phone`` drives the session through
/// the same events, sends what ``MixSession/outgoingMix()`` builds whenever a mix is numbered, and
/// the desk's reports arrive as the JSON Android's tests write. A `config` is handed over as the
/// session layer hands it (``MixEvent/configReceived(_:)``), since its decoding has its own suite.
/// What Android checks on the receive thread (`receiver.mixState`) and in its store
/// (`store.channel`) belongs to the app (T6) and is left out where a test checks it.
@Suite("Outgoing mix: Android parity")
struct OutgoingMixParityTests {

    // MARK: ControlChannelTest.kt

    /// Android pushes the controls in a `mix_state` and reads them off the mix it sends next.
    @Test func aMixNeverCarriesTheEngineersControls() throws {
        var phone = Phone.connected(2)
        try phone.answerMixes()
        try phone.receive(engineerState(60, locked: 0))
        // Not in Android's test, whose sent mix never came from the state it pushed: the phone holds
        // the controls, so their absence below is the rule and not an accident.
        try #require(phone.session.mix.engineer == EngineerControls(limitPercent: 60, lockedChannels: [0]))

        let mix = try WireMix(phone.session.outgoingMix())

        #expect(!mix.has("engineer_limit_percent"))
        #expect(!mix.has("locked"))
        #expect(!mix.has("following"))
    }

    /// Android hands the channel its newest number through `mixSequence`, 12 and then null. The
    /// session's number is never absent, so the unnumbered half is ``MixCommand``'s: a mix without
    /// a number writes no `seq`.
    @Test func aMixCarriesTheNumberOfTheNewestChangeAndNoneWithoutOne() throws {
        var session = connectedWith(2)
        session.sync = MixSync(latest: 12, answered: 12)

        var unnumbered = session.outgoingMix()
        unnumbered.sequence = nil

        #expect(try WireMix(session.outgoingMix()).int("seq") == 12)
        #expect(try !WireMix(unnumbered).has("seq"))
    }

    // MARK: MonitorViewModelTest.kt, what is sent

    @Test func channelChangesAreSentAsAMix() throws {
        var phone = Phone.connected(2)

        phone.apply(
            .channelLevelChanged(channel: 1, level: 40), .channelMuteChanged(channel: 0, muted: true),
            .channelSoloChanged(channel: 1, soloed: true), .channelPanChanged(channel: 0, pan: 50))

        let last = try phone.lastMix()
        #expect(last.ints("channels") == [100, 40])
        #expect(last.bools("mutes") == [true, false])
        #expect(last.bools("solos") == [false, true])
        #expect(last.ints("pans") == [50, 100])
        #expect(phone.session.mix.levels == [100, 40])
    }

    @Test func aPanFromTheScreensIsKeptAndSentAsAMix() throws {
        var phone = Phone.connected(2)

        phone.apply(.channelPanChanged(channel: 1, pan: 35))

        #expect(phone.session.mix.pans == [0, 35])
        #expect(try phone.lastMix().ints("pans") == [0, 35])
    }

    @Test func aChangeThatChangesNothingSendsNothing() {
        var phone = Phone.connected(2)
        let sent = phone.sent.count

        phone.apply(
            .channelLevelChanged(channel: 0, level: 100), .channelLevelChanged(channel: 7, level: 20))

        #expect(phone.sent.count == sent)
        // Not in Android's test: a change that changes something is sent, so the count above is not
        // a phone that sends nothing at all.
        phone.apply(.channelLevelChanged(channel: 0, level: 90))
        #expect(phone.sent.count == sent + 1)
    }

    @Test func groupLevelsAreSentAsAMix() throws {
        var phone = Phone.connected(config(2, [group("Band", [0, 1])]))

        phone.apply(.groupLevelChanged(group: 0, level: 65))

        #expect(try phone.lastMix().ints("group_levels") == [65])
        #expect(phone.session.mix.groups.map(\.name) == ["Band"])
    }

    @Test func aPairsStripSendsBothSidesAndMoreOfMeLiftsBoth() throws {
        var phone = Phone.connected(3)
        phone.apply(announcePair)

        phone.apply(.channelLevelChanged(channel: 0, level: 60), .channelPanChanged(channel: 0, pan: 75))

        #expect(phone.session.mix.strips.map(\.label) == ["Keys", "Vox"])
        #expect(try phone.lastMix().ints("channels") == [60, 60, 100])
        #expect(try phone.lastMix().ints("pans") == [50, 100, 0])

        phone.apply(.musicianChannelChosen(0), .moreOfMeToggled(on: true))

        #expect(try phone.lastMix().ints("channels") == [100, 100, 50])
    }

    /// Android also checks the channel it stored (`store.channel`), which is the app's (T6).
    @Test func aConfigThatPairsTheMusiciansChannelRemembersThePairAndSendsNothing() {
        var phone = Phone.connected(3)
        phone.apply(.musicianChannelChosen(1))
        let sent = phone.sent.count

        phone.apply(announcePair)

        #expect(phone.session.moreOfMe.musicianChannel == 0)
        // The desk panned the new pair apart; its `mix_state` follows, so the phone's mix stays put.
        #expect(phone.sent.count == sent)
    }

    /// Android also checks the channel it stored (`store.channel`), which is the app's (T6).
    @Test func aConfigThatPairsTheMusiciansChannelWithMoreOfMeOnDropsTheLiftOnceTheLinksMixArrives()
        throws
    {
        var phone = Phone.connected(3)
        phone.apply(
            .channelLevelChanged(channel: 0, level: 80), .channelLevelChanged(channel: 1, level: 60),
            .channelLevelChanged(channel: 2, level: 40), .channelPanChanged(channel: 0, pan: 30),
            .channelPanChanged(channel: 1, pan: 70), .channelPanChanged(channel: 2, pan: 50),
            .musicianChannelChosen(1), .moreOfMeToggled(on: true))
        #expect(try phone.lastMix().ints("channels") == [40, 100, 20])
        try phone.answerMixes()
        let sent = phone.sent.count

        phone.apply(announcePair)

        #expect(phone.session.moreOfMe.musicianChannel == 0)
        #expect(!phone.session.moreOfMe.isEnabled)
        #expect(phone.session.moreOfMe.notice == .channelPaired)
        // The link's own `mix_state` follows with the pans it moved; a send now would put the
        // phone's old pans back over them.
        #expect(phone.sent.count == sent)

        // That push still carries the lift (the second side took the first's lifted level) and the
        // pans the link moved apart.
        var linked = mixState(levels: [40, 40, 20])
        linked["pans"] = (0..<32).map { $0 == 1 ? 100 : ($0 == 2 ? 50 : 0) }
        try phone.receive(linked)

        // One whole mix goes out: the musician's own levels (the pair's at its first side's) with
        // the moved pans, and the phone keeps the same.
        #expect(phone.sent.count == sent + 1)
        #expect(try phone.lastMix().ints("channels") == [80, 80, 40])
        #expect(try phone.lastMix().ints("pans") == [0, 100, 50])
        #expect(phone.session.mix.levels == [80, 80, 40])
        #expect(phone.session.mix.pans == [0, 100, 50])
        #expect(!phone.session.moreOfMe.isEnabled)

        try phone.answerMixes()
        phone.apply(.moreOfMeToggled(on: true))

        #expect(try phone.lastMix().ints("channels") == [100, 100, 20])
    }

    /// Android also checks the receive thread's ceiling (`receiver.mixState`), which is the app's
    /// (T6); it is copied from ``MixState/effectiveCap``.
    @Test func theEngineersLimitHoldsThePhonesOwnPlaybackAndIsNeverSent() throws {
        var phone = Phone.connected(3)
        try phone.answerMixes()

        try phone.receive(engineerState(60))
        phone.apply(.levelCapChanged(level: 90))

        #expect(phone.session.mix.effectiveCap == 60)
        let last = try phone.lastMix()
        #expect(last.int("max_level_percent") == 90)
        #expect(!last.has("engineer_limit_percent"))
        #expect(!last.has("locked"))
    }

    @Test func aLockedChannelsEditSendsNothingAndMoreOfMeLeavesIt() throws {
        var phone = Phone.connected(3)
        try phone.answerMixes()
        try phone.receive(engineerState(100, locked: 0))
        let sent = phone.sent.count

        phone.apply(
            .channelLevelChanged(channel: 0, level: 40), .channelMuteChanged(channel: 0, muted: true))

        #expect(phone.sent.count == sent)

        phone.apply(.musicianChannelChosen(1), .moreOfMeToggled(on: true))

        #expect(try phone.lastMix().ints("channels") == [100, 100, 50])
    }

    /// Android checks the receive thread's ceiling (`receiver.mixState.maxLevelPercent`), which is
    /// the app's (T6). The value it copies is ``MixState/effectiveCap``, checked here in its place.
    @Test func aLimitOnlyReportMovesThePhonesOwnCeilingWithNoMasterEdit() throws {
        var phone = Phone.connected(3)
        try phone.answerMixes()
        #expect(phone.session.mix.effectiveCap == 100)

        try phone.receive(engineerState(60))

        #expect(phone.session.mix.effectiveCap == 60)

        // An answer that only lowers the limit moves it too; the change it answers kept the cap.
        phone.apply(.channelLevelChanged(channel: 0, level: 40))
        var answer = try phone.lastMix().object
        answer["type"] = "mix_ack"
        answer["engineer_limit_percent"] = 45
        answer["locked"] = Array(repeating: false, count: 32)
        try phone.receive(answer)

        #expect(phone.session.mix.effectiveCap == 45)
        #expect(phone.session.mix.levelCap == 100)
    }

    @Test func moreOfMeLeavesBothSidesOfAHalfLockedPairInWhatItSends() throws {
        var phone = Phone.connected(3)
        phone.apply(announcePair)
        try phone.answerMixes()
        try phone.receive(engineerState(100, locked: 1))

        phone.apply(.musicianChannelChosen(2), .moreOfMeToggled(on: true))

        #expect(try phone.lastMix().ints("channels") == [100, 100, 100])
    }

    @Test func whileFollowingOnlyTheMasterMovesAndTheAnswerBringsTheLeadersChannels() throws {
        var phone = Phone.connected(3)
        try phone.answerMixes()
        try phone.receive(followingState("Ana", level: 70))
        let sent = phone.sent.count

        phone.apply(
            .channelLevelChanged(channel: 0, level: 40), .channelMuteChanged(channel: 1, muted: true))

        #expect(phone.sent.count == sent)

        phone.apply(.masterVolumeChanged(level: 50))

        #expect(try phone.lastMix().ints("channels") == [70, 70, 70])
        #expect(try phone.lastMix().int("volume_percent") == 50)

        // The desk keeps the leader's channels (moved on meanwhile) and the answer carries them.
        var answer = followingState("Ana", level: 65)
        answer["type"] = "mix_ack"
        answer["seq"] = try phone.lastMix().int("seq")
        answer["volume_percent"] = 50
        try phone.receive(answer)

        #expect(phone.session.mix.levels == [65, 65, 65])
        #expect(phone.session.mix.masterVolume == 50)
    }

    /// With the opening mix's lists checked as `aConfigArrivingBeforeTheOpeningMixIsSent…` checks
    /// them; that test's own subject, a `config` landing before the queued opening mix is sent, is
    /// the transport's (T6).
    @Test func onTheFirstConnectThePhoneShowsTheMixTheServerAlreadyHolds() throws {
        var phone = Phone()
        phone.apply(.sessionStarted)
        phone.opened()
        phone.apply(.configReceived(bandConfig))

        try #require(phone.sent.count == 1)
        let opening = try WireMix(phone.sent[0])
        #expect(opening.ints("channels") == [])
        #expect(opening.ints("pans") == [])
        #expect(opening.bools("mutes") == [])
        #expect(opening.bools("solos") == [])
        #expect(opening.ints("group_levels") == [])
        // Not in Android's test: the master values are the phone's own, so they go out as they are.
        #expect(opening.int("volume_percent") == 100)
        #expect(opening.int("max_level_percent") == 100)

        try phone.receive(
            answerKeepingTheServersMix(opening, levels: [40, 50, 60], groupLevel: 70))

        #expect(phone.session.mix.levels == [40, 50, 60])
        #expect(phone.session.mix.groupLevels == [70])
        #expect(phone.sent.count == 1)
    }

    /// Android also checks the receive thread's copy (`receiver.mixState`), which is the app's (T6).
    @Test func masterChangesReachTheAudioThreadAndTheServer() throws {
        var phone = Phone.connected()

        phone.apply(
            .masterVolumeChanged(level: 40), .levelCapChanged(level: 80), .masterMuteChanged(muted: true))

        let last = try phone.lastMix()
        #expect(last.int("volume_percent") == 40)
        #expect(last.int("max_level_percent") == 80)
        #expect(last.object["muted"] as? Bool == true)
    }

    // MARK: MonitorViewModelTest.kt, More of me

    /// Android also checks the channel it stored (`store.channel`), which is the app's (T6).
    @Test func moreOfMeLiftsTheMusiciansChannelOnlyInWhatIsSent() throws {
        var phone = Phone.connected(2)
        phone.apply(.channelLevelChanged(channel: 0, level: 80))
        let sent = phone.sent.count

        phone.apply(.moreOfMeToggled(on: true))

        #expect(phone.session.moreOfMe.notice == .needsChannel)
        #expect(phone.sent.count == sent)

        phone.apply(.musicianChannelChosen(1), .moreOfMeToggled(on: true))

        #expect(try phone.lastMix().ints("channels") == [40, 100])
        #expect(phone.session.mix.levels == [80, 100])

        phone.apply(.moreOfMeToggled(on: false))

        #expect(try phone.lastMix().ints("channels") == [80, 100])
    }

    /// Android also checks the channel it stored (`store.channel`), which is the app's (T6).
    @Test func choosingNoChannelTurnsMoreOfMeOffAndResendsTheMix() throws {
        var phone = Phone.connected(2)
        phone.apply(.musicianChannelChosen(0), .moreOfMeToggled(on: true))
        #expect(try phone.lastMix().ints("channels") == [100, 50])

        phone.apply(.musicianChannelChosen(nil))

        #expect(!phone.session.moreOfMe.isEnabled)
        #expect(try phone.lastMix().ints("channels") == [100, 100])
    }

    // MARK: MonitorViewModelTest.kt, a mix the desk pushed

    /// Android also checks the receive thread's copy (`receiver.mixState`), which is the app's (T6).
    @Test func aPushedMixIsShownAndHeardWithoutBeingSentBack() throws {
        var phone = Phone.connected(2)
        try phone.answerMixes()
        let sent = phone.sent.count

        try phone.receive(mixState(levels: [80, 70], volume: 50, cap: 90, muted: true))

        #expect(phone.session.mix.levels == [80, 70])
        #expect(phone.session.mix.masterVolume == 50)
        #expect(phone.session.mix.effectiveCap == 90)
        #expect(phone.session.mix.masterMuted)
        #expect(phone.sent.count == sent)
    }

    /// Android also checks the channel it stored (`store.channel`), which is the app's (T6).
    @Test func aPushDuringMoreOfMeTurnsItOffWithoutSendingAndTheNextMoveSendsThePushedMix() throws {
        var phone = Phone.connected(2)
        phone.apply(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        try phone.answerMixes()
        let sent = phone.sent.count

        try phone.receive(mixState(levels: [80, 70]))

        #expect(!phone.session.moreOfMe.isEnabled)
        #expect(phone.session.moreOfMe.notice == .mixChangedElsewhere)
        #expect(phone.sent.count == sent)

        // A fader still moving when the push landed sends its own value with the pushed mix around
        // it; the desk keeps whichever whole mix it got last.
        phone.apply(.channelLevelChanged(channel: 1, level: 30))

        #expect(try phone.lastMix().ints("channels") == [80, 30])
    }

    @Test func theServerSendingBackTheMixThePhoneSentChangesNothing() throws {
        var phone = Phone.connected(2)
        phone.apply(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        try phone.answerMixes()
        let before = phone.session
        let sent = phone.sent.count
        // Not in Android's test: the mix sent last is lifted, so hearing it back is not vacuous.
        try #require(phone.session.moreOfMe.isEnabled && sent == 2)

        // After missing events the desk re-sends the whole mix it holds, which is the phone's own.
        var resent = try phone.lastMix().object
        resent["type"] = "mix_state"
        try phone.receive(resent)

        #expect(phone.session == before)
        #expect(phone.sent.count == sent)
    }

    // MARK: MonitorViewModelTest.kt, the desk's answers to the phone's mixes

    @Test func aPushOlderThanAMixStillInFlightGivesWayToTheAnswerToThatMix() throws {
        var phone = Phone.connected(2)
        try phone.answerMixes()
        phone.apply(.channelLevelChanged(channel: 0, level: 40))
        let sent = phone.sent.count

        // The desk pushed its mix before the phone's newer one reached it; the gesture has ended.
        try phone.receive(mixState(levels: [80, 70]))

        #expect(phone.session.mix.levels == [40, 100])

        try phone.answerMixes()

        #expect(phone.session.mix.levels == [40, 100])
        #expect(phone.sent.count == sent)
    }

    /// Android also checks the receive thread's copy (`receiver.mixState`), which is the app's (T6).
    @Test func withTwoMixesInFlightOnlyTheAnswerToTheNewestIsTaken() throws {
        var phone = Phone.connected(2)
        try phone.answerMixes()
        phone.apply(
            .channelLevelChanged(channel: 0, level: 40), .channelLevelChanged(channel: 0, level: 50))
        try #require(phone.sent.count >= 2)
        let first = try WireMix(phone.sent[phone.sent.count - 2]).int("seq")
        let second = try WireMix(phone.sent[phone.sent.count - 1]).int("seq")
        let sent = phone.sent.count

        try phone.receive(mixState(levels: [40, 100], type: "mix_ack", seq: first))

        #expect(phone.session.mix.levels == [50, 100])

        // What the desk answers to the newest mix is what it plays, so the phone takes it.
        try phone.receive(mixState(levels: [50, 60], volume: 70, type: "mix_ack", seq: second))

        #expect(phone.session.mix.levels == [50, 60])
        #expect(phone.session.mix.masterVolume == 70)
        #expect(phone.sent.count == sent)
    }

    @Test func anAnswerThatIsTheMixThePhonePlaysChangesNothing() throws {
        var phone = Phone.connected(2)
        phone.apply(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        let before = phone.session
        let sent = phone.sent.count
        // Not in Android's test: a mix lifted by More of me is on its way, so its answer below is
        // not vacuous.
        try #require(!phone.session.sync.isSettled)

        // The answers carry the mixes as sent, the last one lifted by More of me.
        try phone.answerMixes()

        #expect(phone.session.mix == before.mix)
        #expect(phone.session.moreOfMe == before.moreOfMe)
        #expect(phone.session.moreOfMe.isEnabled)
        #expect(phone.sent.count == sent)
    }

    @Test func anErrorWithoutANumberNeverMakesAnOlderAnswerTheNewest() throws {
        var phone = Phone.connected(2)
        try phone.answerMixes()
        phone.apply(
            .channelLevelChanged(channel: 0, level: 40), .channelLevelChanged(channel: 0, level: 50))
        try #require(phone.sent.count >= 2)
        var first = try WireMix(phone.sent[phone.sent.count - 2]).object
        let second = try WireMix(phone.sent[phone.sent.count - 1]).int("seq")

        // An error answering a stats report, then the answer to the first mix, then a push: the
        // second mix is still on its way, so none of them is the desk's latest word.
        try phone.receive(["type": "error", "message": "no UDP target configured for client IP"])
        first["type"] = "mix_ack"
        try phone.receive(first)
        try phone.receive(mixState(levels: [80, 70]))

        #expect(phone.session.mix.levels == [50, 100])

        try phone.receive(mixState(levels: [50, 60], type: "mix_ack", seq: second))

        #expect(phone.session.mix.levels == [50, 60])
    }
}

// MARK: - Fixtures, as `MonitorViewModelTest.kt` names them

/// A mix report of `type` as the desk writes it (`mix_state` when the mix changed elsewhere,
/// `mix_ack` answering the phone's mix number `seq`): its full 32-channel and 16-group tables, with
/// `levels` on the first channels and the rest neutral.
private func mixState(
    levels: [Int], volume: Int = 100, cap: Int = 100, muted: Bool = false,
    type: String = "mix_state", seq: Int? = nil
) -> [String: Any] {
    var object: [String: Any] = [
        "type": type,
        "volume_percent": volume,
        "max_level_percent": cap,
        "muted": muted,
        "channels": (0..<32).map { $0 < levels.count ? levels[$0] : 100 },
        "pans": (0..<32).map { $0 % 2 == 0 ? 0 : 100 },
        "mutes": Array(repeating: false, count: 32),
        "solos": Array(repeating: false, count: 32),
        "group_levels": Array(repeating: 100, count: 16),
    ]
    object["seq"] = seq
    return object
}

/// A `mix_state` of neutral levels carrying the engineer's `limit` and `locked` channels.
private func engineerState(_ limit: Int, locked: Int...) -> [String: Any] {
    var object = mixState(levels: [])
    object["engineer_limit_percent"] = limit
    object["locked"] = (0..<32).map { locked.contains($0) }
    return object
}

/// A `mix_state` with every channel at `level` that names whose mix the phone follows.
private func followingState(_ name: String, level: Int = 100) -> [String: Any] {
    var object = mixState(levels: Array(repeating: level, count: 32))
    object["following"] = name
    return object
}

/// The desk announces a keys pair on channels 0 and 1 and a voice on channel 2: Android's
/// `announcePair`.
private let announcePair = MixEvent.configReceived(
    config(3, [], [info("Keys", .keys, link: true), info("", .keys), info("Vox", .voice)]))

/// Three channels and a group of the first two.
private let bandConfig = config(3, [group("Band", [0, 1])])

/// The desk's answer to the phone's `sent` mix when it kept its own channels and groups: every
/// channel from `levels` on and the first group at `groupLevel`.
private func answerKeepingTheServersMix(_ sent: WireMix, levels: [Int], groupLevel: Int)
    -> [String: Any]
{
    var object = mixState(levels: levels, type: "mix_ack", seq: sent.int("seq"))
    object["group_levels"] = (0..<16).map { $0 == 0 ? groupLevel : 100 }
    return object
}
