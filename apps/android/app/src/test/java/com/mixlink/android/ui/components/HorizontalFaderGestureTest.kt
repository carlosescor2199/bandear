package com.mixlink.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class HorizontalFaderGestureTest {
    /** A 24 px thumb on a 224 px fader: the thumb's edge travels 200 px, two pixels a step. */
    private fun laidOut(widthPx: Float = 224f) = HorizontalFaderGesture(thumbWidthPx = 24f).apply { this.widthPx = widthPx }

    @Test
    fun aRightwardDragRaisesTheLevel() {
        val gesture = laidOut()
        gesture.startDrag(current = 50)
        assertEquals(60, gesture.drag(dxPx = 20f, current = 50))
        assertEquals(70, gesture.drag(dxPx = 20f, current = 60))
    }

    @Test
    fun aLeftwardDragLowersTheLevel() {
        val gesture = laidOut()
        gesture.startDrag(current = 50)
        assertEquals(40, gesture.drag(dxPx = -20f, current = 50))
    }

    @Test
    fun aDragPastTheRightEndReadsAsTheTop() {
        val gesture = laidOut()
        gesture.startDrag(current = 90)
        assertEquals(FADER_MAX, gesture.drag(dxPx = 500f, current = 90))
    }

    @Test
    fun aTapNearTheRightEndRaisesTheLevel() {
        val gesture = laidOut()
        // 4 px from the right edge is inside the thumb's half-width at the loud end: the top.
        assertEquals(FADER_MAX, gesture.tap(xPx = 220f, current = 50))
        // 62 px from the right edge, as a tap 62 px down a vertical fader of the same length.
        assertEquals(75, gesture.tap(xPx = 162f, current = 50))
    }

    @Test
    fun aTapNearTheLeftEndLowersTheLevel() {
        val gesture = laidOut()
        assertEquals(0, gesture.tap(xPx = 4f, current = 50))
        assertEquals(25, gesture.tap(xPx = 62f, current = 50))
    }

    @Test
    fun aTapInTheMiddleOnTheCurrentLevelReportsNothing() {
        assertNull(laidOut().tap(xPx = 112f, current = 50))
    }

    @Test
    fun aScreenReaderStepIsTheSameOnEitherAxis() {
        assertEquals(43, laidOut().step(target = 42.6f, current = 10))
    }

    @Test
    fun beforeLayoutItIgnoresInput() {
        val gesture = HorizontalFaderGesture(thumbWidthPx = 24f)
        assertFalse(gesture.acceptsInput)
        assertNull(gesture.tap(xPx = 200f, current = 30))
        gesture.startDrag(current = 30)
        assertNull(gesture.drag(dxPx = 40f, current = 30))
        assertTrue(laidOut().acceptsInput)
    }
}
