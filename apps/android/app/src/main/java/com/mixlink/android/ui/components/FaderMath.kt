package com.mixlink.android.ui.components

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
