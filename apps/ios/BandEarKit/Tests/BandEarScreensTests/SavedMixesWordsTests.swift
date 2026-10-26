import BandEarScreens
import Testing

/// The Mixes tab's copy that shows before any mix is saved (`SavedMixesScreen.kt:90-92,357`,
/// `MonitorUiState.kt:384`), which no Android test pins.
@Suite("Saved mixes words")
struct SavedMixesWordsTests {

    @Test func theMixesTabsCopyIsAndroids() {
        #expect(SavedMixesWords.screenTitle == "Saved mixes")
        #expect(
            SavedMixesWords.screenIntro
                == "Store your mix for a song or a set and bring it back with one tap.")
        #expect(SavedMixesWords.emptyTitle == "No saved mixes yet")
        #expect(
            SavedMixesWords.emptyDetail
                == "Save the current mix above, then bring it back here with one tap.")
    }
}
