package com.mixlink.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Pure JVM coverage for the link-stats message the client sends over the control channel. The
 * server parses these exact keys as integers, so the test checks the parsed object rather than the
 * raw string: the JVM's org.json does not keep key order, while Android's does.
 */
class ControlChannelTest {
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
}
