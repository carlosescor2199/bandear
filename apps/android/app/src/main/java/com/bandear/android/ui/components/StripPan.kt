package com.bandear.android.ui.components

import androidx.compose.foundation.gestures.Orientation
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.draggable
import androidx.compose.foundation.gestures.rememberDraggableState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.progressSemantics
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
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
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.disabled
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarColors
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/** The whole control is a 48 dp touch target, the drawn pan in its middle. */
private val PanTouchHeight = 48.dp

/** The row the line, the centre mark and the dot are drawn in. */
private val TrackRowHeight = 14.dp
private val LineHeight = 2.dp
private val DotSize = 10.dp

/**
 * How far in from each end the dot's centre stops: the line runs between those points. The pan's
 * maths take the dot as a thumb twice this wide, so its centre travels the width minus 8 dp.
 */
private val DotTravelInset = 4.dp
private val CentreMarkWidth = 1.dp
private val CentreMarkHeight = 10.dp
private const val DisabledPanAlpha = 0.4f

/**
 * The small pan at the top of a console strip: a thin line from hard left to hard right with a
 * mark at the centre, a dot where the pan is, and the pan written under it in numbers ("L30", "C",
 * "R20", see [panText]).
 *
 * It is the strip-sized [PanControl], with the same maths and the same meaning: a horizontal drag
 * moves the dot from wherever it is, a tap moves it to the tapped point, and the centre holds the
 * dot in a detent ([panValueAt]); a vertical swipe is left to the parent. A screen reader hears a
 * slider named [contentDescription] whose value is spoken as "Left 30", "Centre" or "Right 20"
 * ([panSpokenText]); the written pan is not read again. Stateless: it shows [pan] and reports a
 * new pan through [onPanChange], only when it changes. The whole control takes the touch: a
 * 48 dp tall target as wide as its slot, the drawn pan (26 dp) in its middle, the dot's centre
 * stopping 4 dp in from either end. The dot is [dotColor]: the secondary text colour, or the
 * accent on the musician's own channel. Disabled, it fades and takes no input.
 */
@Composable
fun StripPan(
    pan: Int,
    onPanChange: (Int) -> Unit,
    contentDescription: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    dotColor: Color = BandEarTheme.colors.textSecondary,
) {
    val colors = BandEarTheme.colors
    val currentPan by rememberUpdatedState(pan)
    val currentOnPanChange by rememberUpdatedState(onPanChange)
    val thumbPx = with(LocalDensity.current) { DotTravelInset.toPx() * 2f }
    // The dot's centre follows the finger, 4 dp short of either end at most.
    val gesture = remember(thumbPx) { HorizontalFaderGesture(thumbPx, PanScale) }

    fun report(next: Int?) {
        if (next != null) currentOnPanChange(next)
    }

    val dragState = rememberDraggableState { delta -> report(gesture.drag(delta, currentPan)) }

    Column(
        modifier = modifier
            .heightIn(min = PanTouchHeight)
            .alpha(if (enabled) 1f else DisabledPanAlpha)
            .onSizeChanged { gesture.widthPx = it.width.toFloat() }
            .progressSemantics(pan.toFloat(), PAN_LEFT.toFloat()..PAN_RIGHT.toFloat())
            .semantics {
                this.contentDescription = contentDescription
                stateDescription = panSpokenText(pan)
                if (enabled) {
                    setProgress { target ->
                        report(gesture.step(target, currentPan))
                        gesture.acceptsInput
                    }
                } else {
                    disabled()
                }
            }
            .pointerInput(enabled, gesture) {
                if (enabled) detectTapGestures(onTap = { report(gesture.tap(it.x, currentPan)) })
            }
            .draggable(
                state = dragState,
                orientation = Orientation.Horizontal,
                enabled = enabled,
                onDragStarted = { gesture.startDrag(currentPan) },
            ),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(TrackRowHeight)
                .drawBehind { drawStripPan(pan, dotColor, colors) },
        )
        Text(
            text = panText(pan),
            modifier = Modifier.clearAndSetSemantics {},
            style = BandEarType.NumberSmall.copy(fontSize = 10.sp, lineHeight = 12.sp),
            color = colors.textSecondary,
        )
    }
}

private fun DrawScope.drawStripPan(pan: Int, dotColor: Color, colors: BandEarColors) {
    val thumb = DotTravelInset.toPx() * 2f
    val middleY = size.height / 2f
    val lineHeight = LineHeight.toPx()
    // The line runs where the dot's centre can go: from hard left to hard right.
    val lineStart = panCentreOf(PAN_LEFT, size.width, thumb)
    val lineEnd = panCentreOf(PAN_RIGHT, size.width, thumb)
    drawRoundRect(
        colors.faderTrack,
        Offset(lineStart, middleY - lineHeight / 2f),
        Size(lineEnd - lineStart, lineHeight),
        CornerRadius(lineHeight / 2f),
    )
    val markWidth = CentreMarkWidth.toPx()
    val markHeight = CentreMarkHeight.toPx()
    drawRect(
        colors.neutral,
        Offset(panCentreOf(PAN_CENTRE, size.width, thumb) - markWidth / 2f, middleY - markHeight / 2f),
        Size(markWidth, markHeight),
    )
    drawCircle(dotColor, DotSize.toPx() / 2f, Offset(panCentreOf(pan, size.width, thumb), middleY))
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun StripPanPreview() {
    BandEarTheme {
        val colors = BandEarTheme.colors
        var pan by remember { mutableIntStateOf(40) }
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            StripPan(pan, { pan = it }, "CH 2 pan", Modifier.width(76.dp))
            StripPan(PAN_CENTRE, {}, "CH 3 pan", Modifier.width(76.dp), dotColor = colors.accent)
            StripPan(70, {}, "CH 4 pan", Modifier.width(76.dp), enabled = false)
        }
    }
}
