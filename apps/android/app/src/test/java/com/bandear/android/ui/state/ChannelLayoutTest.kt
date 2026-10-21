package com.bandear.android.ui.state

import com.bandear.android.ChannelInfo
import com.bandear.android.ChannelKind
import org.junit.Assert.assertEquals
import org.junit.Test

/** Which strips the phone shows for the server's channel details, and how a pair's balance is written. */
class ChannelLayoutTest {
    private val shown = ChannelInfo()
    private val hidden = ChannelInfo(hidden = true)
    private val linked = ChannelInfo(link = true)

    @Test
    fun withoutDetailsEveryChannelIsAStripOfItsOwn() {
        assertEquals(listOf(StripChannels(0), StripChannels(1), StripChannels(2)), stripLayout(3, null))
        assertEquals(emptyList<StripChannels>(), stripLayout(0, null))
    }

    @Test
    fun hiddenChannelsHaveNoStrip() {
        assertEquals(listOf(StripChannels(0), StripChannels(2)), stripLayout(4, listOf(shown, hidden, shown, hidden)))
    }

    @Test
    fun aLinkedChannelAndTheNextAreOneStripKnownByTheFirst() {
        val layout = stripLayout(5, listOf(shown, linked, shown, linked, shown))

        assertEquals(listOf(StripChannels(0), StripChannels(1, 2), StripChannels(3, 4)), layout)
        assertEquals(listOf(1, 2), layout[1].sides)
        assertEquals(listOf(0), layout[0].sides)
    }

    @Test
    fun aHiddenPairHasNoStrip() {
        val details = listOf(ChannelInfo(hidden = true, link = true), hidden, shown)

        assertEquals(listOf(StripChannels(2)), stripLayout(3, details))
    }

    @Test
    fun aLinkThatCannotMakeAPairLeavesEachChannelAsItIs() {
        // The server never sends these; the phone still shows what it can. A link on the last channel,
        // or into a hidden channel, makes no pair; a link on a pair's second side is part of the pair.
        assertEquals(listOf(StripChannels(0), StripChannels(1)), stripLayout(2, listOf(shown, linked)))
        assertEquals(listOf(StripChannels(0), StripChannels(2)), stripLayout(3, listOf(linked, hidden, shown)))
        assertEquals(listOf(StripChannels(0, 1), StripChannels(2)), stripLayout(3, listOf(linked, linked, shown)))
    }

    @Test
    fun channelsPastTheDetailsAreChannelsTheEngineerNeverTouched() {
        assertEquals(listOf(StripChannels(0, 1), StripChannels(2)), stripLayout(3, listOf(linked)))
        // Details past the count have no channel to describe.
        assertEquals(listOf(StripChannels(0)), stripLayout(1, listOf(linked, shown)))
    }

    @Test
    fun aStripIsNamedByTheEngineerOrByItsNumber() {
        assertEquals("Vox", channelLabel(0, ChannelInfo(" Vox ", ChannelKind.VOICE)))
        assertEquals("CH 2", channelLabel(1, ChannelInfo("  ")))
        assertEquals("CH 3", channelLabel(2, null))
    }

    @Test
    fun aBalanceIsWrittenAsBothPans() {
        assertEquals(0 to 0, balanceToPans(0))
        assertEquals(0 to 50, balanceToPans(25))
        assertEquals(0 to 100, balanceToPans(50))
        assertEquals(50 to 100, balanceToPans(75))
        assertEquals(100 to 100, balanceToPans(100))
        // Out of range reads as the nearest end.
        assertEquals(0 to 0, balanceToPans(-10))
        assertEquals(100 to 100, balanceToPans(140))
    }

    @Test
    fun twoPansReadBackAsTheirMean() {
        assertEquals(50, pansToBalance(0, 100))
        assertEquals(30, pansToBalance(10, 50))
        // Halves round up, as the desktop rounds them.
        assertEquals(1, pansToBalance(0, 1))
        for (balance in 0..100) {
            val (first, second) = balanceToPans(balance)
            assertEquals("balance $balance", balance, pansToBalance(first, second))
        }
    }
}
