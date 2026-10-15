package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.linkHealthWords
import com.mixlink.android.ui.components.pillWords
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.DiagnosticsUiState
import java.util.Locale

/** One labelled number in the diagnostics. */
data class Reading(val label: String, val value: String)

/** Shown for a value the phone does not know yet. */
private const val UNKNOWN_VALUE = "—"

/**
 * The six readings of the compact diagnostics: the ones that tell whether the audio is healthy.
 * Numbers use a fixed format, whatever the phone's language, so they read the same as the desktop.
 */
fun diagnosticsSummary(diagnostics: DiagnosticsUiState): List<Reading> = listOf(
    Reading("Packets / s", decimal(diagnostics.packetsPerSecond, places = 0)),
    Reading("Loss", lossText(diagnostics)),
    Reading("Underruns", diagnostics.underruns.toString()),
    Reading("Jitter", milliseconds(diagnostics.jitterMs, places = 1)),
    Reading("Playback buffer", milliseconds(diagnostics.playbackBufferMs, places = 0)),
    Reading("Sample rate", sampleRateText(diagnostics.sampleRateHz)),
)

/** Every counter, for the expander under the compact diagnostics, led by the control channel. */
fun allReadings(diagnostics: DiagnosticsUiState, controlStatus: String): List<Reading> = listOf(
    Reading("Control channel", controlStatus),
    Reading("Link health", linkHealthWords(diagnostics.linkHealth)),
    Reading("Packets received", diagnostics.packetsReceived.toString()),
    Reading("Packets lost", diagnostics.packetsLost.toString()),
    Reading("Loss", lossText(diagnostics)),
    Reading("Packets / s", decimal(diagnostics.packetsPerSecond, places = 1)),
    Reading("Inter-arrival", milliseconds(diagnostics.interArrivalMs, places = 1)),
    Reading("Jitter", milliseconds(diagnostics.jitterMs, places = 1)),
    Reading("Playback buffer", milliseconds(diagnostics.playbackBufferMs, places = 0)),
    Reading("Sample rate", sampleRateText(diagnostics.sampleRateHz)),
    Reading("Underruns", diagnostics.underruns.toString()),
    Reading("Discarded samples", diagnostics.discardedSamples.toString()),
    Reading("Packets with discarded samples", diagnostics.packetsWithDiscardedSamples.toString()),
)

/** The last problems of the session, each named by where it came from; empty when there were none. */
fun recentErrors(diagnostics: DiagnosticsUiState): List<String> = listOfNotNull(
    diagnostics.lastError?.let { "Last problem: $it" },
    diagnostics.controlError?.let { "Control channel: $it" },
)

/** A sample rate in kilohertz: whole rates without decimals, others with one. */
fun sampleRateText(hz: Int?): String = when {
    hz == null -> UNKNOWN_VALUE
    hz % 1000 == 0 -> "${hz / 1000} kHz"
    else -> "${decimal(hz / 1000.0, places = 1)} kHz"
}

/** The link-health light's words in the diagnostics header, such as "Link good". */
fun linkSummaryText(health: LinkHealth): String = "Link ${linkHealthWords(health).lowercase(Locale.ROOT)}"

/** The server card's title: the server while a session runs, otherwise that there is none. */
fun serverCardTitle(connection: ConnectionUiState): String =
    if (connection.sessionRunning) "MixLink server" else "Not connected"

/** The line under the server card's title: where, and the pill's state, or what to do instead. */
fun serverCardDetail(connection: ConnectionUiState): String {
    val server = connection.server
    if (!connection.sessionRunning || server == null) return "Pick a server, or enter its address below."
    return "${server.label} · ${pillWords(connection.pill).lowercase(Locale.ROOT)}"
}

private fun lossText(diagnostics: DiagnosticsUiState): String = "${decimal(diagnostics.lossPercent, places = 2)} %"

private fun milliseconds(value: Double, places: Int): String = "${decimal(value, places)} ms"

private fun decimal(value: Double, places: Int): String = String.format(Locale.ROOT, "%.${places}f", value)
