package com.bandear.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FaderMarksTest {
    /** A 214 px fader with a 14 px cap: the cap's centre travels 200 px, from 7 px to 207 px. */
    private val length = 214f
    private val cap = 14f

    @Test
    fun theCapsCentreRunsFromHalfACapBelowTheTopToHalfACapAboveTheBottom() {
        assertEquals(7f, faderCentreOf(FADER_MAX, length, cap), 0.001f)
        assertEquals(207f, faderCentreOf(0, length, cap), 0.001f)
        assertEquals(57f, faderCentreOf(75, length, cap), 0.001f)
    }

    @Test
    fun aLevelOutsideTheRangeIsDrawnAtTheNearestEnd() {
        assertEquals(7f, faderCentreOf(140, length, cap), 0.001f)
        assertEquals(207f, faderCentreOf(-5, length, cap), 0.001f)
    }

    @Test
    fun aFaderNoTallerThanItsCapKeepsTheCapAtTheTop() {
        assertEquals(7f, faderCentreOf(30, lengthPx = 10f, capPx = cap), 0.001f)
    }

    @Test
    fun aTapOnTheDrawnCapReadsBackAsItsLevel() {
        val gesture = FaderGesture(cap).apply { lengthPx = length }
        for (level in 0..FADER_MAX) {
            assertEquals("level $level", level, gesture.tap(faderCentreOf(level, length, cap), current = -1))
        }
    }

    @Test
    fun theTicksMarkTheTopEveryQuarterAndTheBottom() {
        assertEquals(listOf(100, 75, 50, 25, 0), FADER_TICK_LEVELS)
        assertEquals(listOf(7f, 57f, 107f, 157f, 207f), faderTickCentres(length, cap))
    }

    @Test
    fun noLimitDrawsNoMarker() {
        assertNull(faderLimitCentre(null, length, cap))
    }

    @Test
    fun theLimitMarkerSitsWhereTheCapsCentreWouldBeAtThatLevel() {
        assertEquals(37f, faderLimitCentre(85, length, cap)!!, 0.001f)
        assertEquals(faderCentreOf(40, length, cap), faderLimitCentre(40, length, cap)!!, 0.001f)
    }

    @Test
    fun aLimitOutsideTheRangeIsDrawnAtTheNearestEnd() {
        assertEquals(7f, faderLimitCentre(120, length, cap)!!, 0.001f)
        assertEquals(207f, faderLimitCentre(-1, length, cap)!!, 0.001f)
    }
}
