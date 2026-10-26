import Testing

@testable import BandEarScreens

/// The module's suites live beside this file, one per screen area, each porting its Android tests.
@Suite("Screens module")
struct BandEarScreensTests {
    @Test func theModuleNamesItsPurpose() {
        #expect(String(describing: BandEarScreens.self) == "BandEarScreens")
    }
}
