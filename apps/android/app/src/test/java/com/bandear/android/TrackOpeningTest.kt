package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertSame
import org.junit.Assert.assertTrue
import org.junit.Assert.fail
import org.junit.Test

/**
 * How the player opens its track ([openTrackKeepingLowLatency]), with stand-in tracks: the large
 * track is kept only when it shrinks, else it is closed and the minimum one opened, and a track that
 * fails to start is never left open.
 */
class TrackOpeningTest {
    private class StandIn(val name: String, val failsToStart: Boolean = false, val applies: Int = 256)

    private val events = mutableListOf<String>()

    private fun open(
        large: StandIn,
        minimum: StandIn = StandIn("minimum"),
        keeps: (Int) -> Boolean = { it <= 256 },
        failingCushion: Boolean = false,
    ): StandIn =
        openTrackKeepingLowLatency(
            openLarge = { events += "open ${large.name}"; large },
            openMinimum = { events += "open ${minimum.name}"; minimum },
            start = { track ->
                events += "start ${track.name}"
                if (track.failsToStart) throw IllegalStateException("${track.name} will not play")
            },
            applyLowestCushion = { track ->
                events += "cushion ${track.name}"
                if (failingCushion) throw IllegalArgumentException("refused")
                track.applies
            },
            keepsLowLatency = keeps,
            close = { track -> events += "close ${track.name}" },
        )

    @Test
    fun theLargeTrackIsKeptWhenItShrinksToTheCushionAsked() {
        val large = StandIn("large", applies = 256)

        assertSame(large, open(large))
        assertEquals(listOf("open large", "start large", "cushion large"), events)
    }

    @Test
    fun aLargeTrackThatWillNotShrinkIsClosedAndTheMinimumOneOpened() {
        val large = StandIn("large", applies = 24_064)
        val minimum = StandIn("minimum")
        var asked = -1

        val opened = open(large, minimum, keeps = { applied -> asked = applied; false })

        assertSame(minimum, opened)
        assertEquals("the decision reads the size the track applied", 24_064, asked)
        assertEquals(
            listOf("open large", "start large", "cushion large", "close large", "open minimum", "start minimum"),
            events,
        )
    }

    @Test
    fun aLargeTrackThatFailsToStartIsClosedBeforeTheFailureGoesOn() {
        try {
            open(StandIn("large", failsToStart = true))
            fail("the failure goes on")
        } catch (error: IllegalStateException) {
            assertEquals("large will not play", error.message)
        }
        assertEquals(listOf("open large", "start large", "close large"), events)
    }

    @Test
    fun aTrackThatRefusesTheCushionIsClosedToo() {
        try {
            open(StandIn("large"), failingCushion = true)
            fail("the failure goes on")
        } catch (error: IllegalArgumentException) {
            assertEquals("refused", error.message)
        }
        assertEquals(listOf("open large", "start large", "cushion large", "close large"), events)
    }

    @Test
    fun aMinimumTrackThatFailsToStartIsClosedAndTheLargeOneStaysClosed() {
        try {
            open(StandIn("large", applies = 24_064), StandIn("minimum", failsToStart = true), keeps = { false })
            fail("the failure goes on")
        } catch (error: IllegalStateException) {
            assertEquals("minimum will not play", error.message)
        }
        assertEquals(
            listOf("open large", "start large", "cushion large", "close large", "open minimum", "start minimum", "close minimum"),
            events,
        )
    }

    @Test
    fun aCloseThatFailsTooIsKeptWithTheFailure() {
        try {
            openTrackKeepingLowLatency(
                openLarge = { StandIn("large") },
                openMinimum = { StandIn("minimum") },
                start = { throw IllegalStateException("will not play") },
                applyLowestCushion = { 256 },
                keepsLowLatency = { true },
                close = { throw IllegalStateException("will not release") },
            )
            fail("the failure goes on")
        } catch (error: IllegalStateException) {
            assertEquals("will not play", error.message)
            assertTrue(error.suppressed.any { it.message == "will not release" })
        }
    }
}
