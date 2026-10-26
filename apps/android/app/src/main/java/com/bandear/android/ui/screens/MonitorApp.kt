package com.bandear.android.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.tooling.preview.Preview
import com.bandear.android.ui.components.BottomTabBar
import com.bandear.android.ui.components.MainTab
import com.bandear.android.ui.components.NavigationRail
import com.bandear.android.ui.components.isLandscapeLayout
import com.bandear.android.ui.state.MonitorUiState
import com.bandear.android.ui.theme.BandEarTheme

/**
 * The whole app: Connect while no session runs, and the Mix, Mixes and Settings tabs while one
 * does, with the bottom bar under them in the portrait layout and the rail beside them in
 * landscape. Everything it shows comes from [state]; everything the musician does goes to
 * [actions].
 *
 * Each screen gets only the parts of [state] it shows. A part that did not change keeps its
 * instance (the reducer copies only what changes), so the screens it feeds are skipped when only
 * the receiver's statistics moved.
 *
 * The only state kept here is whether the manual address form was opened from Connect. The back
 * key closes it, and from the Mixes or Settings tab returns to Mix. The selected tab lives in
 * [state], so it outlasts the Activity being recreated when the phone turns.
 */
@Composable
fun MonitorApp(
    state: MonitorUiState,
    actions: MonitorActions,
    modifier: Modifier = Modifier,
    landscape: Boolean = isLandscapeLayout(),
) {
    val colors = BandEarTheme.colors
    val sessionRunning = state.connection.sessionRunning
    var manualAddressOpen by rememberSaveable { mutableStateOf(false) }
    // Once a session starts the form has done its job; after it ends, Connect shows again.
    LaunchedEffect(sessionRunning) {
        if (sessionRunning) manualAddressOpen = false
    }
    val screen = appScreenFor(sessionRunning, state.tab, manualAddressOpen)
    val rail = landscape && screen.showsTabs
    val bottomBar = !landscape && screen.showsTabs

    BackHandler(enabled = screen == AppScreen.MANUAL_ADDRESS) { manualAddressOpen = false }
    BackHandler(enabled = screen == AppScreen.MIXES || screen == AppScreen.SETTINGS) {
        actions.selectTab(MainTab.MIX)
    }

    // The bar and the rail pad for the insets on their own side (the navigation bar, a cutout);
    // the screen pads for the rest. The screen is called from this one place in both layouts, so
    // what it keeps in rememberSaveable is found again after the phone turns.
    val screenSides = when {
        rail -> WindowInsetsSides.Top + WindowInsetsSides.End + WindowInsetsSides.Bottom
        bottomBar -> WindowInsetsSides.Top + WindowInsetsSides.Horizontal
        else -> WindowInsetsSides.Vertical + WindowInsetsSides.Horizontal
    }
    Row(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground),
    ) {
        if (rail) NavigationRail(selected = state.tab, onSelect = { actions.selectTab(it) })
        Column(
            modifier = Modifier
                .weight(1f)
                .fillMaxHeight(),
        ) {
            Box(
                modifier = Modifier
                    .weight(1f)
                    .fillMaxWidth()
                    .windowInsetsPadding(WindowInsets.safeDrawing.only(screenSides)),
            ) {
                ScreenContent(screen, state, actions, landscape, onManualAddress = { manualAddressOpen = it })
            }
            if (bottomBar) BottomTabBar(selected = state.tab, onSelect = { actions.selectTab(it) })
        }
    }
}

@Composable
private fun ScreenContent(
    screen: AppScreen,
    state: MonitorUiState,
    actions: MonitorActions,
    landscape: Boolean,
    onManualAddress: (open: Boolean) -> Unit,
) {
    when (screen) {
        AppScreen.CONNECT -> ConnectScreen(
            musicianName = state.musicianName,
            servers = state.servers,
            connection = state.connection,
            onNameChange = { actions.setName(it) },
            onConnect = { actions.connect(it) },
            onManualAddress = { onManualAddress(true) },
            landscape = landscape,
        )
        AppScreen.MANUAL_ADDRESS, AppScreen.SETTINGS -> SettingsScreen(
            musicianName = state.musicianName,
            connection = state.connection,
            diagnostics = state.diagnostics,
            onNameChange = { actions.setName(it) },
            onManualAddressChange = { actions.editManualAddress(it) },
            onConnectManual = { host, controlPort, audioPort -> actions.connectManual(host, controlPort, audioPort) },
            onDisconnect = { actions.disconnect() },
            onBack = if (screen == AppScreen.MANUAL_ADDRESS) ({ onManualAddress(false) }) else null,
            landscape = landscape,
        )
        AppScreen.MIX -> MixScreen(
            musicianName = state.musicianName,
            connection = state.connection,
            mix = state.mix,
            moreOfMe = state.moreOfMe,
            onMoreOfMe = { actions.setMoreOfMe(it) },
            onMusicianChannel = { actions.setMusicianChannel(it) },
            onChannelLevel = { channel, level -> actions.setChannelLevel(channel, level) },
            onChannelPan = { channel, pan -> actions.setChannelPan(channel, pan) },
            onChannelMuted = { channel, muted -> actions.setChannelMuted(channel, muted) },
            onChannelSoloed = { channel, soloed -> actions.setChannelSoloed(channel, soloed) },
            onGroupLevel = { group, level -> actions.setGroupLevel(group, level) },
            onMasterVolume = { actions.setMasterVolume(it) },
            onMasterMuted = { actions.setMasterMuted(it) },
            onLevelCap = { actions.setLevelCap(it) },
        )
        AppScreen.MIXES -> SavedMixesScreen(
            savedMixes = state.savedMixes,
            onSave = { actions.saveMix(it) },
            onRecall = { actions.recallMix(it) },
            onRename = { from, to -> actions.renameMix(from, to) },
            onDelete = { actions.deleteMix(it) },
            landscape = landscape,
        )
    }
}

@Composable
private fun AppPreview(state: MonitorUiState, landscape: Boolean = false) {
    BandEarTheme {
        MonitorApp(state = state, actions = PreviewActions, landscape = landscape)
    }
}

@Preview(
    name = "App · not connected",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppNotConnectedPreview() = AppPreview(PreviewSamples.notConnected)

@Preview(
    name = "App · not connected, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppNotConnectedLandscapePreview() = AppPreview(PreviewSamples.notConnected, landscape = true)

@Preview(
    name = "App · Mix tab",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppMixPreview() = AppPreview(PreviewSamples.connected)

@Preview(
    name = "App · Mix tab, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppMixLandscapePreview() = AppPreview(PreviewSamples.connected, landscape = true)

@Preview(
    name = "App · Mixes tab",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppMixesPreview() = AppPreview(PreviewSamples.connected.copy(tab = MainTab.MIXES))

@Preview(
    name = "App · Mixes tab, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppMixesLandscapePreview() = AppPreview(PreviewSamples.connected.copy(tab = MainTab.MIXES), landscape = true)

@Preview(
    name = "App · Settings tab, reconnecting",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppSettingsPreview() = AppPreview(PreviewSamples.reconnecting.copy(tab = MainTab.SETTINGS))

@Preview(
    name = "App · Settings tab, reconnecting, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppSettingsLandscapePreview() =
    AppPreview(PreviewSamples.reconnecting.copy(tab = MainTab.SETTINGS), landscape = true)
