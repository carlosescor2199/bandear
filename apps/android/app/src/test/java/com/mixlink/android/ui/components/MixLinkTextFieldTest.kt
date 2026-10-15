package com.mixlink.android.ui.components

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
}
