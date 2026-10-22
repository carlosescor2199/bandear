import BandEarProtocol
import Testing

@testable import BandEarMix

/// The Android client's `MonitorReducerTest.kt` tests of the mix, ported case for case
/// (`apps/android/app/src/test/java/com/bandear/android/ui/state/`), unit by unit as the mix is
/// built. Android reaches every state through events; until T5b brings the reducer, the states
/// here are the ones a first `config` builds (`firstConfig`), and T5b's ports prove the reducer
/// builds exactly those.
@Suite("Mix: Android parity")
struct MixReducerParityTests {

    // MARK: T5a, the channel layout

    @Test func announcedDetailsNameTheStripsShowTheirKindsAndJoinPairs() throws {
        let strips = firstConfig(channels: 5, details: band).strips
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
        let mix = firstConfig(channels: 3, groups: [group("Band", [0, 1])])

        #expect(mix.channelDetails == nil)
        #expect(mix.strips.map(\.label) == ["CH 1", "CH 2", "CH 3"])
        #expect(mix.strips.map(\.kind) == [nil, nil, nil])
        #expect(mix.strips.map(\.pairedWith) == [nil, nil, nil])
        #expect(mix.groupFaders.map(\.members) == [["CH 1", "CH 2"]])
    }

    /// Android goes on to choose channels through `MusicianChannelChosen`; that half is More of me
    /// and is ported with it in T5d.
    @Test func onlyShownStripsCanBeChosenAndAPairIsChosenByItsFirstChannel() {
        let mix = firstConfig(channels: 5, details: band)

        #expect(mix.musicianChoices.map(\.index) == [0, 1, 4])
        #expect(
            mix.musicianChoices == [
                ChannelChoice(index: 0, label: "Vox"), ChannelChoice(index: 1, label: "Keys"),
                ChannelChoice(index: 4, label: "CH 5"),
            ])
    }

    @Test func aGroupNamesItsShownStripsAPairOnce() {
        let mix = firstConfig(channels: 5, groups: [group("Band", [0, 1, 2, 3])], details: band)

        #expect(mix.groupFaders.map(\.members) == [["Vox", "Keys"]])
    }
}
