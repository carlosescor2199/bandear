package com.bandear.android.ui.state

import com.bandear.android.LinkStatsReport
import com.bandear.android.ui.components.LinkHealth

/*
 * The server's link thresholds (`apps/server/src/health.rs`), so the phone shows its own link with
 * the same light the engineer sees for it on the desktop.
 */

/** Underruns per second at or above which the link is bad. */
private const val BAD_UNDERRUNS_PER_SECOND = 2L

/** Loss, in basis points (hundredths of a percent), at or above which the link is bad: 2 %. */
private const val BAD_LOSS_BASIS_POINTS = 200L

/** Loss, in basis points, at or above which the link needs attention: 0.5 %. */
private const val WARNING_LOSS_BASIS_POINTS = 50L

private const val BASIS_POINTS_PER_WHOLE = 10_000L
private const val MILLISECONDS_PER_SECOND = 1_000L

/**
 * Judges the phone's own link from one [report], the one it also sends to the server.
 *
 * Nothing received over the interval is [LinkHealth.SILENT], whatever else the report says, which is
 * what the desktop calls a bad link that hears no audio. Otherwise a receive timeout, two underruns
 * a second or 2 % loss is bad, and any underrun or 0.5 % loss is a warning. The comparisons are made
 * in integers, like the server's, so a count exactly on a threshold is judged exactly. The phone
 * never judges a link [LinkHealth.LOST]: it is the one making the reports.
 */
internal fun phoneLinkHealth(report: LinkStatsReport): LinkHealth {
    if (report.packetsReceived == 0L) return LinkHealth.SILENT
    val intervalMs = report.intervalMs.coerceAtLeast(1L)
    val expected = report.packetsReceived + report.packetsLost
    fun lossReaches(basisPoints: Long): Boolean =
        report.packetsLost * BASIS_POINTS_PER_WHOLE >= basisPoints * expected
    val underrunsBad = report.underruns * MILLISECONDS_PER_SECOND >= BAD_UNDERRUNS_PER_SECOND * intervalMs
    if (report.receiveTimeouts > 0 || underrunsBad || lossReaches(BAD_LOSS_BASIS_POINTS)) return LinkHealth.BAD
    if (report.underruns > 0 || lossReaches(WARNING_LOSS_BASIS_POINTS)) return LinkHealth.WARNING
    return LinkHealth.GOOD
}
