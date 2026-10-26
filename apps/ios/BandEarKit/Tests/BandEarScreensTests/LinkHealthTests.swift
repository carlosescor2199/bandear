import Testing

@testable import BandEarScreens

/// What Android leaves untested about the link-health words: four of the six meanings
/// (`LinkHealth.kt:22-30`), which its tests reach only through two descriptions.
@Suite("Link health words")
struct LinkHealthTests {
    @Test func everyLevelMeansWhatItMeansOnAndroid() {
        #expect(LinkHealth.good.meaning == "Clean audio.")
        #expect(LinkHealth.warning.meaning == "An underrun or a little loss.")
        #expect(LinkHealth.bad.meaning == "Heavy loss or dropouts.")
        #expect(LinkHealth.silent.meaning == "No audio received.")
        #expect(LinkHealth.lost.meaning == "Reports stopped arriving.")
        #expect(LinkHealth.unknown.meaning == "No report yet.")
    }
}
