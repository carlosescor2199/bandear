package com.mixlink.android

import com.mixlink.android.ui.state.FakeControlScheduler
import com.mixlink.android.ui.state.FakeSocket
import com.mixlink.android.ui.state.FakeTransport
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
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

    /** The number of the phone's newest mix change, as the ViewModel would hand it to the channel. */
    private var newestMix: Long? = 12
    private val channel = ControlChannel(
        listener = events,
        running = AtomicBoolean(true),
        currentSnapshot = { MixSnapshot() },
        moreOfMeEnabled = { false },
        musicianChannels = { emptyList() },
        mixSequence = { newestMix },
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

    /** A mix report as the server writes it: its full 32-channel and 16-group tables, and [seq] if any. */
    private fun mixReport(type: String, firstLevel: Int = 10, seq: Long? = null): String = JSONObject()
        .put("type", type)
        .put("seq", seq)
        .put("volume_percent", 80)
        .put("max_level_percent", 90)
        .put("muted", true)
        .put("channels", JSONArray(List(32) { firstLevel + it }))
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
    fun unknownTypesAreIgnored() {
        val socket = openChannel()

        socket.receive("""{"type":"mix_preview","channels":[1,2]}""")

        assertTrue(events.pushed.isEmpty())
        assertTrue(events.errors.isEmpty())
    }

    @Test
    fun aPushMissingPartOfTheMixIsReportedAndNotHandedOn() {
        val socket = openChannel()

        socket.receive("""{"type":"mix_state","volume_percent":80,"channels":[100,100]}""")

        assertTrue(events.pushed.isEmpty())
        assertTrue(events.errors.single(), events.errors.single().startsWith("Invalid control response"))
    }

    // The channel details a `config` carries.

    @Test
    fun aConfigHandsOnTheChannelDetailsInChannelOrder() {
        val socket = openChannel()

        socket.receive(
            """{"type":"config","source_channels":4,"groups":[],"channels":[
                {"name":"Vox","kind":"voice","hidden":false,"link":false},
                {"name":"Keys","kind":"keys","hidden":false,"link":true},
                {"name":"","kind":"keys","hidden":false,"link":false},
                {"name":"Spare","kind":"other","hidden":true,"link":false}]}""",
        )

        assertEquals(
            listOf(
                ChannelInfo("Vox", ChannelKind.VOICE),
                ChannelInfo("Keys", ChannelKind.KEYS, link = true),
                ChannelInfo("", ChannelKind.KEYS),
                ChannelInfo("Spare", ChannelKind.OTHER, hidden = true),
            ),
            events.details.last(),
        )
    }

    @Test
    fun aConfigFromAnOlderServerHandsOnNoDetails() {
        openChannel()

        // The opening config of [openChannel] has no `channels`, as an older server's.
        assertEquals(listOf<List<ChannelInfo>?>(null), events.details)
    }

    @Test
    fun channelDetailsAreReadLeniently() {
        val details = parseChannelDetails(
            JSONArray(
                """[{"name":"Gtr","kind":"theremin","hidden":true,"link":true},
                    "not an entry",
                    {"name":7,"kind":null},
                    {"name":"Extra","kind":"bass"}]""",
            ),
            count = 3,
        )

        // An unknown kind reads as other; an entry that is not an object, or a field of the wrong
        // type, reads as the default; an entry past the announced count is dropped.
        assertEquals(
            listOf(ChannelInfo("Gtr", ChannelKind.OTHER, hidden = true, link = true), ChannelInfo(), ChannelInfo()),
            details,
        )
        // Fewer entries than channels: the rest are channels the engineer never touched.
        assertEquals(listOf(ChannelInfo("Bass", ChannelKind.BASS), ChannelInfo()), parseChannelDetails(JSONArray("""[{"name":"Bass","kind":"bass"}]"""), 2))
        // Anything but an array is no details at all.
        assertEquals(null, parseChannelDetails(null, 2))
        assertEquals(null, parseChannelDetails("voice", 2))
    }

    @Test
    fun everyKindIsReadByItsWireName() {
        for (kind in ChannelKind.entries) {
            assertEquals(kind, ChannelKind.fromWire(kind.wire))
        }
        assertEquals(
            listOf("voice", "guitar", "bass", "keys", "drums", "percussion", "horns", "strings", "playback", "click", "other"),
            ChannelKind.entries.map { it.wire },
        )
    }

    // The engineer's limit and locks a mix report carries.

    /** [report] with the engineer's fields set to [limit] and [locked], as a server since T5a writes them. */
    private fun withEngineer(report: String, limit: Any?, locked: Any?): String =
        JSONObject(report).put("engineer_limit_percent", limit).put("locked", locked).toString()

    @Test
    fun aPushAndAnAckCarryTheEngineersLimitAndLocks() {
        val socket = openChannel()

        socket.receive(withEngineer(mixReport("mix_state"), 60, JSONArray(List(32) { it == 0 || it == 3 })))
        socket.receive(withEngineer(mixReport("mix_ack", seq = 12), 75, JSONArray(List(32) { it == 1 })))

        assertEquals(listOf(EngineerControls(60, setOf(0, 3)), EngineerControls(75, setOf(1))), events.engineer)
        assertEquals(10, events.pushed.single().channelGains[0])
    }

    @Test
    fun aReportFromAnOlderServerHasNoLimitAndNothingLocked() {
        val socket = openChannel()

        socket.receive(mixReport("mix_state"))

        assertEquals(listOf(EngineerControls()), events.engineer)
        assertFalse(EngineerControls().limited)
    }

    @Test
    fun theEngineersControlsAreReadLeniently() {
        fun read(limit: Any?, locked: Any?) = parseEngineerControls(JSONObject(withEngineer(mixReport("mix_state"), limit, locked)))

        // A short list leaves the rest open; an entry that is not a boolean is open.
        assertEquals(EngineerControls(100, setOf(0, 2)), read(null, JSONArray("""[true, false, true]""")))
        assertEquals(EngineerControls(100, setOf(2)), read(null, JSONArray("""[1, null, true, "true"]""")))
        // A limit that is not a number is no limit; one out of range is held to 0-100.
        assertEquals(EngineerControls(), read("high", "all"))
        assertEquals(EngineerControls(100), read(140, null))
        assertEquals(EngineerControls(0), read(-5, null))
        assertTrue(EngineerControls(60).limited)
    }

    @Test
    fun aMixNeverCarriesTheEngineersControls() {
        val socket = openChannel()
        socket.receive(withEngineer(mixReport("mix_state"), 60, JSONArray(List(32) { it == 0 })))

        channel.sendMixControl()

        val mix = socket.sentOfType("mix").last()
        assertFalse(mix.has("engineer_limit_percent"))
        assertFalse(mix.has("locked"))
    }

    // The numbers that match the server's answers to the phone's mixes.

    @Test
    fun aMixCarriesTheNumberOfTheNewestChangeAndNoneWithoutOne() {
        val socket = openChannel()

        newestMix = null
        channel.sendMixControl()

        val (opening, unnumbered) = socket.sentOfType("mix")
        assertEquals(12L, opening.getLong("seq"))
        assertFalse(unnumbered.has("seq"))
    }

    @Test
    fun aNumberedAckAnswersItsMixWithTheMixTheServerPlays() {
        val socket = openChannel()

        socket.receive(mixReport("mix_ack", firstLevel = 40, seq = 12))

        val (seq, mix) = events.answers.single()
        assertEquals(12L, seq)
        assertEquals(40, mix?.channelGains?.get(0))
        assertEquals(1, events.acknowledged)
        assertTrue(channel.mixAcknowledged)
    }

    @Test
    fun anAckWithoutANumberOnlyAcknowledges() {
        val socket = openChannel()

        socket.receive(mixReport("mix_ack", firstLevel = 40))

        assertTrue(events.answers.isEmpty())
        assertEquals(1, events.acknowledged)
    }

    @Test
    fun aNumberedErrorAnswersItsMixWithNothingToShow() {
        val socket = openChannel()

        socket.receive("""{"type":"error","message":"too many channels","seq":12}""")

        assertEquals(listOf(12L to null), events.answers)
        assertEquals(listOf("too many channels"), events.errors)
    }

    @Test
    fun anErrorWithoutANumberNeverTouchesTheMixes() {
        val socket = openChannel()

        socket.receive("""{"type":"error","message":"no UDP target configured for client IP"}""")

        assertTrue(events.answers.isEmpty())
        assertEquals(listOf("no UDP target configured for client IP"), events.errors)
    }

    @Test
    fun aNumberedAckWhoseMixCannotBeReadStillAnswersItsNumber() {
        val socket = openChannel()

        socket.receive("""{"type":"mix_ack","seq":12,"channels":[100]}""")

        assertEquals(listOf(12L to null), events.answers)
        assertEquals(1, events.acknowledged)
        assertTrue(events.errors.isEmpty())
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
        val answers = mutableListOf<Pair<Long, MixSnapshot?>>()

        /** The engineer's controls of every readable report, pushed or answering, in order. */
        val engineer = mutableListOf<EngineerControls>()
        val errors = mutableListOf<String>()
        val details = mutableListOf<List<ChannelInfo>?>()
        var acknowledged = 0

        override fun onOpened() = Unit

        override fun onConfig(channels: Int, groups: List<GroupInfo>, details: List<ChannelInfo>?) {
            this.details += details
        }

        override fun onAcknowledged() {
            acknowledged++
        }

        override fun onMixPushed(report: MixReport) {
            pushed += report.mix
            engineer += report.engineer
        }

        override fun onMixAnswered(seq: Long, applied: MixReport?) {
            answers += seq to applied?.mix
            applied?.let { engineer += it.engineer }
        }

        override fun onClosed() = Unit

        override fun onControlError(message: String) {
            errors += message
        }
    }
}
