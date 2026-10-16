package com.mixlink.android.ui.screens

import com.mixlink.android.ChannelKind
import com.mixlink.android.ui.state.ChannelUiState
import com.mixlink.android.ui.state.channelName
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ChannelSheetTextTest {
    private fun channel(index: Int) = ChannelUiState(
        index = index,
        label = channelName(index),
        group = null,
        level = 80,
        pan = 50,
        muted = false,
        soloed = false,
    )

    @Test
    fun theOpenSheetFindsItsChannelByIndexNotByPosition() {
        // A strip reports its channel's index; the list's positions need not match it.
        val channels = listOf(channel(2), channel(5))

        assertEquals(channel(5), sheetChannelFor(channels, openChannel = 5))
        assertEquals(channel(2), sheetChannelFor(channels, openChannel = 2))
        assertNull(sheetChannelFor(channels, openChannel = 1))
    }

    @Test
    fun aChannelTheServerNoLongerAnnouncesHasNoSheet() {
        val channels = (0 until 4).map(::channel)

        assertNull(sheetChannelFor(channels, openChannel = 6))
        assertNull(sheetChannelFor(emptyList(), openChannel = 0))
    }

    @Test
    fun noSheetIsOpenWithoutAChannel() {
        assertNull(sheetChannelFor((0 until 4).map(::channel), openChannel = null))
    }

    @Test
    fun theLabelButtonIsNamedForItsChannel() {
        // Channel indexes are 0-based; the strip reads "CH 3" for index 2.
        assertEquals("Channel 3 settings", channelSettingsLabel(channel(2)))
        assertEquals("Vox settings", channelSettingsLabel(channel(0).copy(label = "Vox", kind = ChannelKind.VOICE)))
    }

    @Test
    fun aLockedChannelsLabelButtonSaysTheEngineerLockedIt() {
        assertEquals("Channel 3 settings, locked by the engineer", channelSettingsLabel(channel(2).copy(locked = true)))
        assertEquals("Keys stereo pair settings, locked by the engineer", channelSettingsLabel(channel(3).copy(label = "Keys", pairedWith = 4, locked = true)))
        assertEquals("Locked by the engineer", LOCKED_NOTE)
    }

    @Test
    fun aFollowersLabelButtonSaysWhoseMixItFollows() {
        assertEquals("Channel 3 settings, following Ana's mix", channelSettingsLabel(channel(2).copy(following = "Ana")))
        assertEquals(
            "Channel 3 settings, locked by the engineer, following Ana's mix",
            channelSettingsLabel(channel(2).copy(locked = true, following = "Ana")),
        )
    }

    @Test
    fun aStereoPairsLabelButtonSaysItIsAPair() {
        assertEquals("Keys stereo pair settings", channelSettingsLabel(channel(3).copy(label = "Keys", pairedWith = 4)))
        assertEquals("Channel 4 stereo pair settings", channelSettingsLabel(channel(3).copy(pairedWith = 4)))
    }

    @Test
    fun theLabelButtonReportsThePanItShows() {
        assertEquals("Pan left 30", channelPanState(35))
        assertEquals("Pan centre", channelPanState(50))
        assertEquals("Pan right 100", channelPanState(100))
    }

    @Test
    fun aStereoPairsPanIsABalance() {
        assertEquals("Balance left 30", channelPanState(35, stereo = true))
        assertEquals("Balance centre", channelPanState(50, stereo = true))
        assertEquals("BALANCE", panTitle(stereo = true))
        assertEquals("PAN", panTitle(stereo = false))
        assertEquals("Keys balance", channelPanDescription("Keys", stereo = true))
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
