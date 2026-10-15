package com.mixlink.android

import com.mixlink.android.ui.state.FakeControlScheduler
import com.mixlink.android.ui.state.FakeSocket
import com.mixlink.android.ui.state.FakeTransport
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Pure JVM coverage for the link-stats message the client sends over the control channel, and for
 * the mix the server pushes to it. The server parses the stats keys as integers, so the test checks
 * the parsed object rather than the raw string: the JVM's org.json does not keep key order, while
 * Android's does.
 */
class ControlChannelTest {
    private val events = RecordingListener()
    private val transport = FakeTransport()
    private val channel = ControlChannel(
        listener = events,
        running = AtomicBoolean(true),
        currentSnapshot = { MixSnapshot() },
        moreOfMeEnabled = { false },
        musicianChannel = { null },
        transport = transport,
        scheduler = FakeControlScheduler(),
    )

    /** Opens the channel and lets the server announce itself, as it does right after connect. */
    private fun openChannel(): FakeSocket {
        channel.connect("192.168.1.20", 50001, 50000, "Ana")
        return transport.sockets.single().also { socket ->
            socket.open()
            socket.receive("""{"type":"config","source_channels":2,"groups":[]}""")
        }
    }

    /** A mix report as the server writes it: its full 32-channel and 16-group tables. */
    private fun mixReport(type: String): String = JSONObject()
        .put("type", type)
        .put("volume_percent", 80)
        .put("max_level_percent", 90)
        .put("muted", true)
        .put("channels", JSONArray(List(32) { 10 + it }))
        .put("pans", JSONArray(List(32) { if (it == 1) 25 else defaultPan(it) }))
        .put("mutes", JSONArray(List(32) { it == 2 }))
        .put("solos", JSONArray(List(32) { it == 3 }))
        .put("group_levels", JSONArray(List(16) { 50 + it }))
        .toString()

    @Test
    fun aPushedMixIsReadWholeAndHandedOnWithNothingSentBack() {
        val socket = openChannel()
        val sent = socket.sent.size

        socket.receive(mixReport("mix_state"))

        val mix = events.pushed.single()
        assertArrayEquals(IntArray(32) { 10 + it }, mix.channelGains)
        assertArrayEquals(IntArray(32) { if (it == 1) 25 else defaultPan(it) }, mix.channelPans)
        assertArrayEquals(BooleanArray(32) { it == 2 }, mix.channelMutes)
        assertArrayEquals(BooleanArray(32) { it == 3 }, mix.channelSolos)
        assertArrayEquals(IntArray(16) { 50 + it }, mix.groupLevels)
        assertEquals(80, mix.volumePercent)
        assertEquals(90, mix.maxLevelPercent)
        assertTrue(mix.muted)
        assertEquals(sent, socket.sent.size)
        assertTrue(events.errors.isEmpty())
    }

    @Test
    fun unknownTypesAreIgnoredAndAnAcknowledgementIsNotAPush() {
        val socket = openChannel()

        socket.receive("""{"type":"mix_preview","channels":[1,2]}""")
        socket.receive(mixReport("mix_ack"))

        assertTrue(events.pushed.isEmpty())
        assertTrue(events.errors.isEmpty())
        assertEquals(1, events.acknowledged)
    }

    @Test
    fun aPushMissingPartOfTheMixIsReportedAndNotHandedOn() {
        val socket = openChannel()

        socket.receive("""{"type":"mix_state","volume_percent":80,"channels":[100,100]}""")

        assertTrue(events.pushed.isEmpty())
        assertTrue(events.errors.single(), events.errors.single().startsWith("Invalid control response"))
    }

    private val wireKeys = setOf(
        "type",
        "interval_ms",
        "packets_received",
        "packets_lost",
        "underruns",
        "receive_timeouts",
    )

    @Test
    fun statsMessageIsExactlyTheWireShape() {
        val message = JSONObject(
            buildStatsMessage(
                LinkStatsReport(
                    intervalMs = 1_000,
                    packetsReceived = 200,
                    packetsLost = 0,
                    underruns = 0,
                    receiveTimeouts = 0,
                ),
            ),
        )
        val expected = JSONObject(
            """{"type":"stats","interval_ms":1000,"packets_received":200,"packets_lost":0,"underruns":0,"receive_timeouts":0}""",
        )

        assertEquals(wireKeys, message.keySet())
        assertTrue("$message", message.similar(expected))
    }

    @Test
    fun statsMessageCarriesEachCountInItsOwnFieldAsAnInteger() {
        val message = JSONObject(
            buildStatsMessage(
                LinkStatsReport(
                    intervalMs = 1_250,
                    packetsReceived = 98,
                    packetsLost = 2,
                    underruns = 1,
                    receiveTimeouts = 3,
                ),
            ),
        )

        assertEquals("stats", message.getString("type"))
        val expected = mapOf(
            "interval_ms" to 1_250L,
            "packets_received" to 98L,
            "packets_lost" to 2L,
            "underruns" to 1L,
            "receive_timeouts" to 3L,
        )
        for ((key, value) in expected) {
            val raw = message.get(key)
            assertTrue("$key must be an integer, got $raw", raw is Int || raw is Long)
            assertEquals(key, value, (raw as Number).toLong())
        }
    }

    private class RecordingListener : ControlChannel.Listener {
        val pushed = mutableListOf<MixSnapshot>()
        val errors = mutableListOf<String>()
        var acknowledged = 0

        override fun onOpened() = Unit

        override fun onConfig(channels: Int, groups: List<GroupInfo>) = Unit

        override fun onAcknowledged() {
            acknowledged++
        }

        override fun onMixPushed(mix: MixSnapshot) {
            pushed += mix
        }

        override fun onClosed() = Unit

        override fun onControlError(message: String) {
            errors += message
        }
    }
}
