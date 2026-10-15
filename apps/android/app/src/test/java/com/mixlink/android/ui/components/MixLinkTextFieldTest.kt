package com.mixlink.android.ui.components

import com.mixlink.android.ui.state.MAX_NAME_LENGTH
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MixLinkTextFieldTest {
    @Test
    fun anOutsideChangeShowsWhileTheFieldIsNotBeingEdited() {
        assertTrue(adoptOutsideText(outside = "Carlos", shown = "", focused = false))
    }

    @Test
    fun anOutsideValueLaggingBehindTheTypingIsIgnored() {
        // The echo of "Carl" arrives after "Carlo" was typed: it must not undo the last key.
        assertFalse(adoptOutsideText(outside = "Carl", shown = "Carlo", focused = true))
    }

    @Test
    fun theSameTextIsNeverReplaced() {
        assertFalse(adoptOutsideText(outside = "Carlos", shown = "Carlos", focused = false))
        assertFalse(adoptOutsideText(outside = "Carlos", shown = "Carlos", focused = true))
    }

    @Test
    fun anEditWithinTheLimitIsKept() {
        assertEquals("Carlos", keptEdit(shown = "Carlo", proposed = "Carlos", maxLength = 6))
    }

    @Test
    fun atTheLimitAFurtherKeyDoesNothing() {
        assertEquals("Carlos", keptEdit(shown = "Carlos", proposed = "Carlosx", maxLength = 6))
    }

    @Test
    fun aPastePastTheLimitKeepsItsStart() {
        assertEquals("Carlos", keptEdit(shown = "", proposed = "Carlos Escudero", maxLength = 6))
    }

    @Test
    fun aTooLongTextCanStillBeShortened() {
        assertEquals("Carlos Escuder", keptEdit(shown = "Carlos Escudero", proposed = "Carlos Escuder", maxLength = 6))
    }

    @Test
    fun theCutNeverSplitsATwoUnitCharacter() {
        // "Ana " and a guitar emoji: the emoji is two UTF-16 units, the 5th and 6th.
        val proposed = "Ana 🎸!"
        assertEquals("Ana ", keptEdit(shown = "", proposed = proposed, maxLength = 5))
        assertEquals("Ana 🎸", keptEdit(shown = "", proposed = proposed, maxLength = 6))
    }

    @Test
    fun aNameStopsAtTheLengthTheServerKeeps() {
        // The server keeps 64 characters of a name; the old name field stopped there too.
        assertEquals(64, MAX_NAME_LENGTH)
        assertEquals(64, keptEdit(shown = "", proposed = "a".repeat(80), maxLength = MAX_NAME_LENGTH).length)
    }
}
