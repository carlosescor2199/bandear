package com.mixlink.android

/**
 * The playback cushion one receive loop keeps, in packets: adapted on Auto, held at the length the
 * desk set otherwise.
 *
 * - Auto ([PlaybackLatency.DESK_BUFFER_AUTO]): once per second of audio played ([onPlayed]) the
 *   controller takes the track's underrun count ([adjust]) and moves the cushion as
 *   [PlaybackLatency.nextCushion] decides, within 1 to 4 packets. The second is counted in played
 *   frames, not packets, so it is a second whatever the block size the server sends.
 * - Fixed ([follow] with a length in ms): the cushion is that length in whole packets, rounded up
 *   and clamped to what the track holds ([PlaybackLatency.fixedCushionPackets]), and nothing moves
 *   it; underruns are still noted, so the ones a fixed buffer had never count against Auto.
 * - Back to Auto, adaptation resumes from the cushion played, brought within the adaptive bounds,
 *   with its clean seconds counted anew.
 *
 * Only the receive thread uses it: it holds primitives, never allocates and never locks.
 */
internal class CushionController(start: Int = PlaybackLatency.START_CUSHION_PACKETS) {
    /** The cushion asked of the track, in packets. */
    var cushionPackets = start
        private set

    /** The desk's fixed length in force, in ms, or [PlaybackLatency.DESK_BUFFER_AUTO] on Auto. */
    var fixedMs = PlaybackLatency.DESK_BUFFER_AUTO
        private set

    private var cleanIntervals = 0
    private var lastUnderruns = 0
    private var framesSinceAdjust = 0L

    companion object {
        /** [onPacket]: nothing to do. */
        const val STEP_NONE = 0

        /** [onPacket]: the cushion changed, so the caller asks the track for it. */
        const val STEP_APPLY = 1

        /** [onPacket]: a second of audio was played, so the caller reads the underruns for [adjust]. */
        const val STEP_ADJUST = 2
    }

    /**
     * The receive loop's step for one packet of [packetSamples] interleaved samples
     * ([PlaybackLatency.CHANNELS] to a frame) in packets of [packetFrames]: follows the desk's buffer
     * [deskBufferMs] (read once per packet) at the track's [playbackRate], or the packet's
     * [packetRate] while no track set one, in a track of [capacityFrames], and counts the frames
     * played. Answers [STEP_NONE] or the flags [STEP_APPLY] and [STEP_ADJUST]; no allocation.
     */
    fun onPacket(
        deskBufferMs: Int,
        packetFrames: Int,
        packetSamples: Int,
        playbackRate: Int?,
        packetRate: Int,
        capacityFrames: Int,
    ): Int {
        val rate = playbackRate ?: packetRate
        var step = STEP_NONE
        if (follow(deskBufferMs, packetFrames, rate, capacityFrames)) step = step or STEP_APPLY
        if (onPlayed(packetSamples / PlaybackLatency.CHANNELS, rate)) step = step or STEP_ADJUST
        return step
    }

    /**
     * Follows the desk's buffer [requestMs] (Auto, or a fixed length in ms) with packets of
     * [packetFrames] at [sampleRate] in a track of [capacityFrames]. Answers whether the cushion
     * changed, so the caller asks the track for it; the same request again changes nothing.
     */
    fun follow(requestMs: Int, packetFrames: Int, sampleRate: Int, capacityFrames: Int): Boolean {
        if (requestMs == fixedMs) return false
        val before = cushionPackets
        fixedMs = requestMs
        cleanIntervals = 0
        cushionPackets = if (requestMs == PlaybackLatency.DESK_BUFFER_AUTO) {
            before.coerceIn(PlaybackLatency.MIN_CUSHION_PACKETS, PlaybackLatency.MAX_CUSHION_PACKETS)
        } else {
            PlaybackLatency.fixedCushionPackets(requestMs, packetFrames, sampleRate, capacityFrames)
        }
        return cushionPackets != before
    }

    /**
     * Counts [frames] played at [sampleRate]; answers true once a whole adjustment interval
     * ([PlaybackLatency.adjustIntervalFrames]) has been played since the last one. What runs past the
     * interval counts towards the next, so the adjustments keep to the second.
     */
    fun onPlayed(frames: Int, sampleRate: Int): Boolean {
        framesSinceAdjust += frames.coerceAtLeast(0)
        val interval = PlaybackLatency.adjustIntervalFrames(sampleRate)
        if (framesSinceAdjust < interval) return false
        framesSinceAdjust -= interval
        return true
    }

    /**
     * One adjustment, with the track's total underrun count [underrunsTotal]. On Auto the cushion
     * moves as [PlaybackLatency.nextCushion] decides; while fixed it never does. Answers whether it
     * changed.
     */
    fun adjust(underrunsTotal: Int): Boolean {
        val underruns = underrunsTotal - lastUnderruns
        lastUnderruns = underrunsTotal
        if (fixedMs != PlaybackLatency.DESK_BUFFER_AUTO) return false
        val decision = PlaybackLatency.nextCushion(cushionPackets, cleanIntervals, underruns)
        cleanIntervals = decision.cleanIntervals
        val changed = decision.cushion != cushionPackets
        cushionPackets = decision.cushion
        return changed
    }
}
