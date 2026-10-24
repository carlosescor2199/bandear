package com.bandear.android.ui.components

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
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarColors
import com.bandear.android.ui.theme.BandEarTheme

/** Wide enough for a 48 dp touch target; the ticks show on both sides of the cap. */
private val FaderWidth = 48.dp
private val FaderHeight = 236.dp
private val TrackWidth = 4.dp

/** The console cap: a light block with a dark line across its middle. */
private val CapWidth = 36.dp
private val CapHeight = 14.dp
private val CapRadius = 3.dp
private val CapLineHeight = 2.dp

/** The ticks: 8 dp marks, 4 dp in from each side, at the levels of [FADER_TICK_LEVELS]. */
private val TickLength = 8.dp
private val TickInset = 4.dp
private val TickThickness = 1.dp

/** The engineer's limit: an amber line across the whole fader. */
private val LimitThickness = 2.dp

private const val DisabledAlpha = 0.4f
private const val CapShadowAlpha = 0.6f

/**
 * A vertical channel fader from 0 at the bottom to 100 at the top, drawn like a mixing console's:
 * a thin track between tick marks at the top, every quarter and the bottom, and a light cap with a
 * dark line across its middle that the fill rises to.
 *
 * A drag moves the cap with the finger from wherever it is, so touching the strip never makes the
 * level jump; a tap moves the cap to the tapped point. A horizontal swipe is left to the parent,
 * so a row of faders still scrolls sideways. For a screen reader it is a slider: it reads the
 * level as a percentage and adjusts it in steps.
 *
 * Stateless: it shows [value] and reports a new level through [onValueChange], only when the level
 * actually changes. A fader laid out no taller than its cap ignores input rather than report
 * silence (see [FaderGesture]). The fill uses [accent], so the caller decides what the colour
 * means (the musician's own channel, a muted one). A disabled fader fades and takes no input.
 * [limitMarker] draws the engineer's limit as an amber line across the track at that level; null
 * draws none, and it adds nothing to what a screen reader hears (the caller's description does).
 *
 * It is 48 dp wide and [height] tall, 236 dp unless the caller needs room for something else in
 * the same column. A modifier that sets the size itself wins over both, so `Modifier.weight(1f)`
 * in a column makes the fader take the height that is left; the ticks stay at its sides and the
 * cap and track in its middle.
 */
@Composable
fun VerticalFader(
    value: Int,
    onValueChange: (Int) -> Unit,
    accent: Color,
    contentDescription: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    height: Dp = FaderHeight,
    limitMarker: Int? = null,
) {
    val colors = BandEarTheme.colors
    val currentValue by rememberUpdatedState(value)
    val currentOnValueChange by rememberUpdatedState(onValueChange)
    val capHeightPx = with(LocalDensity.current) { CapHeight.toPx() }
    // Kept out of composition: layout and drags change it, and neither should recompose the fader.
    // Positions from the top are positions from the loud end, as the gesture expects.
    val gesture = remember(capHeightPx) { FaderGesture(capHeightPx) }

    fun report(next: Int?) {
        if (next != null) currentOnValueChange(next)
    }

    val dragState = rememberDraggableState { delta -> report(gesture.drag(delta, currentValue)) }

    Box(
        modifier = modifier
            .width(FaderWidth)
            .height(height)
            .alpha(if (enabled) 1f else DisabledAlpha)
            .onSizeChanged { gesture.lengthPx = it.height.toFloat() }
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
                if (enabled) detectTapGestures(onTap = { report(gesture.tap(it.y, currentValue)) })
            }
            .draggable(
                state = dragState,
                orientation = Orientation.Vertical,
                enabled = enabled,
                onDragStarted = { gesture.startDrag(currentValue) },
            )
            .drawBehind { drawFader(value, accent, limitMarker, colors) },
    )
}

private fun DrawScope.drawFader(value: Int, accent: Color, limitMarker: Int?, colors: BandEarColors) {
    val cap = CapHeight.toPx()
    val trackWidth = TrackWidth.toPx()
    val trackLeft = (size.width - trackWidth) / 2f
    val trackCorner = CornerRadius(trackWidth / 2f)
    // The track runs where the cap's centre can go, so the fill always ends under the cap's line.
    val trackTop = faderCentreOf(FADER_MAX, size.height, cap)
    val trackBottom = faderCentreOf(0, size.height, cap)
    val capCentre = faderCentreOf(value, size.height, cap)

    val tickLength = TickLength.toPx()
    val tickInset = TickInset.toPx()
    val tickThickness = TickThickness.toPx()
    faderTickCentres(size.height, cap).forEach { y ->
        val top = y - tickThickness / 2f
        drawRect(colors.border, Offset(tickInset, top), Size(tickLength, tickThickness))
        drawRect(colors.border, Offset(size.width - tickInset - tickLength, top), Size(tickLength, tickThickness))
    }

    drawRoundRect(colors.faderTrack, Offset(trackLeft, trackTop), Size(trackWidth, trackBottom - trackTop), trackCorner)
    // The fill rises from the bottom of the track to the cap's centre.
    drawRoundRect(accent, Offset(trackLeft, capCentre), Size(trackWidth, trackBottom - capCentre), trackCorner)

    val limitCentre = faderLimitCentre(limitMarker, size.height, cap)
    if (limitCentre != null) {
        val thickness = LimitThickness.toPx()
        drawRoundRect(
            colors.accent,
            Offset(0f, limitCentre - thickness / 2f),
            Size(size.width, thickness),
            CornerRadius(thickness / 2f),
        )
    }

    // A soft shadow under the cap, then the cap with its dark line across the middle.
    val capWidth = CapWidth.toPx()
    val capLeft = (size.width - capWidth) / 2f
    val capTop = capCentre - cap / 2f
    val capCorner = CornerRadius(CapRadius.toPx())
    drawRoundRect(
        Color.Black.copy(alpha = CapShadowAlpha),
        Offset(capLeft, capTop + 1.dp.toPx()),
        Size(capWidth, cap),
        capCorner,
    )
    drawRoundRect(colors.faderThumb, Offset(capLeft, capTop), Size(capWidth, cap), capCorner)
    val lineHeight = CapLineHeight.toPx()
    drawRect(colors.ground, Offset(capLeft, capCentre - lineHeight / 2f), Size(capWidth, lineHeight))
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun VerticalFaderPreview() {
    BandEarTheme {
        val colors = BandEarTheme.colors
        var own by remember { mutableIntStateOf(88) }
        var other by remember { mutableIntStateOf(64) }
        var master by remember { mutableIntStateOf(80) }
        Row(
            modifier = Modifier.padding(16.dp),
            horizontalArrangement = Arrangement.spacedBy(24.dp),
        ) {
            VerticalFader(own, { own = it }, colors.accent, "CH 3 level")
            VerticalFader(other, { other = it }, colors.faderIdle, "CH 1 level")
            VerticalFader(55, {}, colors.faderMuted, "CH 4 level, muted")
            VerticalFader(40, {}, colors.faderIdle, "CH 5 level", enabled = false)
            VerticalFader(master, { master = it }, colors.textSecondary, "Master level", limitMarker = 85)
        }
    }
}
