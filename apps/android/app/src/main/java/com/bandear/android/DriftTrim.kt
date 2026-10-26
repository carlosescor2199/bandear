package com.bandear.android

import java.util.ArrayDeque

/**
 * Removes a few frames from a PCM packet without a click.
 *
 * Used to give back the surplus a faster desk clock piles up ([DriftTrimmer]). The frames go from the
 * middle of the packet: the audio before the cut fades out into the audio after it over
 * [FADE_FRAMES] frames, so the join is a short crossfade and never a hard cut. Pure, no state.
 */
object PcmSplice {
    /** The length of the crossfade over the join, in frames: about 1.3 ms at 48 kHz. */
    const val FADE_FRAMES = 64

    /**
     * [samples] (16-bit interleaved, [channels] per frame) without [frames] frames, so the answer is
     * exactly [frames] frames shorter. At most half the packet can go, so a tiny packet still plays; a
     * packet that cannot be split into whole frames, or nothing to remove, answers [samples] itself.
     */
    fun removeFrames(samples: ShortArray, channels: Int, frames: Int): ShortArray {
        if (channels <= 0 || samples.size % channels != 0) return samples
        val total = samples.size / channels
        val removed = minOf(frames, total / 2)
        if (removed <= 0) return samples

        val remaining = total - removed
        val fade = minOf(FADE_FRAMES, remaining)
        // The join sits in the middle of what is left, as far from both edges as it can be.
        val start = (remaining - fade) / 2
        val out = ShortArray(remaining * channels)
        System.arraycopy(samples, 0, out, 0, start * channels)
        for (index in 0 until fade) {
            // Linear weights that add up to one, so a steady signal stays exactly steady.
            val weightNumerator = index + 1
            val weightDenominator = fade + 1
            for (channel in 0 until channels) {
                val before = samples[(start + index) * channels + channel].toInt()
                val after = samples[(start + removed + index) * channels + channel].toInt()
                out[(start + index) * channels + channel] =
                    (before + (after - before) * weightNumerator / weightDenominator).toShort()
            }
        }
        val tail = start + fade
        System.arraycopy(samples, (tail + removed) * channels, out, tail * channels, (remaining - tail) * channels)
        return out
    }
}

/**
 * When, and how much, to remove from the audio queued before the track so that a desk clock that runs
 * faster than the phone's DAC cannot make the heard latency grow without bound.
 *
 * Fed once per packet with the frames queued in all ([DriftTrimmer.onPacket]): the audio still to
 * play ahead of the packet about to be written, on the track and before it, against the target the
 * track is sized to. It answers how many frames to cut from that packet ([PcmSplice]); 0 almost
 * always.
 *
 * - It acts only on a surplus that persists: the smallest surplus seen over a [WINDOW_SECONDS] window
 *   has to stay above half a packet, so a burst or a reorder (which comes back down within the
 *   window) never triggers it. It judges again at the end of every window, so it also stands down
 *   by itself once the surplus is gone.
 * - It cuts [SPLICE_FRAMES] at a time at most (a quarter of a packet for tiny packets), and the
 *   frames it may cut accrue at [MAX_TRIM_PPM] of the audio played, so it never removes more than
 *   0.05% of the audio: more than enough for a crystal mismatch of 200 ppm (about 10 frames a
 *   second at 48 kHz), and far below what can be heard as a pitch change.
 * - It never cuts below the target: a splice needs the surplus now to cover the splice plus a
 *   quarter of a packet.
 *
 * Time is the audio played, not a clock, so it is deterministic. Single-threaded, on the receive
 * thread; allocates nothing.
 */
class DriftTrimmer(sampleRate: Int) {
    private val windowFrames = WINDOW_SECONDS * (if (sampleRate > 0) sampleRate else 48_000).toLong()
    private var framesInWindow = 0L
    private var windowMinimum = Long.MAX_VALUE
    private var armed = false

    /** The frames that may be cut, in millionths of a frame; never more than one splice banked. */
    private var creditMicroFrames = 0L

    /**
     * Frames to cut from the packet of [packetFrames] frames about to be written, given the
     * [queuedFrames] still to play ahead of it (on the track and before it) and the [targetFrames]
     * the track is sized to.
     */
    fun onPacket(packetFrames: Int, queuedFrames: Long, targetFrames: Long): Int {
        if (packetFrames <= 0) return 0
        if (framesInWindow >= windowFrames) {
            armed = windowMinimum > packetFrames / 2
            framesInWindow = 0L
            windowMinimum = Long.MAX_VALUE
        }
        val surplus = queuedFrames - targetFrames
        windowMinimum = minOf(windowMinimum, surplus)
        framesInWindow += packetFrames
        creditMicroFrames = minOf(
            creditMicroFrames + packetFrames.toLong() * MAX_TRIM_PPM,
            SPLICE_FRAMES * MICRO,
        )
        if (!armed) return 0

        val splice = minOf(SPLICE_FRAMES, maxOf(1L, (packetFrames / 4).toLong()))
        if (creditMicroFrames < splice * MICRO) return 0
        if (surplus < splice + packetFrames / 4) return 0
        creditMicroFrames -= splice * MICRO
        return splice.toInt()
    }

    companion object {
        /** How long a surplus has to persist before it is trimmed, in seconds of audio. */
        const val WINDOW_SECONDS = 2

        /** The most frames cut from one packet. */
        const val SPLICE_FRAMES = 8L

        /** The most that is cut, in parts per million of the audio played: 0.05%. */
        const val MAX_TRIM_PPM = 500L

        private const val MICRO = 1_000_000L
    }
}

/**
 * The packets waiting in the app to be written to the track, in order, counted in frames.
 *
 * The receive loop drains every datagram the socket already holds into the reorder hold and from
 * there into this queue, so the audio that a blocked write would otherwise leave unseen in the
 * kernel's socket buffer is measurable ([queuedFrames]). It is bounded: past [maxFrames] the oldest
 * packets are dropped, so a track that stalls for a long time catches up instead of growing the
 * queue without limit; the newest packet is always kept.
 *
 * Single-threaded, on the receive thread.
 */
internal class PlayoutQueue<T : Any>(private val maxFrames: Long) {
    private class Entry<T>(val item: T, val frames: Int)

    private val entries = ArrayDeque<Entry<T>>()

    /** The frames waiting. */
    var queuedFrames = 0L
        private set

    /** The packets waiting. */
    val size: Int get() = entries.size

    /** Queues [item] of [frames] frames; answers the frames dropped from the front to stay in bounds. */
    fun add(item: T, frames: Int): Long {
        entries.addLast(Entry(item, frames))
        queuedFrames += frames
        var dropped = 0L
        while (queuedFrames > maxFrames && entries.size > 1) {
            val oldest = entries.removeFirst()
            queuedFrames -= oldest.frames
            dropped += oldest.frames
        }
        return dropped
    }

    /** The oldest packet, or null when none waits. */
    fun poll(): T? {
        val entry = entries.pollFirst() ?: return null
        queuedFrames -= entry.frames
        return entry.item
    }
}
