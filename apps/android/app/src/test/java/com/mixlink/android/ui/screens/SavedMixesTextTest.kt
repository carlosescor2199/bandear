package com.mixlink.android.ui.screens

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SavedMixesTextTest {
    @Test
    fun theListTitleCountsTheMixes() {
        assertEquals("YOUR MIXES · 3", savedMixesTitle(3))
        assertEquals("YOUR MIXES · 0", savedMixesTitle(0))
    }

    @Test
    fun onlyTheActiveMixHasADetailLine() {
        assertEquals("Active now", savedMixDetail(active = true))
        assertNull(savedMixDetail(active = false))
    }

    @Test
    fun aRenameNeedsANewNonBlankName() {
        assertTrue(canRename(from = "Soundcheck", to = "Main set"))
        assertFalse(canRename(from = "Soundcheck", to = "   "))
        assertFalse(canRename(from = "Soundcheck", to = " Soundcheck "))
    }

    @Test
    fun aSavedMixClearsTheField() {
        assertEquals("", nameAfterSave(typed = "Ballads", saved = true))
    }

    @Test
    fun aRefusedSaveKeepsWhatWasTyped() {
        assertEquals("Ballads", nameAfterSave(typed = "Ballads", saved = false))
        assertEquals("   ", nameAfterSave(typed = "   ", saved = false))
    }

    @Test
    fun aRenameConfirmsTheTrimmedName() {
        assertEquals("Main set", renameTarget(from = "Soundcheck", to = "  Main set "))
        assertEquals("Main set", renameTarget(from = "Soundcheck", to = "Main set"))
    }

    @Test
    fun aBlankOrUnchangedRenameConfirmsNothing() {
        assertNull(renameTarget(from = "Soundcheck", to = "   "))
        assertNull(renameTarget(from = "Soundcheck", to = " Soundcheck "))
    }
}
