import Testing

@testable import BandEarScreens

/// The Android client's own tests for which screen shows, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/screens/AppScreenTest.kt`.
@Suite("App screen: Android parity")
struct AppScreenParityTests {

    // AppScreenTest.kt:10
    @Test func withNoSessionTheConnectScreenShows() {
        #expect(AppScreen(sessionRunning: false, tab: .mix, manualAddressOpen: false) == .connect)
    }

    // AppScreenTest.kt:15
    @Test func aTabLeftOpenByTheLastSessionDoesNotHideTheConnectScreen() {
        // Disconnect from Settings: the session ends with the Settings tab still selected.
        #expect(
            AppScreen(sessionRunning: false, tab: .settings, manualAddressOpen: false) == .connect)
    }

    // AppScreenTest.kt:24
    @Test func theManualAddressOpensFromTheConnectScreen() {
        #expect(
            AppScreen(sessionRunning: false, tab: .mix, manualAddressOpen: true) == .manualAddress)
    }

    // AppScreenTest.kt:32
    @Test func aRunningSessionShowsTheSelectedTab() {
        #expect(AppScreen(sessionRunning: true, tab: .mix, manualAddressOpen: false) == .mix)
        #expect(AppScreen(sessionRunning: true, tab: .mixes, manualAddressOpen: false) == .mixes)
        #expect(
            AppScreen(sessionRunning: true, tab: .settings, manualAddressOpen: false) == .settings)
    }

    // AppScreenTest.kt:42
    @Test func aSessionStartedFromTheManualAddressShowsTheTabs() {
        #expect(AppScreen(sessionRunning: true, tab: .mix, manualAddressOpen: true) == .mix)
    }

    // AppScreenTest.kt:47
    @Test func onlyTheConnectedScreensShowTheTabBar() {
        #expect(AppScreen.mix.showsTabs)
        #expect(AppScreen.mixes.showsTabs)
        #expect(AppScreen.settings.showsTabs)
        #expect(!AppScreen.connect.showsTabs)
        #expect(!AppScreen.manualAddress.showsTabs)
    }
}
