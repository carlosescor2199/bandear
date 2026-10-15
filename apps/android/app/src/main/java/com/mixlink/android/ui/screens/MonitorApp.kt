package com.mixlink.android.ui.screens

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
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
import com.mixlink.android.ui.components.BottomTabBar
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.state.MonitorUiState
import com.mixlink.android.ui.theme.MixLinkTheme

/**
 * The whole app: Connect while no session runs, and the Mix, Mixes and Settings tabs with the
 * bottom bar while one does. Everything it shows comes from [state]; everything the musician does
 * goes to [actions].
 *
 * Each screen gets only the parts of [state] it shows. A part that did not change keeps its
 * instance (the reducer copies only what changes), so the screens it feeds are skipped when only
 * the receiver's statistics moved.
 *
 * The only state kept here is whether the manual address form was opened from Connect. The back
 * key closes it, and from the Mixes or Settings tab returns to Mix.
 */
@Composable
fun MonitorApp(state: MonitorUiState, actions: MonitorActions, modifier: Modifier = Modifier) {
    val colors = MixLinkTheme.colors
    val sessionRunning = state.connection.sessionRunning
    var manualAddressOpen by rememberSaveable { mutableStateOf(false) }
    // Once a session starts the form has done its job; after it ends, Connect shows again.
    LaunchedEffect(sessionRunning) {
        if (sessionRunning) manualAddressOpen = false
    }
    val screen = appScreenFor(sessionRunning, state.tab, manualAddressOpen)

    BackHandler(enabled = screen == AppScreen.MANUAL_ADDRESS) { manualAddressOpen = false }
    BackHandler(enabled = screen == AppScreen.MIXES || screen == AppScreen.SETTINGS) {
        actions.selectTab(MainTab.MIX)
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground),
    ) {
        Box(
            modifier = Modifier
                .weight(1f)
                .fillMaxWidth()
                .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)),
        ) {
            ScreenContent(screen, state, actions, onManualAddress = { manualAddressOpen = it })
        }
        // The bar reaches under the navigation bar; its items stay above it.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .background(if (screen.showsTabs) colors.bar else colors.ground)
                .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Bottom + WindowInsetsSides.Horizontal)),
        ) {
            if (screen.showsTabs) BottomTabBar(selected = state.tab, onSelect = { actions.selectTab(it) })
        }
    }
}

@Composable
private fun ScreenContent(
    screen: AppScreen,
    state: MonitorUiState,
    actions: MonitorActions,
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
        )
    }
}

@Composable
private fun AppPreview(state: MonitorUiState) {
    MixLinkTheme {
        MonitorApp(state = state, actions = PreviewActions)
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
    name = "App · Mix tab",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppMixPreview() = AppPreview(PreviewSamples.connected)

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
    name = "App · Settings tab, reconnecting",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun AppSettingsPreview() = AppPreview(PreviewSamples.reconnecting.copy(tab = MainTab.SETTINGS))
