package com.bandear.android.ui.screens

import com.bandear.android.ui.components.MainTab

/**
 * The screens of the app. Only the three of a running session show the tabs: the bottom tab bar
 * in portrait, the navigation rail in landscape.
 */
enum class AppScreen(val showsTabs: Boolean) {
    /** Pick a found server; shows whenever no session runs. */
    CONNECT(showsTabs = false),

    /** The Settings form opened from Connect, to type a server's address. */
    MANUAL_ADDRESS(showsTabs = false),
    MIX(showsTabs = true),
    MIXES(showsTabs = true),
    SETTINGS(showsTabs = true),
}

/**
 * The screen to show. A running session shows its selected tab. Without one, Connect shows, or the
 * manual address form when the musician opened it from Connect; the tab a finished session left
 * selected never hides Connect.
 */
fun appScreenFor(sessionRunning: Boolean, tab: MainTab, manualAddressOpen: Boolean): AppScreen = when {
    sessionRunning -> when (tab) {
        MainTab.MIX -> AppScreen.MIX
        MainTab.MIXES -> AppScreen.MIXES
        MainTab.SETTINGS -> AppScreen.SETTINGS
    }
    manualAddressOpen -> AppScreen.MANUAL_ADDRESS
    else -> AppScreen.CONNECT
}
