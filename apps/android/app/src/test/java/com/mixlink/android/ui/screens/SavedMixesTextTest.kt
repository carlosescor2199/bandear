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
}
