package com.bandear.android.ui.screens

import com.bandear.android.ui.components.MainTab
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AppScreenTest {
    @Test
    fun withNoSessionTheConnectScreenShows() {
        assertEquals(AppScreen.CONNECT, appScreenFor(sessionRunning = false, tab = MainTab.MIX, manualAddressOpen = false))
    }

    @Test
    fun aTabLeftOpenByTheLastSessionDoesNotHideTheConnectScreen() {
        // Disconnect from Settings: the session ends with the Settings tab still selected.
        assertEquals(
            AppScreen.CONNECT,
            appScreenFor(sessionRunning = false, tab = MainTab.SETTINGS, manualAddressOpen = false),
        )
    }

    @Test
    fun theManualAddressOpensFromTheConnectScreen() {
        assertEquals(
            AppScreen.MANUAL_ADDRESS,
            appScreenFor(sessionRunning = false, tab = MainTab.MIX, manualAddressOpen = true),
        )
    }

    @Test
    fun aRunningSessionShowsTheSelectedTab() {
        assertEquals(AppScreen.MIX, appScreenFor(sessionRunning = true, tab = MainTab.MIX, manualAddressOpen = false))
        assertEquals(AppScreen.MIXES, appScreenFor(sessionRunning = true, tab = MainTab.MIXES, manualAddressOpen = false))
        assertEquals(
            AppScreen.SETTINGS,
            appScreenFor(sessionRunning = true, tab = MainTab.SETTINGS, manualAddressOpen = false),
        )
    }

    @Test
    fun aSessionStartedFromTheManualAddressShowsTheTabs() {
        assertEquals(AppScreen.MIX, appScreenFor(sessionRunning = true, tab = MainTab.MIX, manualAddressOpen = true))
    }

    @Test
    fun onlyTheConnectedScreensShowTheTabBar() {
        assertTrue(AppScreen.MIX.showsTabs)
        assertTrue(AppScreen.MIXES.showsTabs)
        assertTrue(AppScreen.SETTINGS.showsTabs)
        assertFalse(AppScreen.CONNECT.showsTabs)
        assertFalse(AppScreen.MANUAL_ADDRESS.showsTabs)
    }
}
