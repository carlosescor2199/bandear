package com.bandear.android.ui.components

import com.bandear.android.ui.theme.ColorToken
import org.junit.Assert.assertEquals
import org.junit.Test

class LinkHealthTest {
    @Test
    fun goodIsGreen() {
        assertEquals(ColorToken.GREEN, linkHealthToken(LinkHealth.GOOD))
        assertEquals("Good", linkHealthWords(LinkHealth.GOOD))
    }

    @Test
    fun warningIsAmber() {
        assertEquals(ColorToken.AMBER, linkHealthToken(LinkHealth.WARNING))
        assertEquals("Warning", linkHealthWords(LinkHealth.WARNING))
    }

    @Test
    fun badSilentAndLostAreRedButSayWhichOne() {
        assertEquals(ColorToken.RED, linkHealthToken(LinkHealth.BAD))
        assertEquals(ColorToken.RED, linkHealthToken(LinkHealth.SILENT))
        assertEquals(ColorToken.RED, linkHealthToken(LinkHealth.LOST))
        assertEquals("Bad", linkHealthWords(LinkHealth.BAD))
        assertEquals("Silent", linkHealthWords(LinkHealth.SILENT))
        assertEquals("Lost", linkHealthWords(LinkHealth.LOST))
    }

    @Test
    fun unknownIsGrey() {
        assertEquals(ColorToken.NEUTRAL, linkHealthToken(LinkHealth.UNKNOWN))
        assertEquals("Unknown", linkHealthWords(LinkHealth.UNKNOWN))
    }

    @Test
    fun descriptionGivesTheWordAndItsMeaning() {
        assertEquals("Link health: Silent. No audio received.", linkHealthDescription(LinkHealth.SILENT))
        assertEquals("Link health: Unknown. No report yet.", linkHealthDescription(LinkHealth.UNKNOWN))
    }

    @Test
    fun everyLevelHasItsOwnWordsAndMeaning() {
        val levels = LinkHealth.entries
        assertEquals(levels.size, levels.map(::linkHealthWords).toSet().size)
        assertEquals(levels.size, levels.map(::linkHealthMeaning).toSet().size)
    }
}
