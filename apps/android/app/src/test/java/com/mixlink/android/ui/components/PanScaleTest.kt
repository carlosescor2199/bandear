package com.mixlink.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PanScaleTest {
    /**
     * A 200 px travel: the detent holds the middle 20 px (90 to 110) at the centre, and each side
     * maps 90 px onto 50 values, 1.8 px a step.
     */
    private val travel = 200f

    @Test
    fun theEndsAreHardLeftAndHardRight() {
        assertEquals(PAN_LEFT, panValueAt(0f, travel))
        assertEquals(PAN_RIGHT, panValueAt(travel, travel))
        assertEquals(PAN_LEFT, panValueAt(-30f, travel))
        assertEquals(PAN_RIGHT, panValueAt(260f, travel))
    }

    @Test
    fun theMiddleTenthOfTheTravelIsTheCentre() {
        assertEquals(PAN_CENTRE, panValueAt(100f, travel))
        assertEquals(PAN_CENTRE, panValueAt(91f, travel))
        assertEquals(PAN_CENTRE, panValueAt(109f, travel))
    }

    @Test
    fun justOutsideTheDetentThePanLeavesTheCentre() {
        assertEquals(49, panValueAt(88f, travel))
        assertEquals(51, panValueAt(112f, travel))
    }

    @Test
    fun everyPanCanBeReachedAndReadsBackAsItself() {
        for (pan in PAN_LEFT..PAN_RIGHT) {
            assertEquals("pan $pan", pan, panValueAt(panPositionOf(pan, travel), travel))
        }
    }

    @Test
    fun movingRightNeverMovesThePanLeft() {
        var previous = PAN_LEFT
        var position = 0f
        while (position <= travel) {
            val pan = panValueAt(position, travel)
            assertTrue("at $position px the pan went from $previous to $pan", pan >= previous)
            previous = pan
            position += 0.5f
        }
    }

    @Test
    fun theCentreIsDrawnInTheMiddleAndTheEndsAtTheEnds() {
        assertEquals(100f, panPositionOf(PAN_CENTRE, travel), 0.001f)
        assertEquals(0f, panPositionOf(PAN_LEFT, travel), 0.001f)
        assertEquals(travel, panPositionOf(PAN_RIGHT, travel), 0.001f)
        // Next to the centre the thumb sits at the detent's edge, so it snaps out of the middle.
        assertEquals(90f - 1.8f, panPositionOf(49, travel), 0.001f)
        assertEquals(110f + 1.8f, panPositionOf(51, travel), 0.001f)
    }

    @Test
    fun aPanOutOfRangeIsDrawnAtTheNearestEnd() {
        assertEquals(0f, panPositionOf(-10, travel), 0.001f)
        assertEquals(travel, panPositionOf(150, travel), 0.001f)
    }

    @Test
    fun withNoTravelItReadsTheCentre() {
        assertEquals(PAN_CENTRE, panValueAt(0f, 0f))
    }

    @Test
    fun theCentreReadsC() {
        assertEquals("C", panText(PAN_CENTRE))
    }

    @Test
    fun aSideReadsHowFarTowardIt() {
        assertEquals("L30", panText(35))
        assertEquals("R20", panText(60))
        assertEquals("L2", panText(49))
        assertEquals("R2", panText(51))
        assertEquals("L100", panText(PAN_LEFT))
        assertEquals("R100", panText(PAN_RIGHT))
    }

    @Test
    fun aPanOutOfRangeReadsAsTheNearestEnd() {
        assertEquals("L100", panText(-5))
        assertEquals("R100", panText(130))
    }

    @Test
    fun aScreenReaderHearsTheSideInWords() {
        assertEquals("Centre", panSpokenText(PAN_CENTRE))
        assertEquals("Left 30", panSpokenText(35))
        assertEquals("Right 100", panSpokenText(PAN_RIGHT))
    }

    /** A 24 px thumb on a 224 px control, so the thumb's edge travels the same 200 px. */
    private fun panGesture() = HorizontalFaderGesture(thumbWidthPx = 24f, scale = PanScale).apply { widthPx = 224f }

    @Test
    fun aTapNearTheMiddleLandsOnTheCentre() {
        val gesture = panGesture()
        assertEquals(PAN_CENTRE, gesture.tap(xPx = 112f, current = 30))
        assertEquals(PAN_CENTRE, gesture.tap(xPx = 121f, current = 80))
    }

    @Test
    fun aTapAtEitherEndIsHardLeftOrHardRight() {
        val gesture = panGesture()
        assertEquals(PAN_LEFT, gesture.tap(xPx = 4f, current = PAN_CENTRE))
        assertEquals(PAN_RIGHT, gesture.tap(xPx = 220f, current = PAN_CENTRE))
    }

    @Test
    fun aDragFromTheCentreHoldsThereUntilItLeavesTheDetent() {
        val gesture = panGesture()
        gesture.startDrag(current = PAN_CENTRE)

        assertNull("still inside the detent", gesture.drag(dxPx = 8f, current = PAN_CENTRE))
        assertEquals(51, gesture.drag(dxPx = 4f, current = PAN_CENTRE))
    }

    @Test
    fun aDragAcrossTheMiddleStopsOnTheCentre() {
        val gesture = panGesture()
        gesture.startDrag(current = 40)

        // From 18 px left of the detent, 28 px to the right lands 10 px inside it.
        assertEquals(PAN_CENTRE, gesture.drag(dxPx = 28f, current = 40))
    }

    @Test
    fun aScreenReaderStepIgnoresTheDetent() {
        assertEquals(48, panGesture().step(target = 48f, current = PAN_CENTRE))
    }
}
