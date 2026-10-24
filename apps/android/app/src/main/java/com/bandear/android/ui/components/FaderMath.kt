package com.bandear.android.ui.components

import kotlin.math.roundToInt

/** The highest fader value: the top of the track. */
const val FADER_MAX = 100

/**
 * The fader value under a point [y] pixels below the top of a track [trackHeight] pixels tall.
 *
 * The top of the track is [FADER_MAX] and the bottom is 0, like a mixing desk, and a point above
 * or below the track reads as the nearest end. A track with no height reads as the bottom; touch
 * input never asks then, because [FaderGesture] ignores a fader with no travel.
 */
fun faderValueAt(y: Float, trackHeight: Float): Int {
    if (trackHeight <= 0f) return 0
    val fromBottom = 1f - y / trackHeight
    return (fromBottom * FADER_MAX).roundToInt().coerceIn(0, FADER_MAX)
}

/**
 * Where [value] sits on a track [trackHeight] pixels tall, in pixels below its top: the inverse of
 * [faderValueAt], used to place the thumb. A value outside 0..[FADER_MAX] is drawn at the
 * nearest end.
 */
fun faderOffsetOf(value: Int, trackHeight: Float): Float {
    val fromBottom = value.coerceIn(0, FADER_MAX).toFloat() / FADER_MAX
    return trackHeight * (1f - fromBottom)
}


/** The levels a vertical fader marks with ticks beside its track: the top, every quarter, the bottom. */
val FADER_TICK_LEVELS: List<Int> = listOf(100, 75, 50, 25, 0)

/**
 * Where the centre of the cap sits for [value] on a fader [lengthPx] long whose cap is [capPx]
 * tall, in pixels below its top. The cap's edge travels the length minus the cap, as
 * [FaderGesture] measures it, so the centre runs from half a cap below the top to half a cap above
 * the bottom; the track, its fill, the ticks and the limit marker are all drawn from it. A fader
 * no taller than its cap keeps the cap at the top.
 */
fun faderCentreOf(value: Int, lengthPx: Float, capPx: Float): Float =
    faderOffsetOf(value, (lengthPx - capPx).coerceAtLeast(0f)) + capPx / 2f

/** Where the ticks of [FADER_TICK_LEVELS] go on a fader [lengthPx] long, top to bottom (see [faderCentreOf]). */
fun faderTickCentres(lengthPx: Float, capPx: Float): List<Float> =
    FADER_TICK_LEVELS.map { faderCentreOf(it, lengthPx, capPx) }

/**
 * Where the marker of a [limit] goes on a fader [lengthPx] long: across the track at the height
 * the cap's centre would have at that level, so the cap reaches the marker exactly at the limit.
 * Null when there is no limit to mark; a limit outside 0..[FADER_MAX] is drawn at the nearest end.
 */
fun faderLimitCentre(limit: Int?, lengthPx: Float, capPx: Float): Float? =
    limit?.let { faderCentreOf(it, lengthPx, capPx) }
