import BandEarProtocol
import Testing

@testable import BandEarMix

/// The Android client's tests of the More of me lift (`MixSnapshotTest.kt:35-81` and
/// `GroupSnapshotTest.kt:44-55`), ported case for case.
///
/// Android lifts a whole `MixSnapshot` and returns a new one. The lift here takes and returns the
/// levels alone (``MoreOfMe/liftedLevels(_:musicianChannels:lockedChannels:)``), so where Android
/// checks that the other values come through untouched, the port checks it on the session: the desk
/// reporting the lifted levels back with every other value as stored is the mix the phone plays,
/// and changes nothing.
@Suite("More of me: Android parity")
struct MoreOfMeParityTests {
    /// Android's `sample()` gains.
    private let sampleGains = [80, 100, 40]

    @Test func moreOfMePushesTheMusicianChannelToFullAndHalvesTheRest() throws {
        let derived = MoreOfMe.liftedLevels(sampleGains, musicianChannels: [1], lockedChannels: [])
        try #require(derived.count == 3)

        #expect(derived[0] == 40)
        #expect(derived[1] == 100)
        #expect(derived[2] == 20)
    }

    /// Android lifts the sample and then checks the sample. The stored mix here is the session's, so
    /// this turns More of me on and checks the mix the session holds.
    @Test func moreOfMeLeavesEveryStoredValueUntouched() throws {
        let on = sample().after(.musicianChannelChosen(1), .moreOfMeToggled(on: true))
        // Not in Android's test: without More of me on, nothing here could have been lifted.
        try #require(on.moreOfMe.isEnabled)

        #expect(on.mix.levels == [80, 100, 40])
        #expect(on.mix.pans == [0, 50, 100])
        #expect(on.mix.mutes == [false, true, false])
        #expect(on.mix.solos == [true, false, false])
        #expect(on.mix.masterVolume == 70)
        #expect(on.mix.levelCap == 90)
        #expect(on.mix.masterMuted)
    }

    @Test func moreOfMeKeepsTheMasterControlsAndFlags() {
        let on = sample().after(.musicianChannelChosen(0), .moreOfMeToggled(on: true))
        let derived = on.played()

        #expect(derived == [100, 50, 20])
        // The pans, mutes, solos and masters the phone plays are the stored ones: reported back with
        // the lifted levels, the mix is the one the phone plays and nothing changes.
        #expect(on.after(.mixPushed(report(of: on.mix, gains: derived))) == on)
    }

    /// Android's `null`: no lift. Every Android caller then plays and sends the stored levels
    /// (`ControlChannel.kt:423`, `MonitorReducer.kt:519`), which is what the lift returns here.
    @Test func moreOfMeDoesNothingWithoutADesignatedChannel() {
        #expect(
            MoreOfMe.liftedLevels(sampleGains, musicianChannels: [], lockedChannels: []) == sampleGains)
    }

    @Test func moreOfMeDoesNothingForAChannelOutsideTheMix() {
        #expect(
            MoreOfMe.liftedLevels(sampleGains, musicianChannels: [9], lockedChannels: []) == sampleGains)
    }

    // `GroupSnapshotTest.kt`

    @Test func moreOfMeKeepsTheGroupLevelsUntouched() {
        let on = connectedWith(2, [group("A", [0]), group("B", [1])]).after(
            .channelLevelChanged(channel: 0, level: 80), .groupLevelChanged(group: 0, level: 30),
            .groupLevelChanged(group: 1, level: 60), .musicianChannelChosen(1),
            .moreOfMeToggled(on: true))
        let derived = on.played()

        #expect(derived == [40, 100])
        #expect(on.mix.groupLevels == [30, 60])
        // Reported back with the stored group levels, the mix is the one the phone plays.
        #expect(on.after(.mixPushed(report(of: on.mix, gains: derived))) == on)
    }
}

/// Android's `sample()`, reached through events on a running three-channel session: levels 80, 100
/// and 40; pans 0, 50 and 100; the middle channel muted and the first soloed; the master at 70, the
/// cap at 90, muted.
private func sample() -> MixSession {
    connectedWith(3).after(
        .channelLevelChanged(channel: 0, level: 80), .channelLevelChanged(channel: 2, level: 40),
        .channelPanChanged(channel: 1, pan: 50), .channelPanChanged(channel: 2, pan: 100),
        .channelMuteChanged(channel: 1, muted: true), .channelSoloChanged(channel: 0, soloed: true),
        .masterVolumeChanged(level: 70), .levelCapChanged(level: 90), .masterMuteChanged(muted: true))
}
