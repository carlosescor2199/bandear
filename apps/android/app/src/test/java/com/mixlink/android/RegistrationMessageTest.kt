package com.mixlink.android

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Test
import java.io.IOException
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Pure JVM coverage for the registration message the client sends when the control channel opens.
 * The server takes the address from the socket, so the message must not carry one, and a server
 * that does not understand the message is allowed to answer with an error or ignore it.
 */
class RegistrationMessageTest {
    @Test
    fun carriesTheTypeTheNameAndTheUdpPort() {
        val message = JSONObject(buildRegisterMessage("Ana", 50000))

        assertEquals("register", message.getString("type"))
        assertEquals("Ana", message.getString("name"))
        assertEquals(50000, message.getInt("udp_port"))
    }

    @Test
    fun trimsTheNameButKeepsAnEmptyOne() {
        assertEquals("Ana", JSONObject(buildRegisterMessage("  Ana  ", 50001)).getString("name"))
        assertEquals("", JSONObject(buildRegisterMessage("   ", 50001)).getString("name"))
    }

    @Test
    fun carriesNoAddressBecauseTheServerUsesTheSocket() {
        val message = JSONObject(buildRegisterMessage("Ana", 50000))

        assertFalse(message.has("address"))
        assertFalse(message.has("ip"))
    }

    @Test
    fun survivesANameWithQuotesAndAccents() {
        val name = "Ana \"La Jefa\" Güemes"

        assertEquals(name, JSONObject(buildRegisterMessage(name, 50000)).getString("name"))
    }

    /** Compared field by field: the JVM org.json does not keep key order. */
    @Test
    fun carriesTheClientIdSoTheServerCanFollowThePhoneToANewAddress() {
        val message = JSONObject(buildRegisterMessage("Ana", 50000, CLIENT_ID))

        assertEquals(setOf("type", "name", "udp_port", "client_id"), message.keySet())
        assertEquals("register", message.getString("type"))
        assertEquals("Ana", message.getString("name"))
        assertEquals(50000, message.getInt("udp_port"))
        assertEquals(CLIENT_ID, message.getString("client_id"))
    }

    @Test
    fun leavesTheClientIdOutWhenThereIsNone() {
        assertEquals(setOf("type", "name", "udp_port"), JSONObject(buildRegisterMessage("Ana", 50000)).keySet())
        assertEquals(setOf("type", "name", "udp_port"), JSONObject(buildRegisterMessage("Ana", 50000, null)).keySet())
    }

    /**
     * The id given to [ControlChannel.connect] travels in every registration of the session: the
     * first open, an open after a lost connection, and a rename on the open channel.
     */
    @Test
    fun theChannelAnnouncesTheSameClientIdOnEveryOpenAndOnARename() {
        val transport = RecordingTransport()
        val scheduler = StepScheduler()
        val channel = ControlChannel(
            listener = SilentListener(),
            running = AtomicBoolean(true),
            currentSnapshot = { MixSnapshot() },
            moreOfMeEnabled = { false },
            musicianChannels = { emptyList() },
            transport = transport,
            scheduler = scheduler,
        )

        channel.connect("192.168.1.20", 9001, 50000, "Ana", CLIENT_ID)
        val first = transport.sockets.single()
        first.events.onOpen(first)
        first.events.onFailure(first, IOException("wifi gone"))
        scheduler.runDelayed()
        val second = transport.sockets[1]
        second.events.onOpen(second)
        channel.updateRegistrationName("Bea")
        scheduler.runDelayed()

        val registrations = transport.sockets.flatMap { socket -> socket.sent.map(::JSONObject) }
            .filter { it.getString("type") == "register" }
        assertEquals(listOf("Ana", "Ana", "Bea"), registrations.map { it.getString("name") })
        assertEquals(listOf(CLIENT_ID, CLIENT_ID, CLIENT_ID), registrations.map { it.optString("client_id") })
    }

    private class RecordingSocket(val events: ControlSocketEvents) : ControlSocket {
        val sent = mutableListOf<String>()

        override fun send(text: String): Boolean {
            sent += text
            return true
        }

        override fun close(code: Int, reason: String?): Boolean = true

        override fun cancel() = Unit
    }

    private class RecordingTransport : ControlTransport {
        val sockets = mutableListOf<RecordingSocket>()

        override fun open(url: String, events: ControlSocketEvents): ControlSocket =
            RecordingSocket(events).also { sockets += it }

        override fun shutdown() = Unit
    }

    /** Runs immediate work at once and delayed work only when the test asks for it. */
    private class StepScheduler : ControlScheduler {
        private val delayed = mutableListOf<() -> Unit>()

        override fun execute(task: () -> Unit) = task()

        override fun schedule(delayMs: Long, task: () -> Unit): ScheduledTask {
            delayed += task
            return ScheduledTask { delayed.remove(task) }
        }

        override fun shutdown() = Unit

        fun runDelayed() {
            val due = delayed.toList()
            delayed.clear()
            due.forEach { it() }
        }
    }

    private class SilentListener : ControlChannel.Listener {
        override fun onOpened() = Unit
        override fun onConfig(channels: Int, groups: List<GroupInfo>, details: List<ChannelInfo>?) = Unit
        override fun onAcknowledged() = Unit
        override fun onClosed() = Unit
        override fun onControlError(message: String) = Unit
    }

    private companion object {
        const val CLIENT_ID = "0b6f1c9e-2d4a-4f7b-9c3e-5a8d7e6f1a2b"
    }
}
