package com.mixlink.android

/**
 * Pure math for the client-side playback latency estimate and its adaptive buffer.
 *
 * The estimate uses a single clock: this device's own audio render position. `AudioTrack.getTimestamp`
 * reports the frame currently being presented, so the frames that were written but are not presented
 * yet are the playback backlog, which is the jitter cushion plus the OS output buffer. No server clock
 * takes part, so no clock synchronization is needed and the value cannot be corrupted by clock skew.
 */
object PlaybackLatency {
    /** The playback track is always opened as interleaved stereo. */
    const val CHANNELS = 2

    /** Cushion bounds, in packet intervals. The adaptive controller stays inside them. */
    const val MIN_CUSHION_PACKETS = 1
    const val MAX_CUSHION_PACKETS = 4
    const val START_CUSHION_PACKETS = 2

    /** Clean adjustment intervals required before the cushion may shrink by one packet. */
    private const val CLEAN_INTERVALS_BEFORE_SHRINK = 3

    /** The desk's buffer when it leaves the cushion to the phone (Auto); any other is fixed, in ms. */
    const val DESK_BUFFER_AUTO = 0

    /** The fixed buffer lengths the desk may set, in milliseconds (the server checks the same). */
    const val MIN_FIXED_BUFFER_MS = 5
    const val MAX_FIXED_BUFFER_MS = 500

    /** How much audio is played between two adjustments of the adaptive cushion: one second. */
    const val ADJUST_INTERVAL_MS = 1_000

    /**
     * The capacity to open the track with, in frames: the longest fixed buffer
     * ([MAX_FIXED_BUFFER_MS]) at [sampleRate], in whole packets of [packetFrames] so its last packet
     * fits too (just the frames when the packet size is unknown). The capacity is only the most the
     * buffer may be set to; what the track keeps queued is what [targetBufferFrames] asks of it, so
     * a large capacity costs memory (about 94 KB at 48 kHz), not latency. This is the platform's
     * documented way to tune latency: open with room, then set the buffer size down.
     */
    fun trackCapacityFrames(sampleRate: Int, packetFrames: Int): Int {
        val frames = ceilDiv(MAX_FIXED_BUFFER_MS.toLong() * sampleRate.coerceAtLeast(0), 1_000L)
        if (packetFrames <= 0) return frames.toInt()
        return (ceilDiv(frames, packetFrames.toLong()) * packetFrames).toInt()
    }

    /**
     * The cushion, in packets, for the desk's fixed buffer of [fixedMs] milliseconds: the length at
     * [sampleRate] rounded up to whole packets of [packetFrames] (so the phone never plays less than
     * the engineer set), at least one packet, and at most the whole packets the track holds
     * ([capacityFrames], the track's capacity). Before a packet sized anything it is one packet.
     */
    fun fixedCushionPackets(fixedMs: Int, packetFrames: Int, sampleRate: Int, capacityFrames: Int): Int {
        if (packetFrames <= 0 || sampleRate <= 0) return 1
        val packets = ceilDiv(fixedMs.coerceAtLeast(0).toLong() * sampleRate, 1_000L * packetFrames)
        val most = (capacityFrames / packetFrames).coerceAtLeast(1)
        return packets.coerceIn(1L, most.toLong()).toInt()
    }

    /**
     * The audio played between two adjustments, in frames: [ADJUST_INTERVAL_MS] at [sampleRate]
     * (48 kHz when the rate cannot be used), so the controller judges the same span of time whatever
     * the block size the server sends (256-frame blocks are about 187 packets a second, 480-frame
     * ones 100).
     */
    fun adjustIntervalFrames(sampleRate: Int): Int {
        val rate = if (sampleRate > 0) sampleRate else 48_000
        return (rate.toLong() * ADJUST_INTERVAL_MS / 1_000L).toInt().coerceAtLeast(1)
    }

    private fun ceilDiv(value: Long, divisor: Long): Long = (value + divisor - 1) / divisor

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
     * arrival jitter ~2 ms), the backlog was linear in the cushion: three packets gave 53.9 ms, two
     * gave 43.9 ms, one gave 33.9 ms, over a fixed ~24 ms device pipeline. The request is a lower
     * bound; the platform clamps it up to its own hardware minimum and is free to ignore it.
     */
    fun targetBufferFrames(packetFrames: Int, cushionPackets: Int): Int =
        (packetFrames.coerceAtLeast(0) * cushionPackets.coerceAtLeast(1)).coerceAtLeast(1)

    /**
     * The cushion the phone aims for, in milliseconds: the buffer [targetBufferFrames] asks the track
     * for, at the track's [sampleRate]. Milliseconds, so phones with different packet sizes read on
     * one scale. `NaN` (not measured) before the first packet sized it or at an unusable rate.
     */
    fun cushionMs(packetFrames: Int, cushionPackets: Int, sampleRate: Int): Double =
        if (packetFrames <= 0 || sampleRate <= 0) {
            Double.NaN
        } else {
            targetBufferFrames(packetFrames, cushionPackets) * 1000.0 / sampleRate
        }

    /** The cushion and the backlog a link report ends its interval with, in milliseconds. */
    data class ReportExtras(val cushionMs: Double, val backlogMs: Double)

    /**
     * What the link report due now ends its interval with, from the receive loop's track inputs.
     *
     * - The cushion is the buffer asked of the track ([cushionMs]): [cushionPackets] packets of
     *   [packetFrames] frames at the track's [playbackRate]; not measured (`NaN`) before the first
     *   packet sized it or while no track set the rate (null).
     * - The backlog is the audio queued now ([backlogMs]): the [framesWritten] past the
     *   [presentedFrame] the track's timestamp read; not measured (`NaN`) when there is no track or
     *   its timestamp could not be read ([timestampRead] false). A track always has its rate, so a
     *   read timestamp with no rate only happens in a test and reads as 0, as [backlogMs] reads it.
     */
    fun reportExtras(
        packetFrames: Int,
        cushionPackets: Int,
        playbackRate: Int?,
        timestampRead: Boolean,
        framesWritten: Long,
        presentedFrame: Long,
    ): ReportExtras {
        val rate = playbackRate ?: 0
        return ReportExtras(
            cushionMs = cushionMs(packetFrames, cushionPackets, rate),
            backlogMs = if (timestampRead) backlogMs(framesWritten, presentedFrame, rate) else Double.NaN,
        )
    }

    /** One adjustment outcome: the cushion to use next, and the clean-interval count to carry. */
    data class CushionDecision(val cushion: Int, val cleanIntervals: Int)

    /**
     * The next cushion after one adjustment interval.
     *
     * Because the buffer's occupancy is the playback latency, the controller walks the cushion down
     * while the track reports no new underruns and jumps back up the moment it does, so the client
     * settles on the smallest buffer this device and this network can actually sustain instead of a
     * fixed guess. A few clean intervals are required before shrinking, so one quiet second cannot
     * ratchet the buffer to the floor and a burst right after does not immediately undo it.
     */
    fun nextCushion(
        currentCushion: Int,
        cleanIntervals: Int,
        underrunsInInterval: Int,
    ): CushionDecision = if (underrunsInInterval > 0) {
        CushionDecision((currentCushion + 1).coerceAtMost(MAX_CUSHION_PACKETS), 0)
    } else {
        val clean = cleanIntervals + 1
        if (clean >= CLEAN_INTERVALS_BEFORE_SHRINK && currentCushion > MIN_CUSHION_PACKETS) {
            CushionDecision(currentCushion - 1, 0)
        } else {
            CushionDecision(currentCushion, clean)
        }
    }
}
