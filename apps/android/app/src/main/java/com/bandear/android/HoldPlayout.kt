package com.bandear.android

/**
 * The receive loop's use of the [ReorderHold] and the [LinkStatsAccumulator], wake-up by wake-up,
 * kept apart from the socket and the track so the wiring is tested without either.
 *
 * Per wake-up the loop calls, in order: [onTimeout] when no packet came, [wake] with the hold the
 * desk asks for now, [offer] for a packet that came (skipping it when refused), then [nextToPlay]
 * until it answers null, playing each packet it answers. [poll] ends a report interval with the
 * hold's recovered count, so a packet a late copy brought counts in `packets_received` (it is
 * played) and in `recovered`, never in `packets_lost` (see [LinkStatsReport.recovered]).
 *
 * Single-threaded, on the receive thread; nothing is allocated per packet.
 */
internal class HoldPlayout<T : Any>(
    private val hold: ReorderHold<T>,
    private val linkStats: LinkStatsAccumulator,
) {
    /** The hold now in force, in packets, read from the desk's setting once per wake-up. */
    val holdPackets: Int get() = hold.holdPackets

    /** Every packet the stream skipped so far, each counted once (the receiver's own metrics). */
    val lost: Long get() = hold.lost

    /** No packet came within the receive timeout: counted, and nothing held is waited for longer. */
    fun onTimeout() {
        linkStats.onReceiveTimeout()
        hold.release()
    }

    /** A wake-up, with the hold the desk asks for now (a shorter one plays what is now due). */
    fun wake(holdPackets: Int) {
        hold.holdPackets = holdPackets
    }

    /** Offers a packet that arrived; false when it is dropped (a duplicate, or too late to play). */
    fun offer(sequence: Long, packet: T): Boolean = hold.offer(sequence, packet)

    /**
     * The next packet to play, in sequence order, counted as played at [nowMs] with the packets the
     * stream skipped right before it; null when none is due.
     */
    fun nextToPlay(nowMs: Long): T? {
        val packet = hold.next() ?: return null
        linkStats.onPacket(hold.lostBefore, nowMs)
        return packet
    }

    /** The link report due at [nowMs], with the hold's recovered count, or null when none is due. */
    fun poll(
        nowMs: Long,
        cumulativeUnderruns: Int,
        cushionMs: Double = Double.NaN,
        backlogMs: Double = Double.NaN,
    ): LinkStatsReport? =
        linkStats.poll(
            nowMs,
            cumulativeUnderruns,
            cushionMs = cushionMs,
            backlogMs = backlogMs,
            cumulativeRecovered = hold.recovered,
        )
}
