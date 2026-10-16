package com.mixlink.android.ui.state

import com.mixlink.android.ChannelInfo
import kotlin.math.roundToInt

private val PERCENT = 0..100

/**
 * The source channels one strip controls: [first] alone, or [first] and [second] as a stereo pair.
 * A strip is known by its [first] channel everywhere: the edits it reports, the sheet it opens and
 * the musician's channel.
 */
data class StripChannels(val first: Int, val second: Int? = null) {
    /** Every channel the strip writes, the first one first. */
    val sides: List<Int>
        get() = listOfNotNull(first, second)
}

/**
 * The strips for [count] source channels with the server's [details] (null when it announced none,
 * which shows every channel as a strip of its own, as always), left to right.
 *
 * A hidden channel has no strip. A channel that links to the next one makes one strip of the two,
 * when both are shown; a link that cannot make a pair (on the last channel, or with a hidden side,
 * which the server never sends) leaves each channel to its own flags. A channel past the details is
 * one the engineer never touched.
 */
fun stripLayout(count: Int, details: List<ChannelInfo>?): List<StripChannels> {
    fun detail(channel: Int): ChannelInfo? = details?.getOrNull(channel)
    val strips = ArrayList<StripChannels>(count)
    var channel = 0
    while (channel < count) {
        val shown = detail(channel)?.hidden != true
        val pairs = detail(channel)?.link == true && channel + 1 < count && shown && detail(channel + 1)?.hidden != true
        when {
            pairs -> strips += StripChannels(channel, channel + 1)
            shown -> strips += StripChannels(channel)
        }
        channel += if (pairs) 2 else 1
    }
    return strips
}

/**
 * The two pans a stereo pair's [balance] writes (0 left, 50 centre, 100 right): the first pan is
 * `max(0, 2b - 100)` and the second `min(100, 2b)`, the rule every client of the server uses. The
 * centre is the pair at full width, panned 0 and 100.
 */
fun balanceToPans(balance: Int): Pair<Int, Int> {
    val level = balance.coerceIn(PERCENT)
    return maxOf(0, 2 * level - 100) to minOf(100, 2 * level)
}

/** The balance a pair's two pans show: their mean (halves rounded up), which reads back [balanceToPans]. */
fun pansToBalance(first: Int, second: Int): Int = ((first + second) / 2.0).roundToInt().coerceIn(PERCENT)

/** How the strip of channel [index] is named: the engineer's name, or "CH n" when it has none. */
fun channelLabel(index: Int, detail: ChannelInfo?): String = detail?.name?.trim().orEmpty().ifEmpty { channelName(index) }
