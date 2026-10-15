package com.mixlink.android

/** How often the phone reports its link to the server: once a second while the receiver runs. */
internal const val LINK_STATS_INTERVAL_MS = 1_000L

/** The longest interval one report may cover; the server refuses a report that claims more. */
internal const val MAX_LINK_STATS_INTERVAL_MS = 60_000L

/**
 * What the phone experienced over one report interval: the counts the server classifies the link
 * from. [packetsReceived] counts the packets played, so a dropped duplicate or late packet is not
 * received twice, and received plus lost is the stretch of the stream the interval covered.
 *
 * Public because the Activity, which is public, receives it through [PmonPlayer.Listener].
 */
data class LinkStatsReport(
    val intervalMs: Long,
    val packetsReceived: Long,
    val packetsLost: Long,
    val underruns: Long,
    val receiveTimeouts: Long,
)

/**
 * Collects the receive loop's counts into one [LinkStatsReport] about once a second.
 *
 * Time is passed in by the caller in milliseconds of a monotonic clock, so the accumulator is pure
 * and its timing is tested without waiting. The first interval starts at [startedAtMs]. The track's
 * underrun count is cumulative, so the accumulator keeps the last value it saw and reports the
 * difference; the playback cushion keeps its own last value and is not affected.
 *
 * Single-threaded: it lives on the receive thread, and between reports it only bumps counters.
 */
internal class LinkStatsAccumulator(startedAtMs: Long) {
    private var intervalStartedAtMs = startedAtMs
    private var packetsReceived = 0L
    private var packetsLost = 0L
    private var receiveTimeouts = 0L
    private var lastUnderruns = 0

    /** One played packet, with the [lostBefore] packets the stream skipped right before it. */
    fun onPacket(lostBefore: Long) {
        packetsReceived++
        packetsLost += lostBefore
    }

    /** One receive timeout: a wait for the next packet that ended with nothing. */
    fun onReceiveTimeout() {
        receiveTimeouts++
    }

    /** True once a report is due at [nowMs], so the caller reads the underrun count only then. */
    fun isDue(nowMs: Long): Boolean = nowMs - intervalStartedAtMs >= LINK_STATS_INTERVAL_MS

    /**
     * The report for the interval ending at [nowMs], or `null` when one is not due yet.
     *
     * The interval is the real time since the last report, clamped to what the server accepts, so a
     * late poll (the loop waits up to one receive timeout) is reported as the longer interval it was
     * and the server's rates stay honest. The interval counts start again from zero.
     */
    fun poll(nowMs: Long, cumulativeUnderruns: Int): LinkStatsReport? {
        if (!isDue(nowMs)) return null
        // A count that went down belongs to a new track, which starts from zero.
        val underruns = (cumulativeUnderruns - lastUnderruns).coerceAtLeast(0)
        lastUnderruns = cumulativeUnderruns
        val report = LinkStatsReport(
            intervalMs = (nowMs - intervalStartedAtMs).coerceIn(1L, MAX_LINK_STATS_INTERVAL_MS),
            packetsReceived = packetsReceived,
            packetsLost = packetsLost,
            underruns = underruns.toLong(),
            receiveTimeouts = receiveTimeouts,
        )
        intervalStartedAtMs = nowMs
        packetsReceived = 0L
        packetsLost = 0L
        receiveTimeouts = 0L
        return report
    }
}
