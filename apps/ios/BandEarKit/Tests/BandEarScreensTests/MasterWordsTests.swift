import BandEarScreens
import Testing

/// The Master card's words that no Android test pins (`MixStrips.kt:348-401`).
@Suite("Master words")
struct MasterWordsTests {

    @Test func theMasterCardsWordsAreAndroids() {
        #expect(MasterWords.title == "MASTER")
        #expect(MasterWords.levelCapLead == "Level cap ")
        #expect(MasterWords.levelCapValue(90) == "90%")
        #expect(MasterWords.levelCapTrail == " · protects your ears")
        #expect(MasterWords.faderDescription(muted: false) == "Master level")
        #expect(MasterWords.faderDescription(muted: true) == "Master level, muted")
        #expect(MasterWords.muteDescription == "Mute master")
    }
}
