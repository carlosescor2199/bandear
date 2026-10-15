package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.PillKind
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.ServerEndpoint
import com.mixlink.android.ui.state.channelName
import com.mixlink.android.ui.theme.ColorToken

/** What the row of strips on the Mix screen shows: one strip per channel, or one per group. */
enum class MixView(val label: String) {
    CHANNELS("Channels"),
    GROUPS("Groups"),
}

/** The colour role of a strip's fader fill; the theme turns it into a colour. */
enum class StripFill { ACCENT, IDLE, MUTED }

/** The banner under the Mix header while the control channel is away; it always says it in words. */
data class ConnectionBanner(val token: ColorToken, val title: String, val detail: String, val busy: Boolean)

/** The level under a strip's fader. A muted strip says so in words, not only by its grey fill. */
fun stripValueText(level: Int, muted: Boolean): String = if (muted) "muted" else "$level%"

/**
 * A muted strip is greyed; the musician's own strip is amber while More of me lifts it; any other
 * strip is the idle grey.
 */
fun stripFillFor(muted: Boolean, mine: Boolean, moreOfMeOn: Boolean): StripFill = when {
    muted -> StripFill.MUTED
    mine && moreOfMeOn -> StripFill.ACCENT
    else -> StripFill.IDLE
}

/** What a screen reader says for a strip's fader: the channel, whether it is yours, whether muted. */
fun stripFaderDescription(label: String, mine: Boolean, muted: Boolean): String = buildString {
    append("$label level")
    if (mine) append(", your channel")
    if (muted) append(", muted")
}

/** The channels of a group, as their strips name them. */
fun groupMembersText(channels: List<Int>): String =
    if (channels.isEmpty()) "No channels" else channels.joinToString(" · ") { channelName(it) }

/** The small capitals over the row of strips, with how many it holds. */
fun sectionTitle(view: MixView, count: Int): String = "${view.label.uppercase()} · $count"

/** The master level in the Master card's corner; muted says so in words. */
fun masterValueText(volume: Int, muted: Boolean): String = if (muted) "muted" else "$volume%"

/** The master Mute key's words, which change with it so the state is not told by colour alone. */
fun masterMuteText(muted: Boolean): String = if (muted) "Muted" else "Mute"

/** The channel on the "My channel" button. */
fun myChannelText(label: String?): String = label ?: "None"

/** What a screen reader says for the "My channel" button. */
fun myChannelDescription(label: String?): String =
    if (label == null) "My channel: none chosen. Tap to choose" else "My channel: $label. Tap to change"

/** The musician's name at the top of the Mix screen, or the app's name before one is set. */
fun headerName(name: String): String = name.trim().ifEmpty { "MixLink" }

/** The server line under the name. */
fun headerServerText(server: ServerEndpoint?): String = server?.label ?: "No server"

/**
 * The banner for a session whose control channel is away, or null when there is none to show.
 *
 * While the channel retries it says who holds the mix meanwhile. Once it gave up, the musician has
 * to connect again, so the banner says how. Connecting and connected need no banner: the pill says
 * it.
 */
fun connectionBanner(connection: ConnectionUiState): ConnectionBanner? {
    if (!connection.sessionRunning) return null
    val held = connection.mixHeldByServer
    return when (connection.pill) {
        PillKind.RECONNECTING -> ConnectionBanner(
            token = ColorToken.AMBER,
            title = "Reconnecting to ${connection.server?.label ?: "the server"}",
            detail = if (held) {
                "Your mix is held at the server. It retries on its own."
            } else {
                "This phone keeps mixing until it is back. It retries on its own."
            },
            busy = true,
        )
        PillKind.NOT_CONNECTED -> ConnectionBanner(
            token = ColorToken.RED,
            title = "Lost the server",
            detail = if (held) {
                "Your mix stays at the server's last setting. Disconnect in Settings, then connect again."
            } else {
                "This phone keeps mixing. Disconnect in Settings, then connect again."
            },
            busy = false,
        )
        PillKind.CONNECTED, PillKind.CONNECTING -> null
    }
}
