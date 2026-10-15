package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.panSpokenText
import com.mixlink.android.ui.state.ChannelUiState

/**
 * The channel whose sheet is open among the strips' [channels]. [openChannel] is the channel index
 * a strip reported, so it is looked up by index, not by position in the list. Null when no sheet is
 * open or the server no longer announces that channel, which closes the sheet.
 */
fun sheetChannelFor(channels: List<ChannelUiState>, openChannel: Int?): ChannelUiState? =
    openChannel?.let { index -> channels.firstOrNull { it.index == index } }

/** What a screen reader calls a strip's label button, which opens that channel's sheet. */
fun channelSettingsLabel(channel: Int): String = "Channel ${channel + 1} settings"

/** The pan a strip's label button reports to a screen reader, as it shows it under the label. */
fun channelPanState(pan: Int): String = "Pan ${panSpokenText(pan).lowercase()}"

/** What a screen reader calls the pan control in a channel's sheet. */
fun channelPanDescription(label: String): String = "$label pan"

/** The words on the sheet's Mute key, which change with it so its state is not told by colour alone. */
fun muteKeyText(muted: Boolean): String = if (muted) "Muted" else "Mute"

/** The words on the sheet's Solo key. */
fun soloKeyText(soloed: Boolean): String = if (soloed) "Soloed" else "Solo"
