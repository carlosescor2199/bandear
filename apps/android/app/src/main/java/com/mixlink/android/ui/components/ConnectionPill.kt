package com.mixlink.android.ui.components

import com.mixlink.android.ControlState
import com.mixlink.android.ui.theme.ColorToken

/** What the status pill at the top of the Mix screen can say. */
enum class PillKind { CONNECTED, RECONNECTING, CONNECTING, NOT_CONNECTED }

/**
 * The pill for a control channel in [state].
 *
 * A channel that gave up ([ControlState.UNAVAILABLE]) or was stopped ([ControlState.CLOSED]) reads
 * as not connected: either way the musician has to pick a server again, and who holds the mix is
 * told elsewhere, not in the pill.
 */
internal fun pillKindFor(state: ControlState): PillKind = when (state) {
    ControlState.CONNECTING -> PillKind.CONNECTING
    ControlState.CONNECTED -> PillKind.CONNECTED
    ControlState.RECONNECTING -> PillKind.RECONNECTING
    ControlState.IDLE, ControlState.UNAVAILABLE, ControlState.CLOSED -> PillKind.NOT_CONNECTED
}

/** The words on the pill. They always say the state, so it is never told by colour alone. */
fun pillWords(kind: PillKind): String = when (kind) {
    PillKind.CONNECTED -> "Connected"
    PillKind.RECONNECTING -> "Reconnecting"
    PillKind.CONNECTING -> "Connecting"
    PillKind.NOT_CONNECTED -> "Not connected"
}

/** The colour of the pill's dot, background tint and border. */
fun pillToken(kind: PillKind): ColorToken = when (kind) {
    PillKind.CONNECTED -> ColorToken.GREEN
    PillKind.RECONNECTING -> ColorToken.AMBER
    PillKind.CONNECTING -> ColorToken.BLUE
    PillKind.NOT_CONNECTED -> ColorToken.NEUTRAL
}
