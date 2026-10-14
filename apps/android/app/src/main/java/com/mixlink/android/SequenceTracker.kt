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
     * Accepts [sequence] if it is to be played, returning how many packets were lost right before
     * it (0 when it follows the last played one), or [DROPPED] when it must not be played.
     */
    fun accept(sequence: Long): Long {
        if (!started || lastPlayed - sequence >= STREAM_RESTART_PACKETS) {
            started = true
            lastPlayed = sequence
            return 0L
        }
        if (sequence <= lastPlayed) {
            dropped++
            return DROPPED
        }
        val gap = sequence - lastPlayed - 1
        lost += gap
        lastPlayed = sequence
        return gap
    }

    companion object {
        /** What [accept] returns for a duplicate or late packet, which must not be played. */
        const val DROPPED = -1L
    }
}
