import BandEarMix
import BandEarProtocol
import BandEarScreens
import Testing

/// Three announced channels, "Keys" paired with an unnamed second, and an unnamed third.
private func mix(locked: Set<Int> = [], following: String? = nil) -> MixState {
    var mix = MixState()
    mix.sourceChannels = 3
    mix.levels = [80, 80, 80]
    mix.channelDetails = [
        SourceChannel(name: "Keys", kind: .keys, isHidden: false, isLinkedToNext: true),
        SourceChannel(name: "", kind: .other, isHidden: false, isLinkedToNext: false),
        SourceChannel(name: "", kind: .other, isHidden: false, isLinkedToNext: false),
    ]
    mix.engineer = EngineerControls(lockedChannels: locked, following: following)
    return mix
}

private func strip(_ channel: Int, in mix: MixState) throws -> ChannelStrip {
    try #require(mix.strips.first { $0.index == channel })
}

/// The channel sheet's words that no Android test pins (`ChannelSheet.kt:141-224`).
@Suite("Channel sheet words")
struct ChannelSheetWordsTests {

    @Test func theSheetsCaptionsAreAndroids() {
        #expect(ChannelSheetWords.levelTitle == "LEVEL")
        #expect(ChannelSheetWords.stereoTag == "Stereo pair")
        #expect(ChannelSheetWords.yourChannel == "Your channel")
        #expect(ChannelSheetWords.noGroup == "No group")
        #expect(ChannelSheetWords.done == "Done")
    }

    /// The sheet's pan control (`ChannelSheet.kt:147`): the channel's pan or balance, held as the
    /// strip is held.
    @Test func thePanControlSaysWhoseItIsAndWhyItIsHeld() throws {
        #expect(ChannelSheetWords.panControlDescription(for: try strip(2, in: mix())) == "CH 3 pan")
        #expect(
            ChannelSheetWords.panControlDescription(
                for: try strip(0, in: mix(locked: [1], following: "Ana")))
                == "Keys balance, locked by the engineer, following Ana's mix")
    }
}
