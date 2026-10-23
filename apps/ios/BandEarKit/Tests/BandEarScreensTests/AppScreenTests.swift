import Testing

@testable import BandEarScreens

/// What Android leaves untested about the screens: the tab bar's words, which it keeps on `MainTab`
/// (`ui/components/BottomTabBar.kt:35-39`).
@Suite("App screen: tabs")
struct AppScreenTests {
    @Test func theTabsAreNamedAsOnAndroidInTheBarsOrder() {
        #expect(AppTab.allCases.map(\.label) == ["Mix", "Mixes", "Settings"])
    }
}
