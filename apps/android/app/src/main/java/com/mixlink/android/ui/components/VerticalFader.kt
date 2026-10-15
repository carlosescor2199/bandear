package com.mixlink.android.ui.components

import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
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
import androidx.compose.ui.graphics.StrokeCap
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
import kotlin.math.roundToInt

private val FaderWidth = 44.dp
private val FaderHeight = 236.dp
private val TrackWidth = 8.dp
private val ThumbWidth = 40.dp
private val ThumbHeight = 24.dp
private val ThumbRadius = 6.dp
private const val DisabledAlpha = 0.4f

/**
 * Where the fader's track is, kept out of composition: it changes with layout and drags, and
 * neither should recompose the fader.
 */
private class FaderTrack {
    var heightPx = 0f
    var dragCenterPx = 0f
}

/**
 * A vertical channel fader from 0 at the bottom to 100 at the top.
 *
 * A drag moves the thumb with the finger from wherever it is, so touching the strip never makes the
 * level jump; a tap moves the thumb to the tapped point. A horizontal swipe is left to the parent,
 * so a row of faders still scrolls sideways. For a screen reader it is a slider: it reads the
 * level as a percentage and adjusts it in steps.
 *
 * Stateless: it shows [value] and reports a new level through [onValueChange], only when the level
 * actually changes. The fill uses [accent], so the caller decides what the colour means (the
 * musician's own channel, a muted one).
 */
@Composable
fun VerticalFader(
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
    val thumbHeightPx = with(LocalDensity.current) { ThumbHeight.toPx() }
    val track = remember { FaderTrack() }

    // The thumb travels the track minus its own height, so it never leaves the track.
    fun travelPx(): Float = (track.heightPx - thumbHeightPx).coerceAtLeast(0f)

    fun report(next: Int) {
        if (next != currentValue) currentOnValueChange(next)
    }

    fun moveThumbCenterTo(centerY: Float) {
        report(faderValueAt(centerY - thumbHeightPx / 2f, travelPx()))
    }

    val dragState = rememberDraggableState { delta ->
        track.dragCenterPx += delta
        moveThumbCenterTo(track.dragCenterPx)
    }

    Box(
        modifier = modifier
            .width(FaderWidth)
            .height(FaderHeight)
            .alpha(if (enabled) 1f else DisabledAlpha)
            .onSizeChanged { track.heightPx = it.height.toFloat() }
            .progressSemantics(value.toFloat(), 0f..FADER_MAX.toFloat())
            .semantics {
                this.contentDescription = contentDescription
                if (enabled) {
                    setProgress { target ->
                        report(target.roundToInt().coerceIn(0, FADER_MAX))
                        true
                    }
                } else {
                    disabled()
                }
            }
            .pointerInput(enabled, thumbHeightPx) {
                if (enabled) detectTapGestures(onTap = { moveThumbCenterTo(it.y) })
            }
            .draggable(
                state = dragState,
                orientation = Orientation.Vertical,
                enabled = enabled,
                onDragStarted = {
                    track.dragCenterPx = faderOffsetOf(currentValue, travelPx()) + thumbHeightPx / 2f
                },
            )
            .drawBehind { drawFader(value, accent, colors) },
    )
}

private fun DrawScope.drawFader(value: Int, accent: Color, colors: MixLinkColors) {
    val trackWidth = TrackWidth.toPx()
    val thumbWidth = ThumbWidth.toPx()
    val thumbHeight = ThumbHeight.toPx()
    val trackLeft = (size.width - trackWidth) / 2f
    val trackCorner = CornerRadius(trackWidth / 2f)
    val thumbTop = faderOffsetOf(value, (size.height - thumbHeight).coerceAtLeast(0f))
    val thumbCenter = thumbTop + thumbHeight / 2f
    val thumbLeft = (size.width - thumbWidth) / 2f
    val thumbCorner = CornerRadius(ThumbRadius.toPx())

    drawRoundRect(colors.faderTrack, Offset(trackLeft, 0f), Size(trackWidth, size.height), trackCorner)
    // The fill rises from the bottom to the middle of the thumb.
    drawRoundRect(
        accent,
        Offset(trackLeft, thumbCenter),
        Size(trackWidth, size.height - thumbCenter),
        trackCorner,
    )
    // A soft shadow under the thumb, then the thumb with a grip line across its middle.
    drawRoundRect(
        Color.Black.copy(alpha = 0.5f),
        Offset(thumbLeft, thumbTop + 2.dp.toPx()),
        Size(thumbWidth, thumbHeight),
        thumbCorner,
    )
    drawRoundRect(colors.faderThumb, Offset(thumbLeft, thumbTop), Size(thumbWidth, thumbHeight), thumbCorner)
    val gripInset = 10.dp.toPx()
    drawLine(
        colors.faderTrack,
        Offset(thumbLeft + gripInset, thumbCenter),
        Offset(thumbLeft + thumbWidth - gripInset, thumbCenter),
        strokeWidth = 2.dp.toPx(),
        cap = StrokeCap.Round,
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun VerticalFaderPreview() {
    MixLinkTheme {
        val colors = MixLinkTheme.colors
        var own by remember { mutableIntStateOf(88) }
        var other by remember { mutableIntStateOf(64) }
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            VerticalFader(own, { own = it }, colors.accent, "CH 3 level")
            VerticalFader(other, { other = it }, colors.faderIdle, "CH 1 level")
            VerticalFader(55, {}, colors.faderMuted, "CH 4 level, muted")
            VerticalFader(40, {}, colors.faderIdle, "CH 5 level", enabled = false)
        }
    }
}
