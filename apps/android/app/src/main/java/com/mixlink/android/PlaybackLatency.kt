package com.mixlink.android

/**
 * Pure math for the client-side playback latency estimate.
 *
 * The estimate uses a single clock: this device's own audio render position. `AudioTrack.getTimestamp`
 * reports the frame currently being presented, so the frames that were written but are not presented
 * yet are the playback backlog, which is the jitter cushion plus the OS output buffer. No server clock
 * takes part, so no clock synchronization is needed and the value cannot be corrupted by clock skew.
 */
object PlaybackLatency {
    /** The playback track is always opened as interleaved stereo. */
    const val CHANNELS = 2

    /**
     * Frames written to the track but not presented yet.
     *
     * Clamped at zero: a render position that briefly runs ahead of the write count is a reporting
     * artifact, not negative latency.
     */
    fun pendingFrames(framesWritten: Long, framePosition: Long): Long =
        (framesWritten - framePosition).coerceAtLeast(0L)

    /** Playback backlog in milliseconds, or 0 when the sample rate cannot be used. */
    fun backlogMs(framesWritten: Long, framePosition: Long, sampleRate: Int): Double =
        if (sampleRate <= 0) {
            0.0
        } else {
            pendingFrames(framesWritten, framePosition) * 1000.0 / sampleRate
        }
}
