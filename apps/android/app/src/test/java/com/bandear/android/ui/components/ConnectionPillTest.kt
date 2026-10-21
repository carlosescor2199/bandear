package com.bandear.android.ui.components

import com.bandear.android.ControlState
import com.bandear.android.ui.theme.ColorToken
import org.junit.Assert.assertEquals
import org.junit.Test

class ConnectionPillTest {
    @Test
    fun liveStatesMapToTheirOwnPill() {
        assertEquals(PillKind.CONNECTING, pillKindFor(ControlState.CONNECTING))
        assertEquals(PillKind.CONNECTED, pillKindFor(ControlState.CONNECTED))
        assertEquals(PillKind.RECONNECTING, pillKindFor(ControlState.RECONNECTING))
    }

    @Test
    fun idleStoppedAndGivenUpChannelsReadAsNotConnected() {
        assertEquals(PillKind.NOT_CONNECTED, pillKindFor(ControlState.IDLE))
        assertEquals(PillKind.NOT_CONNECTED, pillKindFor(ControlState.UNAVAILABLE))
        assertEquals(PillKind.NOT_CONNECTED, pillKindFor(ControlState.CLOSED))
    }

    @Test
    fun everyPillSaysItsStateInWords() {
        assertEquals("Connected", pillWords(PillKind.CONNECTED))
        assertEquals("Reconnecting", pillWords(PillKind.RECONNECTING))
        assertEquals("Connecting", pillWords(PillKind.CONNECTING))
        assertEquals("Not connected", pillWords(PillKind.NOT_CONNECTED))
    }

    @Test
    fun everyPillHasItsOwnColour() {
        assertEquals(ColorToken.GREEN, pillToken(PillKind.CONNECTED))
        assertEquals(ColorToken.AMBER, pillToken(PillKind.RECONNECTING))
        assertEquals(ColorToken.BLUE, pillToken(PillKind.CONNECTING))
        assertEquals(ColorToken.NEUTRAL, pillToken(PillKind.NOT_CONNECTED))
    }

    @Test
    fun noTwoPillsShareWordsOrColour() {
        val kinds = PillKind.entries
        assertEquals(kinds.size, kinds.map(::pillWords).toSet().size)
        assertEquals(kinds.size, kinds.map(::pillToken).toSet().size)
    }
}
