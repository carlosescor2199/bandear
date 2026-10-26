import BandEarProtocol
import BandEarScreens
import Testing

/// What each kind of channel is called (`ChannelKind.label`, `ClientMixState.kt:10-21`), which no
/// Android test pins word for word.
@Suite("Kind words")
struct KindWordsTests {

    @Test func everyKindIsNamedAsAndroidNamesIt() {
        #expect(
            ChannelKind.allCases.map(KindWords.label) == [
                "Voice", "Guitar", "Bass", "Keys", "Drums", "Percussion", "Horns", "Strings",
                "Playback", "Click", "Other",
            ])
    }

    /// Adapted from `KindIconsTest.kt:10-14`, which reads the labels through the icons' names.
    @Test func everyKindHasALabelOfItsOwn() {
        #expect(Set(ChannelKind.allCases.map(KindWords.label)).count == ChannelKind.allCases.count)
    }
}
