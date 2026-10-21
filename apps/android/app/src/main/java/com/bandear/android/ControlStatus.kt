package com.bandear.android

internal enum class ControlState { IDLE, CONNECTING, CONNECTED, RECONNECTING, UNAVAILABLE, CLOSED }

/**
 * Describes who currently owns this client's mix.
 *
 * The server keeps the last mix it accepted for a client IP and keeps applying it after the
 * control socket goes away. Once the server has acknowledged a mix, it owns that mix for the
 * rest of the session, so the client must not apply its own gain on top of it. Only a client
 * whose mix was never acknowledged can safely apply local processing, because the server is
 * still holding its neutral default in that case.
 */
internal fun controlStatusLabel(state: ControlState, remoteMixAcknowledged: Boolean): String = when (state) {
    ControlState.IDLE -> "not connected"
    ControlState.CONNECTING -> "connecting..."
    ControlState.CONNECTED -> if (remoteMixAcknowledged) {
        "connected - mix applied on the server"
    } else {
        "connected - waiting for the server to take over"
    }
    // The connection dropped and the channel is retrying on its own. Ownership is unchanged until
    // it reopens: a server that took the mix keeps applying it, otherwise this device still mixes.
    ControlState.RECONNECTING -> if (remoteMixAcknowledged) {
        "reconnecting - mix held at the server"
    } else {
        "reconnecting - mixing on this device"
    }
    ControlState.UNAVAILABLE, ControlState.CLOSED -> if (remoteMixAcknowledged) {
        "channel lost - mix held at the server's last setting"
    } else {
        "unavailable - mixing on this device"
    }
}
