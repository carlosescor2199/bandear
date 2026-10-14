package com.mixlink.android

/** How often the phone reports its link to the server: once a second while the receiver runs. */
internal const val LINK_STATS_INTERVAL_MS = 1_000L

/** The longest interval one report may cover; the server refuses a report that claims more. */
internal const val MAX_LINK_STATS_INTERVAL_MS = 60_000L

/**
 * How long the phone waits for its first packet before it reports a link that hears nothing.
 *
 * The receiver starts before the server knows where to send audio, and the server judges a report
 * with no packets as bad, so a report sent before the audio arrives would show a healthy phone as
 * red. Three seconds covers connecting, registering and the first packets, and matches how long
 * the server keeps a report fresh.
 */
internal const val LINK_STATS_STARTUP_GRACE_MS = 3_000L

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
 * and its timing is tested without waiting. The track's underrun count is cumulative, so the
 * accumulator keeps the last value it saw and reports the difference; the playback cushion keeps
 * its own last value and is not affected.
 *
 * Reports are held back until the stream starts (the first packet is played) or
 * [LINK_STATS_STARTUP_GRACE_MS] has passed since [startedAtMs], whichever comes first; from then
 * on one is due every [LINK_STATS_INTERVAL_MS]:
 * - The stream starts within the grace: the first interval starts at the first packet, so the
 *   first report comes one interval later and describes the stream alone. The silence before it
 *   was startup, not the link, and is not reported.
 * - The grace passes with no packet: the first report covers everything since [startedAtMs] (the
 *   grace, or longer when the poll is late, clamped like any interval). It carries no packets, no
 *   timeouts and no underruns, which is the finding: the phone heard nothing for that long.
 * - A stream that starts after the grace joins the interval already running.
 *
 * Single-threaded: it lives on the receive thread, and between reports it only bumps counters.
 */
internal class LinkStatsAccumulator(private val startedAtMs: Long) {
    private var intervalStartedAtMs = startedAtMs
    private var packetsReceived = 0L
    private var packetsLost = 0L
    private var receiveTimeouts = 0L
    private var lastUnderruns = 0
    private var streamStarted = false

    /**
     * One played packet, arriving at [nowMs], with the [lostBefore] packets the stream skipped
     * right before it.
     */
    fun onPacket(lostBefore: Long, nowMs: Long) {
        if (!streamStarted && !gracePassed(nowMs)) {
            // Reports start with the stream: the first interval runs from this packet.
            intervalStartedAtMs = nowMs
        }
        streamStarted = true
        packetsReceived++
        packetsLost += lostBefore
    }

    /**
     * One receive timeout: a wait for the next packet that ended with nothing.
     *
     * Counted only once the stream has started (a packet was played). The receiver starts before
     * the server knows where to send audio, so a wait before the first packet says nothing about
     * the link, and counting it would show a phone that connects cleanly as red for its first
     * second. Once started, the stream stays started: a wait while a restarted server comes back is
     * a real gap in the audio and is counted.
     */
    fun onReceiveTimeout() {
        if (streamStarted) receiveTimeouts++
    }

    /** True once a report is due at [nowMs], so the caller reads the underrun count only then. */
    fun isDue(nowMs: Long): Boolean =
        (streamStarted || gracePassed(nowMs)) &&
            nowMs - intervalStartedAtMs >= LINK_STATS_INTERVAL_MS

    private fun gracePassed(nowMs: Long): Boolean =
        nowMs - startedAtMs >= LINK_STATS_STARTUP_GRACE_MS

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
