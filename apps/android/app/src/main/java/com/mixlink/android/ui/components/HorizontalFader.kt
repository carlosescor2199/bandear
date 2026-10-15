package com.mixlink.android.ui.components

import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.progressSemantics
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.mixlink.android.ui.theme.MixLinkColors
import com.mixlink.android.ui.theme.MixLinkTheme

private val LevelFaderHeight = 44.dp
private val LevelTrackHeight = 8.dp
private val LevelThumbSize = 28.dp
private const val DisabledLevelAlpha = 0.4f

/**
 * A horizontal level control from 0 at the left to 100 at the right, such as the master level and
 * the level cap. It is 44 dp tall; the caller gives it its width.
 *
 * It behaves like [VerticalFader]: a drag moves the thumb from wherever it is, a tap moves it to the
 * tapped point, a screen reader hears a slider, and it reports a new level through [onValueChange]
 * only when the level changes. A fader no wider than its thumb ignores input (see [FaderGesture]).
 * A vertical swipe is left to the parent, so a page that scrolls still scrolls over it.
 */
@Composable
fun HorizontalFader(
    value: Int,
    onValueChange: (Int) -> Unit,
    accent: Color,
    contentDescription: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = MixLinkTheme.colors
    val currentValue by rememberUpdatedState(value)
    val currentOnValueChange by rememberUpdatedState(onValueChange)
    val thumbPx = with(LocalDensity.current) { LevelThumbSize.toPx() }
    // The gesture measures from the loud end, which is the right end here: positions and deltas
    // are flipped on the way in.
    val gesture = remember(thumbPx) { FaderGesture(thumbPx) }

    fun report(next: Int?) {
        if (next != null) currentOnValueChange(next)
    }

    val dragState = rememberDraggableState { delta -> report(gesture.drag(-delta, currentValue)) }

    Box(
        modifier = modifier
            .height(LevelFaderHeight)
            .alpha(if (enabled) 1f else DisabledLevelAlpha)
            .onSizeChanged { gesture.lengthPx = it.width.toFloat() }
            .progressSemantics(value.toFloat(), 0f..FADER_MAX.toFloat())
            .semantics {
                this.contentDescription = contentDescription
                if (enabled) {
                    setProgress { target ->
                        report(gesture.step(target, currentValue))
                        gesture.acceptsInput
                    }
                } else {
                    disabled()
                }
            }
            .pointerInput(enabled, gesture) {
                if (enabled) {
                    detectTapGestures(onTap = { report(gesture.tap(size.width - it.x, currentValue)) })
                }
            }
            .draggable(
                state = dragState,
                orientation = Orientation.Horizontal,
                enabled = enabled,
                onDragStarted = { gesture.startDrag(currentValue) },
            )
            .drawBehind { drawLevelFader(value, accent, colors) },
    )
}

private fun DrawScope.drawLevelFader(value: Int, accent: Color, colors: MixLinkColors) {
    val trackHeight = LevelTrackHeight.toPx()
    val thumb = LevelThumbSize.toPx()
    val travel = (size.width - thumb).coerceAtLeast(0f)
    val thumbCentreX = travel - faderOffsetOf(value, travel) + thumb / 2f
    val trackTop = (size.height - trackHeight) / 2f
    val corner = CornerRadius(trackHeight / 2f)
    val centre = Offset(thumbCentreX, size.height / 2f)

    drawRoundRect(colors.faderTrack, Offset(0f, trackTop), Size(size.width, trackHeight), corner)
    // The fill runs from the left end to the middle of the thumb.
    drawRoundRect(accent, Offset(0f, trackTop), Size(thumbCentreX, trackHeight), corner)
    drawCircle(Color.Black.copy(alpha = 0.5f), thumb / 2f, centre + Offset(0f, 2.dp.toPx()))
    drawCircle(colors.faderThumb, thumb / 2f, centre)
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun HorizontalFaderPreview() {
    MixLinkTheme {
        val colors = MixLinkTheme.colors
        var master by remember { mutableIntStateOf(85) }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            HorizontalFader(master, { master = it }, colors.accent, "Master level", Modifier.fillMaxWidth())
            HorizontalFader(100, {}, colors.faderIdle, "Level cap", Modifier.fillMaxWidth())
            HorizontalFader(30, {}, colors.faderMuted, "Master level, muted", Modifier.fillMaxWidth(), enabled = false)
        }
    }
}
