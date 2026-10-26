package com.bandear.android.ui.components

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
import androidx.compose.material3.MaterialTheme
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
import com.bandear.android.ui.theme.BandEarColors
import com.bandear.android.ui.theme.BandEarTheme
import kotlin.math.max
import kotlin.math.min

private val PanTouchHeight = 52.dp
private val PanTrackHeight = 8.dp
private val PanThumbSize = 32.dp
private val CentreMarkWidth = 2.dp
private val CentreMarkHeight = 24.dp
private const val DisabledPanAlpha = 0.4f

/**
 * A large left-to-right pan control: hard left at the left end, hard right at the right end, and
 * the centre in the middle, marked and held by a detent so it is easy to land on (see
 * [panValueAt]). Under it, "L", "C" and "R" mark the ends and the middle.
 *
 * It behaves like [HorizontalFader] and shares its gesture: a drag moves the thumb from wherever it
 * is, a tap moves it to the tapped point, and a screen reader hears a slider whose value is spoken
 * as "Left 30", "Centre" or "Right 20". The fill runs from the centre to the thumb, so the side and
 * how far is shown by position as well as by the colour. Stateless: it shows [pan] and reports a
 * new pan through [onPanChange], only when it changes.
 */
@Composable
fun PanControl(
    pan: Int,
    onPanChange: (Int) -> Unit,
    contentDescription: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    val currentPan by rememberUpdatedState(pan)
    val currentOnPanChange by rememberUpdatedState(onPanChange)
    val thumbPx = with(LocalDensity.current) { PanThumbSize.toPx() }
    val gesture = remember(thumbPx) { HorizontalFaderGesture(thumbPx, PanScale) }

    fun report(next: Int?) {
        if (next != null) currentOnPanChange(next)
    }

    val dragState = rememberDraggableState { delta -> report(gesture.drag(delta, currentPan)) }

    Column(
        modifier = modifier.alpha(if (enabled) 1f else DisabledPanAlpha),
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(PanTouchHeight)
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
                )
                .drawBehind { drawPan(pan, colors) },
        )
        // The marks sit under the thumb's centre at each end and in the middle.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 12.dp)
                .clearAndSetSemantics {},
        ) {
            PanMark("L", Modifier.align(Alignment.CenterStart))
            PanMark("C", Modifier.align(Alignment.Center))
            PanMark("R", Modifier.align(Alignment.CenterEnd))
        }
    }
}

@Composable
private fun PanMark(text: String, modifier: Modifier) {
    Text(
        text = text,
        modifier = modifier,
        style = MaterialTheme.typography.labelMedium,
        color = BandEarTheme.colors.textSecondary,
    )
}

private fun DrawScope.drawPan(pan: Int, colors: BandEarColors) {
    val trackHeight = PanTrackHeight.toPx()
    val thumb = PanThumbSize.toPx()
    val thumbCentreX = panCentreOf(pan, size.width, thumb)
    val middleX = size.width / 2f
    val trackTop = (size.height - trackHeight) / 2f
    val corner = CornerRadius(trackHeight / 2f)

    drawRoundRect(colors.faderTrack, Offset(0f, trackTop), Size(size.width, trackHeight), corner)
    // The fill runs from the centre to the thumb: nothing at the centre, the whole side at an end.
    val fillStart = min(middleX, thumbCentreX)
    val fillEnd = max(middleX, thumbCentreX)
    if (fillEnd > fillStart) {
        drawRoundRect(colors.accent, Offset(fillStart, trackTop), Size(fillEnd - fillStart, trackHeight), corner)
    }
    val markWidth = CentreMarkWidth.toPx()
    val markHeight = CentreMarkHeight.toPx()
    drawRoundRect(
        colors.textSecondary,
        Offset(middleX - markWidth / 2f, (size.height - markHeight) / 2f),
        Size(markWidth, markHeight),
        CornerRadius(markWidth / 2f),
    )
    val centre = Offset(thumbCentreX, size.height / 2f)
    drawCircle(Color.Black.copy(alpha = 0.5f), thumb / 2f, centre + Offset(0f, 2.dp.toPx()))
    drawCircle(colors.faderThumb, thumb / 2f, centre)
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117, widthDp = 360)
@Composable
private fun PanControlPreview() {
    BandEarTheme {
        var pan by remember { mutableIntStateOf(35) }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            PanControl(pan, { pan = it }, "CH 3 pan", Modifier.fillMaxWidth())
            PanControl(PAN_CENTRE, {}, "CH 4 pan", Modifier.fillMaxWidth())
            PanControl(PAN_RIGHT, {}, "CH 5 pan", Modifier.fillMaxWidth(), enabled = false)
        }
    }
}
