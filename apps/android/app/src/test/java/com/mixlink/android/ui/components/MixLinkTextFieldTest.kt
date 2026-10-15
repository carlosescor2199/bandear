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

    // Pastes into a name already typed, cursor first: the old field's length filter kept the name
    // and cut only what was pasted, with the cursor after the part it kept.

    @Test
    fun aPasteThatFitsIsKeptWhereverItLands() {
        val shown = FieldText("Carlos", 0)
        assertEquals(FieldText("Dr Carlos", 3), keptEdit(shown, FieldText("Dr Carlos", 3), maxLength = 10))
        assertEquals(
            FieldText("Car-X-los", 5),
            keptEdit(FieldText("Carlos", 3), FieldText("Car-X-los", 5), maxLength = 10),
        )
        assertEquals(
            FieldText("Carlos Esc", 10),
            keptEdit(FieldText("Carlos", 6), FieldText("Carlos Esc", 10), maxLength = 10),
        )
    }

    @Test
    fun aPasteAtTheStartThatOverflowsKeepsTheWholeName() {
        val kept = keptEdit(FieldText("Carlos", 0), FieldText("Escudero Carlos", 9), maxLength = 10)
        assertEquals(FieldText("EscuCarlos", 4), kept)
    }

    @Test
    fun aPasteInTheMiddleThatOverflowsKeepsBothEndsOfTheName() {
        val kept = keptEdit(FieldText("Carlos", 3), FieldText("Car-X-Y-Zlos", 9), maxLength = 10)
        assertEquals(FieldText("Car-X-Ylos", 7), kept)
    }

    @Test
    fun aPasteAtTheEndThatOverflowsCutsOnlyThePaste() {
        val kept = keptEdit(FieldText("Carlos", 6), FieldText("Carlos Escudero", 15), maxLength = 10)
        assertEquals(FieldText("Carlos Esc", 10), kept)
    }

    @Test
    fun aPasteOverASelectionReplacesItAndCutsOnlyThePaste() {
        // "D" selected at the end of "Carlos D", then "Escudero" pasted over it.
        val kept = keptEdit(FieldText("Carlos D", 7, 8), FieldText("Carlos Escudero", 15), maxLength = 10)
        assertEquals(FieldText("Carlos Esc", 10), kept)
    }

    @Test
    fun atTheLimitAPasteInTheMiddleChangesNothing() {
        val shown = FieldText("Carlos", 3)
        assertEquals(shown, keptEdit(shown, FieldText("Carxxlos", 5), maxLength = 6))
    }

    @Test
    fun theCutOfAPasteNeverSplitsATwoUnitCharacter() {
        // Two guitar emoji, two UTF-16 units each, pasted before "Ana" with room for three units.
        val kept = keptEdit(FieldText("Ana", 0), FieldText("🎸🎸Ana", 4), maxLength = 6)
        assertEquals(FieldText("🎸Ana", 2), kept)
    }

    @Test
    fun withoutTheCursorThePasteIsFoundByComparingTheTexts() {
        // The plain-text form puts both cursors at the end, which does not explain a paste at the
        // start; the texts themselves show where it went.
        assertEquals("EscuCarlos", keptEdit(shown = "Carlos", proposed = "Escudero Carlos", maxLength = 10))
        assertEquals("CarlLos", keptEdit(shown = "CarLos", proposed = "Carlitos Los", maxLength = 7))
    }

    @Test
    fun comparingTheTextsNeverSplitsATwoUnitCharacter() {
        // A piano emoji before a guitar emoji: both start with the same UTF-16 unit, and the
        // limit leaves room for only one unit, so nothing of the piano is kept.
        assertEquals("🎸", keptEdit(shown = "🎸", proposed = "🎹🎸", maxLength = 3))
    }
}
