import BandEarDiscovery
import BandEarMix
import BandEarScreens
import Testing

/// A mix whose phone cap is `cap` and whose engineer set `limit`. Android hands the two numbers to
/// the master card's words; iOS reads them, and the lower of the two, from the mix itself.
private func mix(cap: Int, limit: Int) -> MixState {
    var mix = MixState()
    mix.levelCap = cap
    mix.engineer.limitPercent = limit
    return mix
}

/// The session's desk in Android's test, `ServerEndpoint("192.168.1.34", 50001)`, whose label is
/// its address and control port. An iOS desk is named by the name it is listed under.
private let server = DiscoveredDesk(
    name: "192.168.1.34:50001", endpoint: .manual(host: "192.168.1.34", controlPort: 50001),
    sampleRate: nil)

/// The Android client's `MixScreenTextTest` (`apps/android/app/src/test/java/com/bandear/android/
/// ui/screens/MixScreenTextTest.kt`), case for case.
///
/// Its five `connectionBanner` cases (`noBannerWhileConnectedOrConnecting` through
/// `aControlChannelThatGaveUpAsksForANewConnect`, lines 136-181) are the connection words' and are
/// ported with them.
@Suite("Mix words: Android parity")
struct MixWordsParityTests {

    /// `MixScreenTextTest.kt:14-18`
    @Test func aStripShowsItsLevelAsAPercentage() {
        #expect(StripWords.valueText(level: 72, muted: false) == "72%")
        #expect(StripWords.valueText(level: 0, muted: false) == "0%")
    }

    /// `MixScreenTextTest.kt:20-23`
    @Test func aMutedStripSaysMutedInWords() {
        #expect(StripWords.valueText(level: 72, muted: true) == "muted")
    }

    /// `MixScreenTextTest.kt:25-30`
    @Test func theMusiciansOwnStripIsAmberOnlyWhileMoreOfMeLiftsIt() {
        #expect(StripWords.fill(muted: false, mine: true, moreOfMeOn: true) == .accent)
        #expect(StripWords.fill(muted: false, mine: true, moreOfMeOn: false) == .idle)
        #expect(StripWords.fill(muted: false, mine: false, moreOfMeOn: true) == .idle)
    }

    /// `MixScreenTextTest.kt:32-36`
    @Test func aMutedStripIsGreyedEvenWhenItIsTheMusiciansOwn() {
        #expect(StripWords.fill(muted: true, mine: true, moreOfMeOn: true) == .muted)
        #expect(StripWords.fill(muted: true, mine: false, moreOfMeOn: false) == .muted)
    }

    /// `MixScreenTextTest.kt:38-44`
    @Test func aFaderTellsAScreenReaderWhoseChannelItIsAndWhetherItIsMuted() {
        #expect(StripWords.faderDescription(label: "CH 1", mine: false, muted: false) == "CH 1 level")
        #expect(
            StripWords.faderDescription(label: "CH 3", mine: true, muted: false)
                == "CH 3 level, your channel")
        #expect(
            StripWords.faderDescription(label: "CH 4", mine: false, muted: true) == "CH 4 level, muted")
        #expect(
            StripWords.faderDescription(label: "CH 3", mine: true, muted: true)
                == "CH 3 level, your channel, muted")
    }

    /// `MixScreenTextTest.kt:46-53`
    @Test func aLockedStripSaysSoInWords() {
        #expect(
            StripWords.faderDescription(label: "Vox", mine: false, muted: false, locked: true)
                == "Vox level, locked by the engineer")
        #expect(
            StripWords.faderDescription(label: "Vox", mine: true, muted: true, locked: true)
                == "Vox level, your channel, muted, locked by the engineer")
        #expect(StripWords.lockedLabel("Vox", locked: true) == "Vox, locked by the engineer")
        #expect(StripWords.lockedLabel("Vox", locked: false) == "Vox")
        #expect(StripWords.lockedTag == "Locked")
    }

    /// `MixScreenTextTest.kt:55-64`
    @Test func aFollowedMixIsNamedInWords() {
        #expect(MixWords.followingTitle("Ana") == "Following Ana's mix")
        #expect(MixWords.followingTitle(" ") == "Following another musician's mix")
        #expect(
            MixWords.followingDetail
                == "Your channels and groups follow that mix. Your master, level cap and mute are still yours.")
        #expect(StripWords.followingTag == "Following")
    }

    /// `MixScreenTextTest.kt:66-77`
    @Test func aFollowersControlsSayWhoseMixTheyFollow() {
        #expect(
            StripWords.faderDescription(label: "Vox", mine: false, muted: false, following: "Ana")
                == "Vox level, following Ana's mix")
        #expect(
            StripWords.faderDescription(
                label: "Vox", mine: false, muted: false, locked: true, following: "Ana")
                == "Vox level, locked by the engineer, following Ana's mix")
        #expect(
            StripWords.lockedLabel("Vox", locked: false, following: "Ana") == "Vox, following Ana's mix")
        #expect(
            StripWords.lockedLabel("Vox", locked: true, following: "Ana")
                == "Vox, locked by the engineer, following Ana's mix")
        #expect(StripWords.groupFaderDescription(name: "Band", following: nil) == "Band group level")
        #expect(
            StripWords.groupFaderDescription(name: "Band", following: "Ana")
                == "Band group level, following Ana's mix")
    }

    /// `MixScreenTextTest.kt:79-84`
    @Test func theEngineersLimitIsShownApartFromTheCapWithWhatTheMusicianGets() {
        #expect(MasterWords.engineerLimitText(for: mix(cap: 90, limit: 100)) == nil)
        #expect(
            MasterWords.engineerLimitText(for: mix(cap: 100, limit: 60))
                == "Limited to 60% by the engineer · at most 60%")
        #expect(
            MasterWords.engineerLimitText(for: mix(cap: 50, limit: 60))
                == "Limited to 60% by the engineer · at most 50%")
    }

    /// `MixScreenTextTest.kt:86-93`
    @Test func theCapControlSaysTheEngineersLimitAlsoApplies() {
        #expect(
            MasterWords.levelCapDescription(for: mix(cap: 90, limit: 100))
                == "Level cap, the loudest this phone plays")
        #expect(
            MasterWords.levelCapDescription(for: mix(cap: 90, limit: 60))
                == "Level cap, the loudest this phone plays. The engineer's limit of 60% also applies: at most 60%")
    }

    /// `MixScreenTextTest.kt:95-100`
    @Test func aGroupStripListsItsChannelsAsTheirStripsNameThem() {
        #expect(StripWords.groupMembersText(["CH 1", "CH 2"]) == "CH 1 · CH 2")
        #expect(StripWords.groupMembersText(["Vox", "Keys"]) == "Vox · Keys")
        #expect(StripWords.groupMembersText([]) == "No channels")
    }

    /// `MixScreenTextTest.kt:102-106`
    @Test func theSectionTitleCountsWhatTheRowShows() {
        #expect(MixWords.sectionTitle(.channels, count: 8) == "CHANNELS · 8")
        #expect(MixWords.sectionTitle(.groups, count: 4) == "GROUPS · 4")
    }

    /// `MixScreenTextTest.kt:108-111`
    @Test func anOverflowingRowSaysToSwipeForMore() {
        #expect(MixWords.swipeForMore == "Swipe for more")
    }

    /// `MixScreenTextTest.kt:108-114`
    @Test func theMasterSaysMutedInWords() {
        #expect(MasterWords.valueText(volume: 85, muted: false) == "85%")
        #expect(MasterWords.valueText(volume: 85, muted: true) == "muted")
        #expect(MasterWords.muteText(muted: false) == "Mute")
        #expect(MasterWords.muteText(muted: true) == "Muted")
    }

    /// `MixScreenTextTest.kt:116-122`
    @Test func theChannelPickerNamesTheChosenChannelOrNone() {
        #expect(ChannelPickerWords.text("CH 3") == "CH 3")
        #expect(ChannelPickerWords.text(nil) == "None")
        #expect(ChannelPickerWords.description("CH 3") == "My channel: CH 3. Tap to change")
        #expect(ChannelPickerWords.description(nil) == "My channel: none chosen. Tap to choose")
    }

    /// `MixScreenTextTest.kt:124-128`
    @Test func theHeaderFallsBackWhenNoNameIsSet() {
        #expect(MixWords.headerName("  Carlos D ") == "Carlos D")
        #expect(MixWords.headerName("   ") == "BandEar")
    }

    /// `MixScreenTextTest.kt:130-134`
    @Test func theHeaderNamesTheServerOfTheSession() {
        #expect(MixWords.headerServerText(server) == "192.168.1.34:50001")
        #expect(MixWords.headerServerText(nil) == "No server")
    }
}
