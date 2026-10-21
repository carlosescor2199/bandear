package com.bandear.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class FaderGestureTest {
    /** A 24 px thumb on a 224 px fader: the thumb's edge travels 200 px, two pixels a step. */
    private fun laidOut(lengthPx: Float = 224f) = FaderGesture(thumbLengthPx = 24f).apply { this.lengthPx = lengthPx }

    @Test
    fun aTapPutsTheThumbCentreUnderTheFinger() {
        val gesture = laidOut()
        // The thumb centre sits half a thumb below its edge, so a tap 12 px down is the very top.
        assertEquals(100, gesture.tap(positionPx = 12f, current = 50))
        assertEquals(75, gesture.tap(positionPx = 62f, current = 50))
        assertEquals(0, gesture.tap(positionPx = 212f, current = 50))
    }

    @Test
    fun aTapPastEitherEndReadsAsThatEnd() {
        val gesture = laidOut()
        assertEquals(100, gesture.tap(positionPx = 0f, current = 50))
        assertEquals(0, gesture.tap(positionPx = 224f, current = 50))
    }

    @Test
    fun aTapOnTheCurrentLevelReportsNothing() {
        assertNull(laidOut().tap(positionPx = 112f, current = 50))
    }

    @Test
    fun aDragThatStartsAtAnyLevelWithZeroDeltaReportsNothing() {
        // An odd length, as a real fader in pixels would be, so the seed's rounding is exercised.
        val gesture = laidOut(lengthPx = 236.5f)
        for (value in 0..FADER_MAX) {
            gesture.startDrag(current = value)
            assertNull("level $value", gesture.drag(deltaPx = 0f, current = value))
        }
    }

    @Test
    fun aDragMovesTheLevelFromWhereItWasNotFromWhereTheFingerLanded() {
        val gesture = laidOut()
        gesture.startDrag(current = 50)
        assertEquals(40, gesture.drag(deltaPx = 20f, current = 50))
        assertEquals(30, gesture.drag(deltaPx = 20f, current = 40))
        assertEquals(55, gesture.drag(deltaPx = -50f, current = 30))
    }

    @Test
    fun aDragKeepsFollowingTheFingerWhenTheLevelWasNotApplied() {
        val gesture = laidOut()
        gesture.startDrag(current = 50)
        assertEquals(40, gesture.drag(deltaPx = 20f, current = 50))
        // The caller kept 50; the thumb still follows the finger, not the refused level.
        assertEquals(30, gesture.drag(deltaPx = 20f, current = 50))
    }

    @Test
    fun aDragSmallerThanHalfAStepReportsNothing() {
        val gesture = laidOut()
        gesture.startDrag(current = 50)
        assertNull(gesture.drag(deltaPx = 0.8f, current = 50))
        assertEquals(49, gesture.drag(deltaPx = 0.4f, current = 50))
    }

    @Test
    fun aDragPastEitherEndClampsToThatEnd() {
        val gesture = laidOut()
        gesture.startDrag(current = 90)
        assertEquals(FADER_MAX, gesture.drag(deltaPx = -500f, current = 90))
        assertEquals(0, gesture.drag(deltaPx = 1000f, current = 100))
    }

    @Test
    fun aStepRoundsToAWholeLevel() {
        val gesture = laidOut()
        assertEquals(43, gesture.step(target = 42.6f, current = 10))
        assertEquals(42, gesture.step(target = 42.4f, current = 10))
    }

    @Test
    fun aStepOutsideTheRangeClampsToTheNearestEnd() {
        val gesture = laidOut()
        assertEquals(FADER_MAX, gesture.step(target = 140f, current = 10))
        assertEquals(0, gesture.step(target = -3f, current = 10))
    }

    @Test
    fun aStepToTheCurrentLevelOrToNoNumberReportsNothing() {
        val gesture = laidOut()
        assertNull(gesture.step(target = 50.2f, current = 50))
        assertNull(gesture.step(target = Float.NaN, current = 50))
    }

    @Test
    fun zeroTravelReportsNothing() {
        // A fader laid out no taller than its thumb has nowhere to move it: no input may silence it.
        for (length in listOf(0f, 10f, 24f)) {
            val gesture = laidOut(lengthPx = length)
            assertFalse("length $length", gesture.acceptsInput)
            assertNull(gesture.tap(positionPx = 200f, current = 80))
            gesture.startDrag(current = 80)
            assertNull(gesture.drag(deltaPx = 0f, current = 80))
            assertNull(gesture.drag(deltaPx = 30f, current = 80))
            assertNull(gesture.step(target = 0f, current = 80))
        }
    }

    @Test
    fun aFaderTallerThanItsThumbAcceptsInput() {
        assertTrue(laidOut(lengthPx = 25f).acceptsInput)
    }

    @Test
    fun beforeLayoutTheFaderAcceptsNoInput() {
        val gesture = FaderGesture(thumbLengthPx = 24f)
        assertFalse(gesture.acceptsInput)
        assertNull(gesture.tap(positionPx = 0f, current = 30))
    }
}
