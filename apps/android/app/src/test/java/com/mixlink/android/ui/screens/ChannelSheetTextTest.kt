package com.mixlink.android.ui.screens

import org.junit.Assert.assertEquals
import org.junit.Test

class ChannelSheetTextTest {
    @Test
    fun theLabelButtonIsNamedForItsChannel() {
        // Channel indexes are 0-based; the strip reads "CH 3" for index 2.
        assertEquals("Channel 3 settings", channelSettingsLabel(2))
    }

    @Test
    fun theLabelButtonReportsThePanItShows() {
        assertEquals("Pan left 30", channelPanState(35))
        assertEquals("Pan centre", channelPanState(50))
        assertEquals("Pan right 100", channelPanState(100))
    }

    @Test
    fun thePanControlIsNamedForItsChannel() {
        assertEquals("CH 3 pan", channelPanDescription("CH 3"))
    }

    @Test
    fun theSheetsKeysSayWhetherTheyAreOn() {
        assertEquals("Mute", muteKeyText(muted = false))
        assertEquals("Muted", muteKeyText(muted = true))
        assertEquals("Solo", soloKeyText(soloed = false))
        assertEquals("Soloed", soloKeyText(soloed = true))
    }
}
