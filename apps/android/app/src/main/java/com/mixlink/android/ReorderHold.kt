package com.mixlink.android

/**
 * How far behind the newest packet a sequence must be to start a new stream: about 2.5 s at
 * 100 packets/s.
 *
 * Wi-Fi reorders packets by a few positions, never by seconds, so a packet this far behind is not
 * late: the server restarted and its sequence began again. A server that restarts less than this
 * far into its previous stream is caught up with instead: its packets are dropped as late until its
 * sequence passes the last played one, which costs at most this many packets of audio once.
 */
internal const val STREAM_RESTART_PACKETS = 256L

/**
 * The longest hold the phone keeps room for, in packets: the longest delay the desk gives a
 * redundant copy (the `redundancy` message, see [parseRedundancyHold]).
 */
internal const val MAX_HOLD_PACKETS = 4

/** Room for the held packets, the one that makes the newest due and one that arrives meanwhile. */
private const val HOLD_SLOTS = MAX_HOLD_PACKETS + 2

/**
 * Puts received packets in sequence order before they are played, holding them back for
 * [holdPackets] packets so a late copy can fill the gap of its lost original.
 *
 * The desk can send each packet twice, the copy a few blocks after the original (the `redundancy`
 * message names the delay). A copy of a lost packet then arrives after newer ones; played on
 * arrival, as the phone did before, it would be out of order and is dropped. With a hold of that
 * many packets the phone plays every packet that many packets late, so the copy arrives in time and
 * is played in its place. That costs the hold in latency, only while the desk asks for one.
 *
 * The rules, packet by packet ([offer], then [next] until it answers null):
 * - The first packet starts the stream, wherever it joins.
 * - A packet newer than any before is held; one that fills a gap among those held is held too.
 * - A packet already played or given up on, or a second copy of one held, is dropped
 *   ([dropped]): it changes nothing, so it cannot rewind the stream or count a gap twice.
 * - A packet [STREAM_RESTART_PACKETS] or more behind the newest starts a new stream (the server
 *   restarted): what the old one left held is dropped, and the jump is not counted as loss.
 * - The oldest packet held is due once the newest is [holdPackets] packets ahead of it, and is
 *   played in order. A missing one is waited for until the newest is more than [holdPackets]
 *   ahead (its copy, sent right after the original [holdPackets] later, can no longer come in time),
 *   then skipped; the packets skipped are counted once, as [lostBefore] the next packet played.
 * - A packet played after filling a gap (it arrived after a newer one) is [recovered]: a delayed
 *   copy, or now and then a packet Wi-Fi reordered.
 * - [release] makes everything held due, for when no packet came for a while.
 *
 * With a hold of 0 every packet is due as it arrives: newer packets are played at once, a gap is
 * counted once, and late packets and duplicates are dropped, exactly as the phone always played.
 * An immediate copy arrives before the next packet, so it is played in its lost original's place
 * there too, and since it cannot be told from the original it is never counted as recovered.
 *
 * Single-threaded, on the receive thread, and nothing is allocated per packet: packets are held by
 * reference in arrays made once. The header's unsigned 64-bit sequence is compared as a signed
 * `Long`, which is exact for any stream shorter than billions of years.
 */
internal class ReorderHold<T : Any> {
    private val packets = arrayOfNulls<Any>(HOLD_SLOTS)
    private val sequences = LongArray(HOLD_SLOTS)
    private val arrivedLate = BooleanArray(HOLD_SLOTS)

    /**
     * The packets each one is held back, 0 to [MAX_HOLD_PACKETS] (a value outside is clamped).
     * A shorter hold makes what is now due playable at once; a longer one holds the next packets.
     */
    var holdPackets = 0
        set(value) {
            field = value.coerceIn(0, MAX_HOLD_PACKETS)
        }

    /** Packets the stream skipped since the hold was created, each counted once. */
    var lost = 0L
        private set

    /** Duplicate or late packets dropped since the hold was created. */
    var dropped = 0L
        private set

    /**
     * Packets played after filling a gap since the hold was created: each arrived for the first time
     * after a newer one and was still in time to be played in order. The number the phone reports as
     * `recovered`; they count as received, not lost.
     */
    var recovered = 0L
        private set

    /** How many packets the stream skipped right before the packet [next] last answered. */
    var lostBefore = 0L
        private set

    /** Whether the packet [next] last answered filled a gap (see [recovered]). */
    var lastRecovered = false
        private set

    private var started = false
    // The next sequence to play or give up on, and the newest one offered.
    private var head = 0L
    private var newest = 0L
    // The last sequence played, for the gap before the next one; none yet in a new stream.
    private var played = false
    private var lastPlayed = 0L
    // Everything held is due (see [release]) until nothing is.
    private var releasing = false
    // A packet that arrived too far ahead to be held with the others: held once [head] is close
    // enough, or once they are played, so [head] never passes it while it waits.
    private var waiting: Any? = null
    private var waitingSequence = 0L

    /**
     * Offers the received packet [sequence] carrying [packet]; false when it is dropped (a duplicate,
     * or too late to play). Then [next] answers what is due.
     */
    fun offer(sequence: Long, packet: T): Boolean {
        if (started && newest - sequence >= STREAM_RESTART_PACKETS) {
            restart()
        }
        if (!started) {
            started = true
            head = sequence
            newest = sequence
            store(sequence, packet, late = false)
            return true
        }
        if (sequence < head || isHeld(sequence) || (waiting != null && sequence == waitingSequence)) {
            dropped++
            return false
        }
        if (sequence > newest) {
            newest = sequence
            if (sequence - head >= HOLD_SLOTS) {
                // Far ahead: what is held now plays first, then this one is held (see [next]).
                if (waiting != null) {
                    // A second jump before the first was settled: the first is given up on.
                    dropped++
                }
                waiting = packet
                waitingSequence = sequence
                return true
            }
            store(sequence, packet, late = false)
        } else if (sequence - head < HOLD_SLOTS) {
            // Within what is held: it fills a gap, arriving after a newer one.
            store(sequence, packet, late = true)
        } else {
            // Behind a packet that arrived far ahead, and too far from what is held to hold.
            dropped++
            return false
        }
        return true
    }

    /**
     * The next packet to play now, in sequence order, or null when none is due; [lostBefore] and
     * [lastRecovered] then describe it. Call it until it answers null after every [offer].
     */
    @Suppress("UNCHECKED_CAST")
    fun next(): T? {
        while (started) {
            val ahead = waiting
            if (ahead != null && waitingSequence - head < HOLD_SLOTS) {
                // The packet that arrived far ahead is now close enough to hold with the others, so
                // the stream plays it in its place: nothing skips past it, or leaves it to rewind to.
                waiting = null
                store(waitingSequence, ahead, late = false)
            }
            val slot = slotOf(head)
            val present = packets[slot] != null && sequences[slot] == head
            if (present) {
                if (!releasing && newest - head < holdPackets) break
                val packet = packets[slot] as T
                packets[slot] = null
                // Never negative: head only moves forward past the last packet played.
                lostBefore = if (played) (head - lastPlayed - 1).coerceAtLeast(0L) else 0L
                lost += lostBefore
                lastRecovered = arrivedLate[slot]
                if (lastRecovered) recovered++
                played = true
                lastPlayed = head
                head++
                return packet
            }
            val held = oldestHeld()
            if (held == NONE_HELD) {
                // Nothing held: a packet that arrived far ahead is held now, the gap before it given
                // up on (counted when it is played).
                val far = waiting ?: break
                waiting = null
                head = waitingSequence
                store(waitingSequence, far, late = false)
                continue
            }
            // Missing: given up on once its copy can no longer come in time, as is every sequence
            // up to the next one held.
            if (!releasing && newest - head <= holdPackets) break
            head = if (releasing) held else minOf(held, newest - holdPackets)
        }
        releasing = false
        return null
    }

    /**
     * Makes everything held due: no packet came for a while, so nothing held is worth waiting for.
     * [next] then plays it in order.
     */
    fun release() {
        if (started) releasing = true
    }

    private fun restart() {
        packets.fill(null)
        waiting = null
        started = false
        played = false
        releasing = false
    }

    private fun slotOf(sequence: Long): Int = Math.floorMod(sequence, HOLD_SLOTS.toLong()).toInt()

    private fun isHeld(sequence: Long): Boolean {
        val slot = slotOf(sequence)
        return packets[slot] != null && sequences[slot] == sequence
    }

    private fun store(sequence: Long, packet: Any, late: Boolean) {
        val slot = slotOf(sequence)
        packets[slot] = packet
        sequences[slot] = sequence
        arrivedLate[slot] = late
    }

    /** The oldest sequence held from [head] on, or [NONE_HELD] (nothing boxed). */
    private fun oldestHeld(): Long {
        var oldest = Long.MAX_VALUE
        for (slot in 0 until HOLD_SLOTS) {
            if (packets[slot] != null && sequences[slot] >= head && sequences[slot] < oldest) {
                oldest = sequences[slot]
            }
        }
        return if (oldest == Long.MAX_VALUE) NONE_HELD else oldest
    }

    private companion object {
        const val NONE_HELD = Long.MIN_VALUE
    }
}
