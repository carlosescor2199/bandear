package com.bandear.android.ui.components

import android.view.WindowManager
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import androidx.compose.ui.window.DialogWindowProvider
import com.bandear.android.ui.theme.BandEarTheme
import kotlinx.coroutines.launch

/** How dark the rest of the screen goes behind the sheet or the panel, as the channel sheet's. */
private const val ScrimAlpha = 0.6f

/** The bottom sheet's top corners; its bottom sits on the screen's edge. */
private val SheetShape = RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp)

/** The side panel's corners on the start side, as round as the sheet's; its end sits on the screen's edge. */
private val SidePanelShape = RoundedCornerShape(topStart = 22.dp, bottomStart = 22.dp)

/** How wide the side panel is, or the whole window when that is narrower. */
private val SidePanelWidth = 400.dp

private const val SidePanelEnterMillis = 220
private const val SidePanelExitMillis = 180

/**
 * The container of a panel that opens over a screen, such as a channel's settings: a bottom sheet
 * in the portrait layout, and in landscape a panel that slides in from the end edge, about 400 dp
 * wide, over the dimmed rest of the screen. Both cover the whole window, bars included, wherever
 * on the screen the caller puts them.
 *
 * It shows while the caller composes it, the way [ModalBottomSheet] does: the caller keeps whether
 * it is open (in the ViewModel or `rememberSaveable`, so it stays open across a rotation) and
 * stops composing it in [onDismiss]. A tap on the dimmed screen, the back key and, for the sheet,
 * a swipe down close it; [content] gets a `dismiss` function to close it the same way from its own
 * Done button, sliding it away first and calling [onDismiss] after. However many of these arrive,
 * [onDismiss] is called once, when the panel has slid off the screen (see [ModalPanelDismissal]).
 *
 * [content] is composed in one place for as long as the layout stays the same, so its own state
 * survives every recomposition, and its `rememberSaveable` state a recreation of the Activity in
 * the same layout. A rotation that swaps the sheet for the panel composes it anew in the other
 * container, so state that must outlive that belongs to the caller, as the channel sheet's does.
 * The side panel scrolls its content when the window is too short for it; the sheet is as tall as
 * its content. [landscape] picks the container, [isLandscapeLayout] unless the caller knows better.
 */
@Composable
fun ModalPanel(
    onDismiss: () -> Unit,
    modifier: Modifier = Modifier,
    landscape: Boolean = isLandscapeLayout(),
    content: @Composable (dismiss: () -> Unit) -> Unit,
) {
    if (landscape) {
        SidePanel(onDismiss = onDismiss, modifier = modifier, content = content)
    } else {
        BottomSheetPanel(onDismiss = onDismiss, modifier = modifier, content = content)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun BottomSheetPanel(
    onDismiss: () -> Unit,
    modifier: Modifier,
    content: @Composable (dismiss: () -> Unit) -> Unit,
) {
    val colors = BandEarTheme.colors
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    val currentOnDismiss by rememberUpdatedState(onDismiss)
    val dismissal = remember { ModalPanelDismissal(onDismiss = { currentOnDismiss() }) }
    val dismiss: () -> Unit = remember(sheetState, scope) {
        {
            // Slide the sheet away first, then forget it, as a swipe down does.
            if (dismissal.requestClose()) {
                scope.launch { sheetState.hide() }.invokeOnCompletion {
                    dismissal.slideEnded(hidden = !sheetState.isVisible)
                }
            }
        }
    }
    ModalBottomSheet(
        // The sheet's own dismissals (scrim, back, swipe down) slide it away before they ask.
        onDismissRequest = { dismissal.slideEnded(hidden = !sheetState.isVisible) },
        modifier = modifier,
        sheetState = sheetState,
        shape = SheetShape,
        containerColor = colors.surface,
        contentColor = colors.text,
        tonalElevation = 0.dp,
        scrimColor = Color.Black.copy(alpha = ScrimAlpha),
        dragHandle = { BottomSheetDefaults.DragHandle(color = colors.textSecondary) },
    ) {
        content(dismiss)
    }
}

/**
 * The landscape panel, in a window of its own over the app's like the sheet's: the window dims
 * everything behind it, bars and cutout included, and the panel slides in from the end edge.
 */
@Composable
private fun SidePanel(
    onDismiss: () -> Unit,
    modifier: Modifier,
    content: @Composable (dismiss: () -> Unit) -> Unit,
) {
    val currentOnDismiss by rememberUpdatedState(onDismiss)
    val dismissal = remember { ModalPanelDismissal(onDismiss = { currentOnDismiss() }) }
    // 0 off the screen past the end edge, 1 in place.
    val shown = remember { Animatable(0f) }
    var leaving by remember { mutableStateOf(false) }
    LaunchedEffect(leaving) {
        if (leaving) {
            shown.animateTo(0f, tween(SidePanelExitMillis))
            dismissal.slideEnded(hidden = true)
        } else {
            shown.animateTo(1f, tween(SidePanelEnterMillis))
        }
    }
    val dismiss: () -> Unit = remember { { if (dismissal.requestClose()) leaving = true } }
    val towardEnd = if (LocalLayoutDirection.current == LayoutDirection.Ltr) 1f else -1f

    Dialog(
        onDismissRequest = dismiss,
        properties = DialogProperties(dismissOnClickOutside = false, usePlatformDefaultWidth = false),
    ) {
        DimBehindWindow()
        Box(Modifier.fillMaxSize()) {
            // The dimmed rest of the screen: a tap there closes the panel, as one outside a sheet does.
            Box(
                Modifier
                    .fillMaxSize()
                    .pointerInput(Unit) { detectTapGestures { dismiss() } },
            )
            SidePanelSurface(
                modifier = modifier
                    .align(Alignment.CenterEnd)
                    .graphicsLayer { translationX = (1f - shown.value) * size.width * towardEnd },
                content = { content(dismiss) },
            )
        }
    }
}

/** Dims everything behind the dialog window this is composed in, as dark as the sheet's scrim. */
@Composable
private fun DimBehindWindow() {
    val window = (LocalView.current.parent as? DialogWindowProvider)?.window
    SideEffect {
        window?.addFlags(WindowManager.LayoutParams.FLAG_DIM_BEHIND)
        window?.setDimAmount(ScrimAlpha)
    }
}

/**
 * The side panel itself: the surface colour with a border, round on its start corners, as tall as
 * the window, keeping its content clear of the bars and cutout on its end side and scrolling it
 * when the window is too short. Taps on it never reach the dimmed screen behind it.
 */
@Composable
private fun SidePanelSurface(modifier: Modifier, content: @Composable () -> Unit) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxHeight()
            .width(SidePanelWidth)
            .clip(SidePanelShape)
            .background(colors.surface)
            .border(1.dp, colors.border, SidePanelShape)
            // Taking pointer input makes the panel the topmost thing a touch hits, so a tap on it
            // that misses its controls never reaches the dimmed screen behind it.
            .pointerInput(Unit) {}
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.End + WindowInsetsSides.Vertical))
            .verticalScroll(rememberScrollState())
            .padding(top = 12.dp),
    ) {
        content()
    }
}

@Composable
private fun PanelPreviewContent() {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Text("Keys", style = MaterialTheme.typography.titleMedium, color = colors.text)
        Text("BALANCE · LEVEL · MUTE · SOLO", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
    }
}

/** A dimmed stand-in for the screen behind the panel in the previews. */
@Composable
private fun PreviewScrim() {
    Box(
        Modifier
            .fillMaxSize()
            .background(Color.Black.copy(alpha = ScrimAlpha)),
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 844, heightDp = 390)
@Composable
private fun ModalPanelLandscapePreview() {
    BandEarTheme {
        Box(Modifier.fillMaxSize()) {
            Row(Modifier.fillMaxSize()) {
                NavigationRail(selected = MainTab.MIX, onSelect = {})
            }
            PreviewScrim()
            SidePanelSurface(Modifier.align(Alignment.CenterEnd)) { PanelPreviewContent() }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 390, heightDp = 844)
@Composable
private fun ModalPanelPortraitPreview() {
    BandEarTheme {
        val colors = BandEarTheme.colors
        Box(Modifier.fillMaxSize()) {
            PreviewScrim()
            // What the bottom sheet draws around the content: its corners, colour and handle.
            Column(
                modifier = Modifier
                    .align(Alignment.BottomCenter)
                    .fillMaxWidth()
                    .clip(SheetShape)
                    .background(colors.surface),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                BottomSheetDefaults.DragHandle(color = colors.textSecondary)
                PanelPreviewContent()
            }
        }
    }
}
