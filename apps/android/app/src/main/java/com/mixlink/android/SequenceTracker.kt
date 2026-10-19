package com.mixlink.android

/**
 * How far behind the last played packet a sequence must be to start a new stream: about 2.5 s at
 * 100 packets/s.
 *
 * Wi-Fi reorders packets by a few positions, never by seconds, so a packet this far behind is not
 * late: the server restarted and its sequence began again. A server that restarts less than this
 * far into its previous stream is caught up with instead: its packets are dropped as late until its
 * sequence passes the last played one, which costs at most this many packets of audio once.
 */
internal const val STREAM_RESTART_PACKETS = 256L

/**
 * How many sequences behind the last played one the tracker remembers whether they arrived: a late
 * packet within it is told apart as a duplicate or as one that fills a gap
 * ([SequenceTracker.recovered]).
 * One bit each, so 64 fit a `Long`; that is well past the longest delay of a redundant copy (4
 * blocks) and any reordering Wi-Fi does.
 */
internal const val RECOVERY_WINDOW_PACKETS = 64

/**
 * Decides, packet by packet, whether a received packet is played, and counts the packets the
 * stream skipped.
 *
 * The UDP header orders packets by a sequence number alone, so the tracker keeps the last played
 * one:
 * - the first packet starts the stream;
 * - a newer packet is played, and any gap before it is counted once as lost;
 * - a duplicate or late packet (not newer than the last played one) is dropped: playing it would be
 *   out of order, and it changes nothing, so it cannot rewind the stream and count a gap twice;
 * - a packet [STREAM_RESTART_PACKETS] or more behind the last played one starts a new stream (the
 *   server restarted) and is played, without counting the jump as loss.
 *
 * A late packet whose sequence never arrived before, within [RECOVERY_WINDOW_PACKETS] of the last
 * played one, filled a gap the stream skipped: it is counted as [recovered]. Only a delayed copy (the
 * desk's redundancy sends each packet again a few blocks later) or a packet Wi-Fi reordered can do
 * that, so reordering can add a few to the count without redundancy. A second copy of a packet that
 * arrived, or one older than the window, is not counted.
 *
 * Pure and single-threaded: it lives on the receive thread and allocates nothing per packet. The
 * header's unsigned 64-bit sequence is compared as a signed `Long`, which is exact for any stream
 * shorter than billions of years.
 */
internal class SequenceTracker {
    private var started = false
    private var lastPlayed = 0L

    /** Packets the stream skipped since the tracker was created, each counted once. */
    var lost = 0L
        private set

    /** Duplicate or late packets dropped since the tracker was created. */
    var dropped = 0L
        private set

    /**
     * Late packets that filled a gap since the tracker was created: each arrived for the first time
     * after a newer one was played (see the class documentation). They are still dropped, since
     * newer audio was already played, and stay counted in [lost]; the count says how many a delayed
     * copy brought, the number the phone reports as `recovered`.
     */
    var recovered = 0L
        private set

    // Bit i set: sequence `lastPlayed - i` arrived. Sequences before the stream started read as
    // arrived, so a reordered packet from before the first one is never counted.
    private var arrived = -1L

    /**
     * Accepts [sequence] if it is to be played, returning how many packets were lost right before
     * it (0 when it follows the last played one), or [DROPPED] when it must not be played.
     */
    fun accept(sequence: Long): Long {
        if (!started || lastPlayed - sequence >= STREAM_RESTART_PACKETS) {
            started = true
            lastPlayed = sequence
            arrived = -1L
            return 0L
        }
        if (sequence <= lastPlayed) {
            dropped++
            val behind = lastPlayed - sequence
            if (behind < RECOVERY_WINDOW_PACKETS) {
                val bit = 1L shl behind.toInt()
                if ((arrived and bit) == 0L) {
                    recovered++
                    arrived = arrived or bit
                }
            }
            return DROPPED
        }
        val gap = sequence - lastPlayed - 1
        lost += gap
        val ahead = sequence - lastPlayed
        arrived = if (ahead >= RECOVERY_WINDOW_PACKETS) 1L else (arrived shl ahead.toInt()) or 1L
        lastPlayed = sequence
        return gap
    }

    companion object {
        /** What [accept] returns for a duplicate or late packet, which must not be played. */
        const val DROPPED = -1L
    }
}
