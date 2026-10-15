package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.PillKind
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.ServerEndpoint
import com.mixlink.android.ui.theme.ColorToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class MixScreenTextTest {
    private val server = ServerEndpoint("192.168.1.34", 50001)

    @Test
    fun aStripShowsItsLevelAsAPercentage() {
        assertEquals("72%", stripValueText(level = 72, muted = false))
        assertEquals("0%", stripValueText(level = 0, muted = false))
    }

    @Test
    fun aMutedStripSaysMutedInWords() {
        assertEquals("muted", stripValueText(level = 72, muted = true))
    }

    @Test
    fun theMusiciansOwnStripIsAmberOnlyWhileMoreOfMeLiftsIt() {
        assertEquals(StripFill.ACCENT, stripFillFor(muted = false, mine = true, moreOfMeOn = true))
        assertEquals(StripFill.IDLE, stripFillFor(muted = false, mine = true, moreOfMeOn = false))
        assertEquals(StripFill.IDLE, stripFillFor(muted = false, mine = false, moreOfMeOn = true))
    }

    @Test
    fun aMutedStripIsGreyedEvenWhenItIsTheMusiciansOwn() {
        assertEquals(StripFill.MUTED, stripFillFor(muted = true, mine = true, moreOfMeOn = true))
        assertEquals(StripFill.MUTED, stripFillFor(muted = true, mine = false, moreOfMeOn = false))
    }

    @Test
    fun aFaderTellsAScreenReaderWhoseChannelItIsAndWhetherItIsMuted() {
        assertEquals("CH 1 level", stripFaderDescription("CH 1", mine = false, muted = false))
        assertEquals("CH 3 level, your channel", stripFaderDescription("CH 3", mine = true, muted = false))
        assertEquals("CH 4 level, muted", stripFaderDescription("CH 4", mine = false, muted = true))
        assertEquals("CH 3 level, your channel, muted", stripFaderDescription("CH 3", mine = true, muted = true))
    }

    @Test
    fun aGroupStripListsItsChannels() {
        assertEquals("CH 1 · CH 2", groupMembersText(listOf(0, 1)))
        assertEquals("CH 5", groupMembersText(listOf(4)))
        assertEquals("No channels", groupMembersText(emptyList()))
    }

    @Test
    fun theSectionTitleCountsWhatTheRowShows() {
        assertEquals("CHANNELS · 8", sectionTitle(MixView.CHANNELS, count = 8))
        assertEquals("GROUPS · 4", sectionTitle(MixView.GROUPS, count = 4))
    }

    @Test
    fun theMasterSaysMutedInWords() {
        assertEquals("85%", masterValueText(volume = 85, muted = false))
        assertEquals("muted", masterValueText(volume = 85, muted = true))
        assertEquals("Mute", masterMuteText(muted = false))
        assertEquals("Muted", masterMuteText(muted = true))
    }

    @Test
    fun theChannelPickerNamesTheChosenChannelOrNone() {
        assertEquals("CH 3", myChannelText("CH 3"))
        assertEquals("None", myChannelText(null))
        assertEquals("My channel: CH 3. Tap to change", myChannelDescription("CH 3"))
        assertEquals("My channel: none chosen. Tap to choose", myChannelDescription(null))
    }

    @Test
    fun theHeaderFallsBackWhenNoNameIsSet() {
        assertEquals("Carlos D", headerName("  Carlos D "))
        assertEquals("MixLink", headerName("   "))
    }

    @Test
    fun theHeaderNamesTheServerOfTheSession() {
        assertEquals("192.168.1.34:50001", headerServerText(server))
        assertEquals("No server", headerServerText(null))
    }

    @Test
    fun noBannerWhileConnectedOrConnecting() {
        assertNull(connectionBanner(running(PillKind.CONNECTED, held = true)))
        assertNull(connectionBanner(running(PillKind.CONNECTING, held = false)))
    }

    @Test
    fun noBannerWithoutASession() {
        assertNull(connectionBanner(ConnectionUiState()))
    }

    @Test
    fun reconnectingSaysTheServerHoldsTheMixWhenItDoes() {
        val banner = connectionBanner(running(PillKind.RECONNECTING, held = true))
        assertEquals(
            ConnectionBanner(
                token = ColorToken.AMBER,
                title = "Reconnecting to 192.168.1.34:50001",
                detail = "Your mix is held at the server. It retries on its own.",
                busy = true,
            ),
            banner,
        )
    }

    @Test
    fun reconnectingSaysThePhoneMixesWhenTheServerNeverTookOver() {
        val banner = connectionBanner(running(PillKind.RECONNECTING, held = false))
        assertEquals("This phone keeps mixing until it is back. It retries on its own.", banner?.detail)
    }

    @Test
    fun aControlChannelThatGaveUpAsksForANewConnect() {
        val held = connectionBanner(running(PillKind.NOT_CONNECTED, held = true))
        assertEquals(
            ConnectionBanner(
                token = ColorToken.RED,
                title = "Lost the server",
                detail = "Your mix stays at the server's last setting. Disconnect in Settings, then connect again.",
                busy = false,
            ),
            held,
        )
        val local = connectionBanner(running(PillKind.NOT_CONNECTED, held = false))
        assertEquals("This phone keeps mixing. Disconnect in Settings, then connect again.", local?.detail)
    }

    private fun running(pill: PillKind, held: Boolean) =
        ConnectionUiState(pill = pill, sessionRunning = true, server = server, mixHeldByServer = held)
}
