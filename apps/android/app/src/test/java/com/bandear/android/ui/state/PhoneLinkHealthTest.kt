package com.bandear.android.ui.state

import com.bandear.android.LinkStatsReport
import com.bandear.android.ui.components.LinkHealth
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The phone judges its own link report with the server's thresholds (`apps/server/src/health.rs`):
 * nothing received is silent, a timeout, 2 underruns a second or 2 % loss is bad, and any underrun
 * or 0.5 % loss is a warning. Boundaries land exactly, as the server compares in integers.
 */
class PhoneLinkHealthTest {
    private fun report(
        intervalMs: Long = 1_000,
        received: Long = 100,
        lost: Long = 0,
        underruns: Long = 0,
        timeouts: Long = 0,
    ) = LinkStatsReport(intervalMs, received, lost, underruns, timeouts)

    @Test
    fun aCleanIntervalIsGood() {
        assertEquals(LinkHealth.GOOD, phoneLinkHealth(report()))
    }

    @Test
    fun nothingReceivedIsSilentWhateverElseHappened() {
        assertEquals(LinkHealth.SILENT, phoneLinkHealth(report(received = 0)))
        assertEquals(LinkHealth.SILENT, phoneLinkHealth(report(received = 0, lost = 5, timeouts = 3)))
    }

    @Test
    fun anyReceiveTimeoutIsBad() {
        assertEquals(LinkHealth.BAD, phoneLinkHealth(report(timeouts = 1)))
    }

    @Test
    fun oneUnderrunIsAWarningAndTwoASecondAreBad() {
        assertEquals(LinkHealth.WARNING, phoneLinkHealth(report(underruns = 1)))
        assertEquals(LinkHealth.BAD, phoneLinkHealth(report(underruns = 2)))
    }

    @Test
    fun underrunsAreJudgedPerSecondOfTheInterval() {
        assertEquals(LinkHealth.WARNING, phoneLinkHealth(report(intervalMs = 2_000, underruns = 3)))
        assertEquals(LinkHealth.BAD, phoneLinkHealth(report(intervalMs = 2_000, underruns = 4)))
    }

    @Test
    fun lossThresholdsLandExactly() {
        // 1 lost of 200 expected is exactly 0.5 %; 1 of 201 is just under.
        assertEquals(LinkHealth.WARNING, phoneLinkHealth(report(received = 199, lost = 1)))
        assertEquals(LinkHealth.GOOD, phoneLinkHealth(report(received = 200, lost = 1)))
        // 2 lost of 100 expected is exactly 2 %; 2 of 101 is just under.
        assertEquals(LinkHealth.BAD, phoneLinkHealth(report(received = 98, lost = 2)))
        assertEquals(LinkHealth.WARNING, phoneLinkHealth(report(received = 99, lost = 2)))
    }
}
