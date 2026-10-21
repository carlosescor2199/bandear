package com.bandear.android.ui.screens

import com.bandear.android.ui.components.LinkHealth
import com.bandear.android.ui.components.PillKind
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.DiagnosticsUiState
import com.bandear.android.ui.state.ServerEndpoint
import org.junit.Assert.assertEquals
import org.junit.Test

class SettingsTextTest {
    private val busy = DiagnosticsUiState(
        packetsReceived = 120_000,
        packetsLost = 24,
        packetsPerSecond = 99.6,
        interArrivalMs = 10.04,
        jitterMs = 1.43,
        playbackBufferMs = 34.4,
        sampleRateHz = 48_000,
        discardedSamples = 96,
        packetsWithDiscardedSamples = 2,
        underruns = 1,
        linkHealth = LinkHealth.WARNING,
    )

    @Test
    fun theCompactDiagnosticsShowSixReadings() {
        assertEquals(
            listOf(
                Reading("Packets / s", "100"),
                Reading("Loss", "0.02 %"),
                Reading("Underruns", "1"),
                Reading("Jitter", "1.4 ms"),
                Reading("Playback buffer", "34 ms"),
                Reading("Sample rate", "48 kHz"),
            ),
            diagnosticsSummary(busy),
        )
    }

    @Test
    fun theReadingsOfAFreshSessionAreZeroAndTheRateIsNotKnownYet() {
        assertEquals(
            listOf(
                Reading("Packets / s", "0"),
                Reading("Loss", "0.00 %"),
                Reading("Underruns", "0"),
                Reading("Jitter", "0.0 ms"),
                Reading("Playback buffer", "0 ms"),
                Reading("Sample rate", "—"),
            ),
            diagnosticsSummary(DiagnosticsUiState()),
        )
    }

    @Test
    fun aSampleRateThatIsNotWholeKilohertzKeepsOneDecimal() {
        assertEquals("44.1 kHz", sampleRateText(44_100))
        assertEquals("96 kHz", sampleRateText(96_000))
        assertEquals("—", sampleRateText(null))
    }

    @Test
    fun theExpanderListsEveryCounterAndTheControlChannel() {
        assertEquals(
            listOf(
                Reading("Control channel", "connected - mix applied on the server"),
                Reading("Link health", "Warning"),
                Reading("Packets received", "120000"),
                Reading("Packets lost", "24"),
                Reading("Loss", "0.02 %"),
                Reading("Packets / s", "99.6"),
                Reading("Inter-arrival", "10.0 ms"),
                Reading("Jitter", "1.4 ms"),
                Reading("Playback buffer", "34 ms"),
                Reading("Sample rate", "48 kHz"),
                Reading("Underruns", "1"),
                Reading("Discarded samples", "96"),
                Reading("Packets with discarded samples", "2"),
            ),
            allReadings(busy, controlStatus = "connected - mix applied on the server"),
        )
    }

    @Test
    fun recentErrorsNameWhereEachProblemCameFrom() {
        val errors = DiagnosticsUiState(lastError = "Socket closed", controlError = "Handshake refused")
        assertEquals(listOf("Last problem: Socket closed", "Control channel: Handshake refused"), recentErrors(errors))
        assertEquals(emptyList<String>(), recentErrors(DiagnosticsUiState()))
    }

    @Test
    fun theLinkSummaryNamesTheLevelInWords() {
        assertEquals("Link good", linkSummaryText(LinkHealth.GOOD))
        assertEquals("Link unknown", linkSummaryText(LinkHealth.UNKNOWN))
    }

    @Test
    fun theServerCardSaysWhereThePhoneIsConnected() {
        val connected = ConnectionUiState(
            pill = PillKind.CONNECTED,
            sessionRunning = true,
            server = ServerEndpoint("192.168.1.34", 50001),
        )
        assertEquals("BandEar server", serverCardTitle(connected))
        assertEquals("192.168.1.34:50001 · connected", serverCardDetail(connected))
    }

    @Test
    fun theServerCardWithoutASessionPointsToTheForm() {
        val idle = ConnectionUiState()
        assertEquals("Not connected", serverCardTitle(idle))
        assertEquals("Pick a server, or enter its address below.", serverCardDetail(idle))
    }
}
