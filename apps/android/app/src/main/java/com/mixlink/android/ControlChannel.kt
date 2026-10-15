package com.mixlink.android

import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import org.json.JSONArray
import org.json.JSONObject
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.ScheduledFuture
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/**
 * How long the channel waits after the last keystroke before re-announcing a new name.
 *
 * Normal typing pauses between keystrokes are around 100-250 ms, well under this, so a name typed
 * at speed becomes one registration message per pause instead of one per key. It stays short
 * enough that the desktop, which polls the engine every 250 ms, shows the new name within about
 * half a second of the musician finishing the word.
 */
private const val NAME_RESEND_DEBOUNCE_MS = 400L

/**
 * How often the client pings the server on an open control connection. A pong that has not come
 * back by the next ping fails the socket, so a half-open link (Wi-Fi gone without any close) is
 * noticed within a few seconds and reconnection starts, instead of waiting for a write to fail.
 */
private const val PING_INTERVAL_SECONDS = 2L

/** The WebSocket close code for a deliberate, normal close. */
private const val NORMAL_CLOSURE = 1000

/**
 * The OkHttp WebSocket control channel, its connection state machine, sending and closing.
 *
 * The channel owns the executor, the OkHttp client and the WebSocket listeners. It reports opened,
 * acknowledged, config, error and closed events through [Listener], so the Activity only renders
 * and wires. [running] is the shared session flag owned by the caller.
 *
 * While the session runs, a connection that fails or closes is reopened on its own, following
 * [ReconnectPolicy], to the server and control port given to [connect]. Every open re-sends the
 * registration and then the current mix, so the server gets this musician back as they were. The
 * audio receiver is never touched. [close] stops the session and any pending attempt.
 */
internal class ControlChannel(
    private val listener: Listener,
    private val running: AtomicBoolean,
    private val currentSnapshot: () -> MixSnapshot,
    private val moreOfMeEnabled: () -> Boolean,
    private val musicianChannel: () -> Int?,
) {
    interface Listener {
        fun onOpened()
        fun onConfig(channels: Int, groups: List<GroupInfo>)
        fun onAcknowledged()
        fun onClosed()
        fun onControlError(message: String)
    }

    private val controlExecutor = Executors.newSingleThreadScheduledExecutor()
    private val httpClient = OkHttpClient.Builder()
        .pingInterval(PING_INTERVAL_SECONDS, TimeUnit.SECONDS)
        .build()

    /** The open socket that sends go to; null while no connection is open. */
    @Volatile private var controlWebSocket: WebSocket? = null
    @Volatile private var registrationMessage: String? = null
    @Volatile private var announcedUdpPort: Int? = null
    private var pendingNameResend: ScheduledFuture<*>? = null
    @Volatile var controlState = ControlState.IDLE
    @Volatile var mixAcknowledged = false
        private set

    // Reconnection state. Compound decisions on it are made while holding [connectionLock].
    private val connectionLock = Any()
    private val generations = ConnectionGenerations()
    private val reconnectPolicy = ReconnectPolicy()
    private var serverHost: String? = null
    private var serverControlPort = 0

    /** The socket of the current attempt, from its creation until it ends; [close] closes it. */
    private var currentSocket: WebSocket? = null
    private var pendingReconnect: ScheduledFuture<*>? = null

    /** Set by [close]: the session was stopped, so nothing reconnects until the next [connect]. */
    private var stopped = true

    /**
     * Listens to one connection attempt. Its [generation] ties every callback to the socket it came
     * from: the callbacks of a socket that is no longer current never change the channel's state,
     * and such a socket is only ever closed, never used.
     */
    private inner class ConnectionListener(private val generation: Long) : WebSocketListener() {
        override fun onOpen(webSocket: WebSocket, response: Response) {
            if (!adoptOpenedSocket(generation, webSocket)) {
                // Stopped or replaced while it was still opening.
                webSocket.close(NORMAL_CLOSURE, "superseded")
                return
            }
            listener.onOpened()
            // Registration goes first: the server adds this client as a target from it, so the mix
            // that follows finds a target to join. A reconnect does exactly the same, so the server
            // gets this musician's name and current mix back.
            sendRegistration()
            sendCurrentMix()
        }

        override fun onMessage(webSocket: WebSocket, text: String) {
            if (!generations.isCurrent(generation)) return
            try {
                val message = JSONObject(text)
                when (message.optString("type")) {
                    "error" -> listener.onControlError(message.optString("message", "unknown control error"))
                    "config" -> {
                        val announcedChannels = message.optInt("source_channels", -1)
                        if (announcedChannels in 1..MAX_CHANNELS) {
                            listener.onConfig(announcedChannels, parseGroups(message.optJSONArray("groups")))
                        }
                    }
                    "mix_ack" -> {
                        mixAcknowledged = true
                        listener.onAcknowledged()
                    }
                }
            } catch (error: Exception) {
                listener.onControlError("Invalid control response: ${error.message ?: "unknown error"}")
            }
        }

        override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
            // The server is closing. Answer, so the close handshake completes, and treat the
            // connection as ended now: the onClosed that follows belongs to an ended generation.
            webSocket.close(NORMAL_CLOSURE, null)
            connectionEnded(generation, failure = null)
        }

        override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
            connectionEnded(generation, failure = "WebSocket error: ${t.message ?: "connection failed"}")
        }

        override fun onClosed(webSocket: WebSocket, code: Int, reason: String) {
            connectionEnded(generation, failure = null)
        }
    }

    /**
     * Opens the control channel and remembers what to announce when it opens: the musician's name
     * and the UDP port this client listens on. A server that does not understand the registration
     * answers with an error or ignores it, and the rest of the session behaves as before.
     *
     * The host and control port are kept for the whole session, so a lost connection can be
     * reopened to the same server without the musician pressing anything.
     */
    fun connect(host: String, port: Int, udpPort: Int, musicianName: String) {
        announcedUdpPort = udpPort
        registrationMessage = buildRegisterMessage(musicianName, udpPort)
        synchronized(connectionLock) {
            serverHost = host
            serverControlPort = port
            stopped = false
            reconnectPolicy.reset()
            cancelPendingReconnectLocked()
        }
        executeOnControl { attemptConnection() }
    }

    /**
     * Opens one connection to the stored server, on the control executor. Does nothing once the
     * session stopped or the receiver ended, so an attempt that was already due when Stop came
     * never connects.
     */
    private fun attemptConnection() {
        val host: String
        val port: Int
        val generation: Long
        synchronized(connectionLock) {
            pendingReconnect = null
            if (!ReconnectPolicy.shouldReconnect(running.get(), stopped)) return
            host = serverHost ?: return
            port = serverControlPort
            generation = generations.begin()
        }
        try {
            val request = Request.Builder()
                .url("ws://$host:$port")
                .build()
            val socket = httpClient.newWebSocket(request, ConnectionListener(generation))
            // The attempt can fail, or the session can stop, before newWebSocket returns: only a
            // socket that is still current is kept, and a stopped one is dropped before it opens.
            val kept = synchronized(connectionLock) {
                generations.isCurrent(generation).also { current -> if (current) currentSocket = socket }
            }
            if (!kept) socket.cancel()
        } catch (error: Exception) {
            if (running.get()) listener.onControlError("Could not start WebSocket: ${error.message ?: "unknown error"}")
        }
    }

    /**
     * Makes the socket of [generation] the open one, if that attempt is still current. A successful
     * open also starts the reconnect schedule again from its shortest wait.
     */
    private fun adoptOpenedSocket(generation: Long, webSocket: WebSocket): Boolean {
        synchronized(connectionLock) {
            if (!generations.isCurrent(generation)) return false
            currentSocket = webSocket
            controlWebSocket = webSocket
            controlState = ControlState.CONNECTED
            reconnectPolicy.reset()
        }
        return true
    }

    /**
     * Handles the end of connection [generation], whichever way it ended. Only the first end of the
     * current connection counts; a stale or repeated one is ignored. While the session runs and was
     * not stopped, the channel shows [ControlState.RECONNECTING] and schedules the next attempt.
     * Ownership of the mix is left exactly as it was.
     */
    private fun connectionEnded(generation: Long, failure: String?) {
        val sessionRunning: Boolean
        synchronized(connectionLock) {
            if (!generations.end(generation)) return
            currentSocket = null
            controlWebSocket = null
            sessionRunning = running.get()
            val reconnect = ReconnectPolicy.shouldReconnect(sessionRunning, stopped)
            controlState = when {
                reconnect -> ControlState.RECONNECTING
                !sessionRunning -> ControlState.IDLE
                failure != null -> ControlState.UNAVAILABLE
                else -> ControlState.CLOSED
            }
            if (reconnect) scheduleReconnectLocked()
        }
        listener.onClosed()
        if (failure != null && sessionRunning) listener.onControlError(failure)
    }

    /** Schedules the next attempt after the policy's wait. Call only while holding [connectionLock]. */
    private fun scheduleReconnectLocked() {
        val delayMs = reconnectPolicy.nextDelayMs()
        pendingReconnect = try {
            controlExecutor.schedule({ attemptConnection() }, delayMs, TimeUnit.MILLISECONDS)
        } catch (_: RejectedExecutionException) {
            // The channel shut down; nothing reconnects after that.
            null
        }
    }

    /** Cancels a scheduled attempt, if any. Call only while holding [connectionLock]. */
    private fun cancelPendingReconnectLocked() {
        pendingReconnect?.cancel(false)
        pendingReconnect = null
    }

    /** Runs [task] on the control executor, or drops it once the channel has shut down. */
    private fun executeOnControl(task: () -> Unit) {
        try {
            controlExecutor.execute(task)
        } catch (_: RejectedExecutionException) {
            // Shut down: the Activity is gone and nothing is listening any more.
        }
    }

    /**
     * Re-announces the client under a new name on the open control channel. The name is a label,
     * not part of the audio: no reconnect happens and no receiver is restarted, and the server
     * updates the target it already has, keeping its mix state and counters.
     *
     * Sends are debounced by [NAME_RESEND_DEBOUNCE_MS] so typing produces one message per pause,
     * not one per keystroke. The stored registration is updated too, so a socket that has not
     * opened yet announces the latest name when it does.
     */
    fun updateRegistrationName(name: String) {
        if (!running.get()) return
        val udpPort = announcedUdpPort ?: return
        val message = buildRegisterMessage(name, udpPort)
        registrationMessage = message
        pendingNameResend?.cancel(false)
        pendingNameResend = controlExecutor.schedule(
            { sendRegistrationMessage(message) },
            NAME_RESEND_DEBOUNCE_MS,
            TimeUnit.MILLISECONDS,
        )
    }

    /**
     * Sends the registration once, on open, ahead of the first mix. Queued on the same executor as
     * the mix sends, so the server always sees the registration first.
     */
    private fun sendRegistration() {
        val message = registrationMessage ?: return
        executeOnControl {
            sendRegistrationMessage(message)
        }
    }

    private fun sendRegistrationMessage(message: String) {
        val webSocket = controlWebSocket ?: return
        if (!webSocket.send(message) && running.get()) {
            listener.onControlError("WebSocket rejected the registration")
        }
    }

    fun sendCurrentMix() {
        sendMixControl()
    }

    /**
     * Clears ownership so a fresh session starts with the client owning its mix. The client re-sends
     * its mix on connect and regains ownership on the next acknowledgement; until then the local
     * master volume stays live even if the control channel never comes up.
     */
    fun clearMixOwnership() {
        mixAcknowledged = false
    }

    fun sendMixControl() {
        executeOnControl {
            val webSocket = controlWebSocket ?: return@executeOnControl
            val stored = currentSnapshot()
            // `More of me` is derived here, at send time, and never written back. The stored values
            // stay exactly as the musician left them, so turning the control off re-sends them.
            val outgoing = if (moreOfMeEnabled()) stored.moreOfMe(musicianChannel()) ?: stored else stored
            if (!webSocket.send(buildMixMessage(outgoing)) && running.get()) {
                listener.onControlError("WebSocket rejected mix update")
            }
        }
    }

    /**
     * Sends one link-stats report on the control executor, behind any registration or mix already
     * queued. Called from the receive thread, so it only hands the report over and never waits.
     *
     * A report describes one interval and is worthless later, so it is dropped, never queued, when
     * the channel is not open or has shut down. A rejected send is not shown as an error either: the
     * next report follows within a second, and the server's view of a link that stops reporting is
     * exactly what the report is for.
     */
    fun sendLinkStats(report: LinkStatsReport) {
        if (controlWebSocket == null) return
        try {
            controlExecutor.execute {
                controlWebSocket?.send(buildStatsMessage(report))
            }
        } catch (_: RejectedExecutionException) {
            // The channel shut down while the receiver was still stopping.
        }
    }

    /**
     * Stops the session's control connection: cancels any scheduled reconnect, makes every socket's
     * callbacks stale and closes the current socket, open or still opening. Nothing reconnects until
     * the next [connect].
     */
    fun close(code: Int, reason: String) {
        val socket: WebSocket?
        synchronized(connectionLock) {
            stopped = true
            cancelPendingReconnectLocked()
            generations.invalidate()
            socket = currentSocket
            currentSocket = null
            controlWebSocket = null
        }
        socket?.close(code, reason)
    }

    fun shutdown() {
        synchronized(connectionLock) {
            stopped = true
            cancelPendingReconnectLocked()
        }
        controlExecutor.shutdownNow()
        httpClient.dispatcher.executorService.shutdown()
    }

    private fun buildMixMessage(snapshot: MixSnapshot): String {
        val channels = JSONArray()
        for (gain in snapshot.channelGains) {
            channels.put(gain)
        }
        val panLevels = JSONArray()
        for (pan in snapshot.channelPans) {
            panLevels.put(pan)
        }
        val muteFlags = JSONArray()
        for (mute in snapshot.channelMutes) {
            muteFlags.put(mute)
        }
        val soloFlags = JSONArray()
        for (solo in snapshot.channelSolos) {
            soloFlags.put(solo)
        }
        val groupLevels = JSONArray()
        for (level in snapshot.groupLevels) {
            groupLevels.put(level)
        }
        return JSONObject()
            .put("type", "mix")
            .put("volume_percent", snapshot.volumePercent)
            .put("max_level_percent", snapshot.maxLevelPercent)
            .put("muted", snapshot.muted)
            .put("channels", channels)
            .put("pans", panLevels)
            .put("mutes", muteFlags)
            .put("solos", soloFlags)
            .put("group_levels", groupLevels)
            .toString()
    }
}

/**
 * Builds the registration message a client sends when the control channel opens: its name and the
 * UDP port it listens on. The server takes the address from the socket, so none is carried here.
 */
internal fun buildRegisterMessage(name: String, udpPort: Int): String =
    JSONObject()
        .put("type", "register")
        .put("name", name.trim())
        .put("udp_port", udpPort)
        .toString()

/**
 * Builds the link-stats message the client sends about once a second: the counts of one report
 * interval, all integers. The server matches it to this musician by the socket's IP, like a mix.
 */
internal fun buildStatsMessage(report: LinkStatsReport): String =
    JSONObject()
        .put("type", "stats")
        .put("interval_ms", report.intervalMs)
        .put("packets_received", report.packetsReceived)
        .put("packets_lost", report.packetsLost)
        .put("underruns", report.underruns)
        .put("receive_timeouts", report.receiveTimeouts)
        .toString()

/**
 * Reads the `groups` array from a `config` message. Missing or malformed entries are skipped, and a
 * server that announces no groups yields an empty list, which leaves the client's group section
 * empty rather than inventing one.
 */
private fun parseGroups(array: JSONArray?): List<GroupInfo> {
    if (array == null) return emptyList()
    val groups = ArrayList<GroupInfo>(array.length())
    for (index in 0 until array.length()) {
        val entry = array.optJSONObject(index) ?: continue
        val name = entry.optString("name")
        if (name.isEmpty()) continue
        val channelsJson = entry.optJSONArray("channels")
        val channels = if (channelsJson == null) {
            emptyList()
        } else {
            (0 until channelsJson.length()).map { channelsJson.optInt(it, -1) }.filter { it >= 0 }
        }
        groups.add(GroupInfo(name, channels))
    }
    return groups
}
