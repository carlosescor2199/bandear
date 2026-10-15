package com.mixlink.android.ui.components

import org.junit.Assert.assertEquals
import org.junit.Test

class FaderMathTest {
    @Test
    fun topOfTheTrackIsFullAndBottomIsSilent() {
        assertEquals(100, faderValueAt(y = 0f, trackHeight = 200f))
        assertEquals(0, faderValueAt(y = 200f, trackHeight = 200f))
    }

    @Test
    fun valueFollowsThePointerDownTheTrack() {
        assertEquals(75, faderValueAt(y = 50f, trackHeight = 200f))
        assertEquals(50, faderValueAt(y = 100f, trackHeight = 200f))
        assertEquals(25, faderValueAt(y = 150f, trackHeight = 200f))
    }

    @Test
    fun valueRoundsToTheNearestStep() {
        // 0.4 of a step below 50 still reads 50; 0.6 of a step reads 49.
        assertEquals(50, faderValueAt(y = 100.8f, trackHeight = 200f))
        assertEquals(49, faderValueAt(y = 101.2f, trackHeight = 200f))
    }

    @Test
    fun pointerOutsideTheTrackClampsToTheNearestEnd() {
        assertEquals(100, faderValueAt(y = -30f, trackHeight = 200f))
        assertEquals(0, faderValueAt(y = 260f, trackHeight = 200f))
    }

    @Test
    fun trackWithNoHeightReadsAsTheBottom() {
        assertEquals(0, faderValueAt(y = 0f, trackHeight = 0f))
        assertEquals(0, faderValueAt(y = 12f, trackHeight = -4f))
    }

    @Test
    fun offsetPlacesTheThumbFromTheTop() {
        assertEquals(0f, faderOffsetOf(value = 100, trackHeight = 200f), 0.001f)
        assertEquals(150f, faderOffsetOf(value = 25, trackHeight = 200f), 0.001f)
        assertEquals(200f, faderOffsetOf(value = 0, trackHeight = 200f), 0.001f)
    }

    @Test
    fun offsetClampsValuesOutsideTheRange() {
        assertEquals(0f, faderOffsetOf(value = 140, trackHeight = 200f), 0.001f)
        assertEquals(200f, faderOffsetOf(value = -5, trackHeight = 200f), 0.001f)
    }

    @Test
    fun offsetIsTheInverseOfTheValueForEveryStep() {
        // An odd height, as a real track in pixels would be, so the rounding is exercised.
        val trackHeight = 212f
        for (value in 0..FADER_MAX) {
            assertEquals(value, faderValueAt(faderOffsetOf(value, trackHeight), trackHeight))
        }
    }
}
