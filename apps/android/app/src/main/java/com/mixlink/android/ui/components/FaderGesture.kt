package com.mixlink.android.ui.components

import kotlin.math.roundToInt

/**
 * The arithmetic behind a fader's touch and screen-reader input, kept apart from Compose so it can
 * be tested on the JVM.
 *
 * Positions are pixels along the fader's axis, measured from its loud end (the top of a vertical
 * fader, the right end of a horizontal one), and a positive delta moves toward the quiet end. The
 * thumb's centre follows the finger; its edge travels the fader's length minus the thumb.
 *
 * Every input returns the level to report, or null when there is nothing to report: the level
 * would not change, or the fader has no travel. A fader laid out no taller than its thumb (or not
 * laid out yet) has nowhere to move the thumb, so it ignores input instead of reading it as the
 * bottom: an input can never silently drop a level to silence.
 */
internal class FaderGesture(private val thumbLengthPx: Float) {
    /** The fader's laid-out length along its axis; 0 until it is laid out. */
    var lengthPx: Float = 0f

    /** Where the thumb's centre is during a drag, from the loud end. */
    private var dragCentrePx = 0f

    private val travelPx: Float
        get() = (lengthPx - thumbLengthPx).coerceAtLeast(0f)

    /** False while the thumb has no room to move; every input is ignored then. */
    val acceptsInput: Boolean
        get() = travelPx > 0f

    /** A tap at [positionPx]: the thumb's centre jumps under the finger. */
    fun tap(positionPx: Float, current: Int): Int? {
        if (!acceptsInput) return null
        return changed(levelWithCentreAt(positionPx), current)
    }

    /**
     * A drag begins. It is seeded from the [current] level rather than from where the finger
     * landed, so touching the fader never makes the level jump.
     */
    fun startDrag(current: Int) {
        dragCentrePx = faderOffsetOf(current, travelPx) + thumbLengthPx / 2f
    }

    /**
     * The finger moved [deltaPx] during a drag. The thumb follows the finger even when the caller
     * did not apply the last level, so the fader never drifts away from the finger.
     */
    fun drag(deltaPx: Float, current: Int): Int? {
        if (!acceptsInput) return null
        dragCentrePx += deltaPx
        return changed(levelWithCentreAt(dragCentrePx), current)
    }

    /** A screen reader sets the level to [target], rounded to a whole level and kept in range. */
    fun step(target: Float, current: Int): Int? {
        if (!acceptsInput || target.isNaN()) return null
        return changed(target.roundToInt().coerceIn(0, FADER_MAX), current)
    }

    private fun levelWithCentreAt(centrePx: Float): Int =
        faderValueAt(centrePx - thumbLengthPx / 2f, travelPx)

    /** Reports are de-duplicated: a level equal to the one shown is not reported again. */
    private fun changed(next: Int, current: Int): Int? = next.takeIf { it != current }
}
