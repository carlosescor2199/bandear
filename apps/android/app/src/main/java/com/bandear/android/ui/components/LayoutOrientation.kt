package com.bandear.android.ui.components

import androidx.compose.runtime.Composable
import androidx.compose.runtime.ReadOnlyComposable
import androidx.compose.ui.platform.LocalConfiguration

/** Which of its two layouts a screen uses: the tall one or the wide one. */
enum class LayoutOrientation { PORTRAIT, LANDSCAPE }

/**
 * The layout for a window [widthDp] wide and [heightDp] tall: landscape when it is wider than it
 * is tall, portrait otherwise, a square window included.
 *
 * It reads the shape of the room the app has rather than which way the device is turned, so a
 * phone on its side, a tablet and one half of a split screen each get the layout that fits. A
 * window not measured yet (0 by 0) is portrait.
 */
fun layoutOrientationFor(widthDp: Int, heightDp: Int): LayoutOrientation =
    if (widthDp > heightDp) LayoutOrientation.LANDSCAPE else LayoutOrientation.PORTRAIT

/**
 * The layout for the window the app is in now (see [layoutOrientationFor]), from the current
 * configuration's width and height. It changes when the window turns or is resized; Android then
 * recreates the Activity, so state a screen keeps across the change belongs in the ViewModel or in
 * `rememberSaveable`.
 */
@Composable
@ReadOnlyComposable
fun layoutOrientation(): LayoutOrientation {
    val configuration = LocalConfiguration.current
    return layoutOrientationFor(configuration.screenWidthDp, configuration.screenHeightDp)
}

/** True while the window is wider than it is tall, so the screens use their landscape layout. */
@Composable
@ReadOnlyComposable
fun isLandscapeLayout(): Boolean = layoutOrientation() == LayoutOrientation.LANDSCAPE
