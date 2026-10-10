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

    /**
     * The playback buffer to request, in frames, for a packet of [packetFrames] frames.
     *
     * The buffer's occupancy *is* the playback latency, so the cushion is kept as tight as the
     * measured network jitter allows. Measured on a Galaxy S23 over the fast path (HAL buffer 4 ms,
     * arrival jitter ~2 ms), the playback backlog was linear in the cushion: three packets gave
     * 53.9 ms, two gave 43.9 ms, and one gave 33.9 ms, with a fixed ~24 ms device pipeline under all
     * of them. One packet left the fast track reporting empty mix cycles, which is the audible-gap
     * risk, so the default keeps one packet of slack. The request is a lower bound; the platform
     * clamps it up to its own hardware minimum and is free to ignore it.
     */
    fun targetBufferFrames(packetFrames: Int): Int =
        (packetFrames.coerceAtLeast(0) * CUSHION_PACKETS).coerceAtLeast(1)

    /** Packet intervals of playback cushion to request. See [targetBufferFrames]. */
    private const val CUSHION_PACKETS = 2
}
