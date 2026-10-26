import BandEarDiscovery
import BandEarScreens
import Testing

/// The Mix screen's words that no Android test pins, and how they read the iOS session's desk.
@Suite("Mix words")
struct MixWordsTests {

    /// Android names a desk by its address because its beacon carries no name
    /// (`MonitorUiState.kt:69-73`); a Bonjour desk has one, and the header says it as the list does.
    @Test func aBonjourDeskIsNamedAsTheListNamesIt() {
        let desk = DiscoveredDesk(
            name: "BandEar on Studio",
            endpoint: .bonjour(name: "BandEar on Studio", type: "_bandear._tcp", domain: "local."),
            sampleRate: 48_000)

        #expect(MixWords.headerServerText(desk) == "BandEar on Studio")
    }

    /// `MixView` (`MixScreenText.kt:10-13`): the two rows the Mix screen switches between.
    @Test func theRowChoiceNamesChannelsAndGroups() {
        #expect(MixRow.allCases == [.channels, .groups])
        #expect(MixRow.channels.label == "Channels")
        #expect(MixRow.groups.label == "Groups")
    }

    /// `ChannelRow` and `GroupRow` with nothing to show (`MixStrips.kt:95,120`).
    @Test func anEmptyRowSaysWhatItWaitsFor() {
        #expect(MixWords.waitingForChannels == "Waiting for the server to announce its channels.")
        #expect(MixWords.noGroups == "The server has no groups.")
    }

    /// `leaderName` (`MonitorUiState.kt:426`): the leader trimmed, or "another musician".
    @Test func theLeaderIsNamedTrimmedOrAsAnotherMusician() {
        #expect(MixWords.leaderName("  Ana ") == "Ana")
        #expect(MixWords.leaderName("") == "another musician")
        #expect(MixWords.followingTitle(" Ana ") == "Following Ana's mix")
    }
}
