package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.panSpokenText
import com.mixlink.android.ui.state.ChannelUiState
import com.mixlink.android.ui.state.channelName

/**
 * The channel whose sheet is open among the strips' [channels]. [openChannel] is the channel index
 * a strip reported, so it is looked up by index, not by position in the list. Null when no sheet is
 * open or the server no longer announces that channel, which closes the sheet.
 */
fun sheetChannelFor(channels: List<ChannelUiState>, openChannel: Int?): ChannelUiState? =
    openChannel?.let { index -> channels.firstOrNull { it.index == index } }

/**
 * What a screen reader calls a strip's label button, which opens that channel's sheet: the channel
 * by its name ("Channel 3" when it has none), for a stereo pair that it is one, and whether the
 * engineer locked it.
 */
fun channelSettingsLabel(channel: ChannelUiState): String = buildString {
    append(if (channel.label == channelName(channel.index)) "Channel ${channel.index + 1}" else channel.label)
    if (channel.stereo) append(" stereo pair")
    append(" settings")
    if (channel.locked) append(", locked by the engineer")
}

/**
 * The pan a strip's label button reports to a screen reader, as it shows it under the label; a
 * [stereo] pair's pan is its balance.
 */
fun channelPanState(pan: Int, stereo: Boolean = false): String =
    "${if (stereo) "Balance" else "Pan"} ${panSpokenText(pan).lowercase()}"

/** The caption over the pan control in a channel's sheet; a [stereo] pair's is its balance. */
fun panTitle(stereo: Boolean): String = if (stereo) "BALANCE" else "PAN"

/** What a screen reader calls the pan control in a channel's sheet. */
fun channelPanDescription(label: String, stereo: Boolean = false): String = "$label ${if (stereo) "balance" else "pan"}"

/** The words that mark a stereo pair in its sheet, so it is not told by an icon alone. */
const val STEREO_TAG = "Stereo pair"

/** The words a locked channel's sheet shows over its disabled controls. */
const val LOCKED_NOTE = "Locked by the engineer"

/** The words on the sheet's Mute key, which change with it so its state is not told by colour alone. */
fun muteKeyText(muted: Boolean): String = if (muted) "Muted" else "Mute"

/** The words on the sheet's Solo key. */
fun soloKeyText(soloed: Boolean): String = if (soloed) "Soloed" else "Solo"
