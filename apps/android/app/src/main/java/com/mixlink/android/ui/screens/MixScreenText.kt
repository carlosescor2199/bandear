package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.PillKind
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.ServerEndpoint
import com.mixlink.android.ui.state.leaderName
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

/**
 * What a screen reader says for a strip's fader: the channel, whether it is yours, whether muted,
 * and whether the engineer locked it.
 */
fun stripFaderDescription(
    label: String,
    mine: Boolean,
    muted: Boolean,
    locked: Boolean = false,
    following: String? = null,
): String = buildString {
    append("$label level")
    if (mine) append(", your channel")
    if (muted) append(", muted")
    append(heldWords(locked, following))
}

/** The words a locked strip shows in place of its group, with the lock, so it is not told by a fade. */
const val LOCKED_TAG = "Locked"

/**
 * A control's channel [label] for a screen reader, saying the engineer locked it when [locked] and
 * whose mix it follows while [following] is set.
 */
fun lockedLabel(label: String, locked: Boolean, following: String? = null): String = label + heldWords(locked, following)

/** The title over the Mix screen, and the channel sheet's note, while the mix follows [following]'s. */
fun followingTitle(following: String): String = "Following ${leaderName(following)}'s mix"

/** What the following banner says under its title: what follows and what stays the musician's. */
const val FOLLOWING_DETAIL = "Your channels and groups follow that mix. Your master, level cap and mute are still yours."

/** The words a follower's strips show in place of their group, so it is not told by a fade. */
const val FOLLOWING_TAG = "Following"

/** What a screen reader says for a group's fader, and whose mix it follows while [following] is set. */
fun groupFaderDescription(name: String, following: String?): String = "$name group level" + heldWords(false, following)

/** ", locked by the engineer" and ", following Ana's mix", as they apply, for a screen reader. */
private fun heldWords(locked: Boolean, following: String?): String = buildString {
    if (locked) append(", locked by the engineer")
    if (following != null) append(", ${followingTitle(following).replaceFirstChar { it.lowercase() }}")
}

/**
 * The engineer's limit under the Master card's level cap, with what the musician hears at most (the
 * lower of the phone's [cap] and the [limit]); null when the engineer set no limit.
 */
fun engineerLimitText(cap: Int, limit: Int): String? =
    if (limit >= 100) null else "Limited to $limit% by the engineer · at most ${minOf(cap, limit)}%"

/** What a screen reader calls the level cap control; it says when the engineer's limit applies too. */
fun levelCapDescription(cap: Int, limit: Int): String = buildString {
    append("Level cap, the loudest this phone plays")
    if (limit < 100) append(". The engineer's limit of $limit% also applies: at most ${minOf(cap, limit)}%")
}

/** The channels of a group, as their strips name them ([com.mixlink.android.ui.state.GroupUiState.members]). */
fun groupMembersText(members: List<String>): String =
    if (members.isEmpty()) "No channels" else members.joinToString(" · ")

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
