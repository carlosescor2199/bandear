import BandEarMix
import BandEarProtocol
import BandEarScreens
import Testing

/// Android's `channel(index)` (`ChannelSheetTextTest.kt:11-19`): an unnamed mono channel at level 80,
/// centred, neither muted nor soloed, with what a case changes through `copy`. iOS derives every
/// strip from a mix, so this builds the mix that shows such a strip and reads the strip back.
private func channel(
    _ index: Int, label: String = "", kind: ChannelKind = .other, pairedWith: Int? = nil,
    locked: Bool = false, following: String? = nil
) throws -> ChannelStrip {
    let count = (pairedWith ?? index) + 1
    var mix = MixState()
    mix.sourceChannels = count
    mix.levels = Array(repeating: 80, count: count)
    mix.pans = Array(repeating: 50, count: count)
    mix.channelDetails = (0..<count).map { channel in
        SourceChannel(
            name: channel == index ? label : "", kind: channel == index ? kind : .other,
            isHidden: false, isLinkedToNext: channel == index && pairedWith != nil)
    }
    mix.engineer = EngineerControls(lockedChannels: locked ? [index] : [], following: following)
    return try #require(mix.strips.first { $0.index == index })
}

/// The Android client's `ChannelSheetTextTest` (`apps/android/app/src/test/java/com/bandear/android/
/// ui/screens/ChannelSheetTextTest.kt`), case for case.
@Suite("Channel sheet words: Android parity")
struct ChannelSheetWordsParityTests {

    /// `ChannelSheetTextTest.kt:21-29`
    @Test func theOpenSheetFindsItsChannelByIndexNotByPosition() throws {
        // A strip reports its channel's index; the list's positions need not match it.
        let two = try channel(2)
        let five = try channel(5)
        let channels = [two, five]

        #expect(ChannelSheetWords.sheetStrip(in: channels, open: 5) == five)
        #expect(ChannelSheetWords.sheetStrip(in: channels, open: 2) == two)
        #expect(ChannelSheetWords.sheetStrip(in: channels, open: 1) == nil)
    }

    /// `ChannelSheetTextTest.kt:31-37`
    @Test func aChannelTheServerNoLongerAnnouncesHasNoSheet() throws {
        let channels = try (0..<4).map { try channel($0) }

        #expect(ChannelSheetWords.sheetStrip(in: channels, open: 6) == nil)
        #expect(ChannelSheetWords.sheetStrip(in: [], open: 0) == nil)
    }

    /// `ChannelSheetTextTest.kt:39-42`
    @Test func noSheetIsOpenWithoutAChannel() throws {
        #expect(ChannelSheetWords.sheetStrip(in: try (0..<4).map { try channel($0) }, open: nil) == nil)
    }

    /// `ChannelSheetTextTest.kt:44-49`
    @Test func theLabelButtonIsNamedForItsChannel() throws {
        // Channel indexes are 0-based; the strip reads "CH 3" for index 2.
        #expect(ChannelSheetWords.settingsLabel(for: try channel(2)) == "Channel 3 settings")
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(0, label: "Vox", kind: .voice))
                == "Vox settings")
    }

    /// `ChannelSheetTextTest.kt:51-56`
    @Test func aLockedChannelsLabelButtonSaysTheEngineerLockedIt() throws {
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(2, locked: true))
                == "Channel 3 settings, locked by the engineer")
        #expect(
            ChannelSheetWords.settingsLabel(
                for: try channel(3, label: "Keys", pairedWith: 4, locked: true))
                == "Keys stereo pair settings, locked by the engineer")
        #expect(ChannelSheetWords.lockedNote == "Locked by the engineer")
    }

    /// `ChannelSheetTextTest.kt:58-65`
    @Test func aFollowersLabelButtonSaysWhoseMixItFollows() throws {
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(2, following: "Ana"))
                == "Channel 3 settings, following Ana's mix")
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(2, locked: true, following: "Ana"))
                == "Channel 3 settings, locked by the engineer, following Ana's mix")
    }

    /// `ChannelSheetTextTest.kt:67-71`
    @Test func aStereoPairsLabelButtonSaysItIsAPair() throws {
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(3, label: "Keys", pairedWith: 4))
                == "Keys stereo pair settings")
        #expect(
            ChannelSheetWords.settingsLabel(for: try channel(3, pairedWith: 4))
                == "Channel 4 stereo pair settings")
    }

    /// `ChannelSheetTextTest.kt:73-78`
    @Test func theLabelButtonReportsThePanItShows() {
        #expect(ChannelSheetWords.panState(pan: 35) == "Pan left 30")
        #expect(ChannelSheetWords.panState(pan: 50) == "Pan centre")
        #expect(ChannelSheetWords.panState(pan: 100) == "Pan right 100")
    }

    /// `ChannelSheetTextTest.kt:80-87`
    @Test func aStereoPairsPanIsABalance() {
        #expect(ChannelSheetWords.panState(pan: 35, stereo: true) == "Balance left 30")
        #expect(ChannelSheetWords.panState(pan: 50, stereo: true) == "Balance centre")
        #expect(ChannelSheetWords.panTitle(stereo: true) == "BALANCE")
        #expect(ChannelSheetWords.panTitle(stereo: false) == "PAN")
        #expect(ChannelSheetWords.panDescription(label: "Keys", stereo: true) == "Keys balance")
    }

    /// `ChannelSheetTextTest.kt:89-92`
    @Test func thePanControlIsNamedForItsChannel() {
        #expect(ChannelSheetWords.panDescription(label: "CH 3") == "CH 3 pan")
    }

    /// `ChannelSheetTextTest.kt:94-100`
    @Test func theSheetsKeysSayWhetherTheyAreOn() {
        #expect(ChannelSheetWords.muteKeyText(muted: false) == "Mute")
        #expect(ChannelSheetWords.muteKeyText(muted: true) == "Muted")
        #expect(ChannelSheetWords.soloKeyText(soloed: false) == "Solo")
        #expect(ChannelSheetWords.soloKeyText(soloed: true) == "Soloed")
    }
}
