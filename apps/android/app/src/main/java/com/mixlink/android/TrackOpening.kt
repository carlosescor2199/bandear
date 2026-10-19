package com.mixlink.android

/**
 * Opens the playback track the way [PmonPlayer] needs it, generic over the track so the steps are
 * tested without the platform: the large track first ([openLarge], with room for the longest fixed
 * buffer the desk may set), started ([start]) and asked for the smallest Auto cushion
 * ([applyLowestCushion], which answers the buffer size the track applied, in frames). It is kept
 * when [keepsLowLatency] says the size applied costs no latency; else it is closed ([close]) and the
 * minimum track ([openMinimum]) is opened and started instead.
 *
 * A track that fails to start, or to take the cushion, is closed before the failure goes on, so a
 * failed start never leaves a track holding the platform's audio resources; a close that fails too
 * is added to the failure as suppressed. Called once per stream start, never per packet.
 */
internal fun <T : Any> openTrackKeepingLowLatency(
    openLarge: () -> T,
    openMinimum: () -> T,
    start: (T) -> Unit,
    applyLowestCushion: (T) -> Int,
    keepsLowLatency: (appliedFrames: Int) -> Boolean,
    close: (T) -> Unit,
): T {
    val large = openLarge()
    val applied = closingOnFailure(large, close) {
        start(large)
        applyLowestCushion(large)
    }
    if (keepsLowLatency(applied)) return large
    close(large)
    val minimum = openMinimum()
    closingOnFailure(minimum, close) { start(minimum) }
    return minimum
}

private inline fun <T : Any, R> closingOnFailure(track: T, close: (T) -> Unit, step: () -> R): R =
    try {
        step()
    } catch (error: Throwable) {
        try {
            close(track)
        } catch (closeError: Throwable) {
            error.addSuppressed(closeError)
        }
        throw error
    }
