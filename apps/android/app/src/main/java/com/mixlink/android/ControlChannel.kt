package com.mixlink.android

import org.json.JSONArray
import org.json.JSONObject
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

/** The WebSocket close code for a deliberate, normal close. */
private const val NORMAL_CLOSURE = 1000

/**
 * The WebSocket control channel, its connection state machine, sending and closing.
 *
 * The channel owns its [transport] (OkHttp WebSockets in production) and its [scheduler] (one
 * background thread in production). It reports opened, acknowledged, config, pushed and answered
 * mixes, error and closed events through [Listener], so the Activity only renders and wires.
 * [running] is the shared session flag owned by the caller.
 *
 * While the session runs, a connection that fails or closes is reopened on its own, following
 * [ReconnectPolicy], to the server and control port given to [connect]. Every open re-sends the
 * registration and then the current mix, so the server gets this musician back as they were. The
 * audio receiver is never touched. [close] stops the session and any pending attempt.
 *
 * Every mix carries [mixSequence], the number of the phone's newest change, and the server answers
 * with it. The channel only carries the number both ways; the caller decides which answer counts.
 */
internal class ControlChannel(
    private val listener: Listener,
    private val running: AtomicBoolean,
    private val currentSnapshot: () -> MixSnapshot,
    private val moreOfMeEnabled: () -> Boolean,
    /**
     * The channels More of me lifts: the musician's channel and, when its strip is a stereo pair, the
     * other side too; empty when no channel is chosen.
     */
    private val musicianChannels: () -> List<Int>,
    /** The number of the phone's newest mix change, sent as `seq`; null sends none. */
    private val mixSequence: () -> Long? = { null },
    private val transport: ControlTransport = OkHttpControlTransport(),
    private val scheduler: ControlScheduler = ExecutorControlScheduler(),
    /** The channels the engineer locked, which More of me leaves as they are. */
    private val lockedChannels: () -> Set<Int> = { emptySet() },
) {
    interface Listener {
        fun onOpened()
        /**
         * The server announced [channels] source channels, its [groups] and the channels' [details]
         * (names, kinds, hidden channels and stereo pairs, one per source channel); [details] is null
         * when the `config` carried none, as an older server's does.
         */
        fun onConfig(channels: Int, groups: List<GroupInfo>, details: List<ChannelInfo>?)
        fun onAcknowledged()

        /**
         * The server pushed this musician's whole mix (`mix_state`) because it changed elsewhere,
         * such as on the desktop, with the engineer's controls. The channel sends nothing back: the
         * server already plays it. A listener that does not follow the server's mix keeps this
         * default and ignores it.
         */
        fun onMixPushed(report: MixReport) = Unit

        /**
         * The server answered the phone's mix number [seq]: [applied] is the whole mix it now plays
         * (its `mix_ack`) with the engineer's controls, or null when it refused the mix (an `error`)
         * or its ack had no readable mix. Answers without a number (from a server that predates
         * them) are never reported here.
         */
        fun onMixAnswered(seq: Long, applied: MixReport?) = Unit

        fun onClosed()
        fun onControlError(message: String)
    }

    /** The open socket that sends go to; null while no connection is open. */
    @Volatile private var controlWebSocket: ControlSocket? = null
    @Volatile private var registrationMessage: String? = null
    @Volatile private var announcedUdpPort: Int? = null
    @Volatile private var announcedClientId: String? = null
    private var pendingNameResend: ScheduledTask? = null
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
    private var currentSocket: ControlSocket? = null
    private var pendingReconnect: ScheduledTask? = null

    /** Set by [close]: the session was stopped, so nothing reconnects until the next [connect]. */
    private var stopped = true

    /** Whether the current connection has heard from the server yet; see [connectionHeardFrom]. */
    private var connectionProved = false

    /** Whether the current outage already showed its error; one error per outage, not per retry. */
    private var outageReported = false

    /**
     * Listens to one connection attempt. Its [generation] ties every callback to the socket it came
     * from: the callbacks of a socket that is no longer current never change the channel's state,
     * and such a socket is only ever closed, never used.
     */
    private inner class ConnectionListener(private val generation: Long) : ControlSocketEvents {
        override fun onOpen(socket: ControlSocket) {
            if (!adoptOpenedSocket(generation, socket)) {
                // Stopped or replaced while it was still opening.
                socket.close(NORMAL_CLOSURE, "superseded")
                return
            }
            listener.onOpened()
            // Registration goes first: the server adds this client as a target from it, so the mix
            // that follows finds a target to join. A reconnect does exactly the same, so the server
            // gets this musician's name and current mix back.
            sendRegistration()
            sendOpeningMix()
        }

        override fun onMessage(socket: ControlSocket, text: String) {
            if (!connectionHeardFrom(generation)) return
            try {
                val message = JSONObject(text)
                when (message.optString("type")) {
                    "error" -> {
                        // Only an error refusing one of the phone's mixes carries its number; an error
                        // answering a registration or a stats report never touches the mixes.
                        message.answeredSequence()?.let { seq -> listener.onMixAnswered(seq, applied = null) }
                        listener.onControlError(message.optString("message", "unknown control error"))
                    }
                    "config" -> {
                        val announcedChannels = message.optInt("source_channels", -1)
                        if (announcedChannels in 1..MAX_CHANNELS) {
                            listener.onConfig(
                                announcedChannels,
                                parseGroups(message.optJSONArray("groups")),
                                parseChannelDetails(message.opt("channels"), announcedChannels),
                            )
                        }
                    }
                    "mix_ack" -> {
                        mixAcknowledged = true
                        listener.onAcknowledged()
                        // Without a number an ack cannot be matched to a mix, so it only acknowledges.
                        // An ack whose mix cannot be read still answers its number.
                        message.answeredSequence()?.let { seq ->
                            listener.onMixAnswered(seq, runCatching { parseMixReport(message) }.getOrNull())
                        }
                    }
                    "mix_state" -> listener.onMixPushed(parseMixReport(message))
                }
            } catch (error: Exception) {
                listener.onControlError("Invalid control response: ${error.message ?: "unknown error"}")
            }
        }

        override fun onClosing(socket: ControlSocket, code: Int, reason: String) {
            // The server is closing. Answer, so the close handshake completes, and treat the
            // connection as ended now: the onClosed that follows belongs to an ended generation.
            socket.close(NORMAL_CLOSURE, null)
            connectionEnded(generation, failure = null)
        }

        override fun onFailure(socket: ControlSocket, error: Throwable) {
            connectionEnded(generation, failure = "WebSocket error: ${error.message ?: "connection failed"}")
        }

        override fun onClosed(socket: ControlSocket, code: Int, reason: String) {
            connectionEnded(generation, failure = null)
        }
    }

    /**
     * Opens the control channel and remembers what to announce when it opens: the musician's name
     * and the UDP port this client listens on. A server that does not understand the registration
     * answers with an error or ignores it, and the rest of the session behaves as before.
     *
     * The host and control port are kept for the whole session, so a lost connection can be
     * reopened to the same server without the musician pressing anything. The [clientId], when
     * given, travels in every registration of the session, so a phone that reconnects from a new
     * IP keeps its row on the server.
     */
    fun connect(host: String, port: Int, udpPort: Int, musicianName: String, clientId: String? = null) {
        announcedUdpPort = udpPort
        announcedClientId = clientId
        registrationMessage = buildRegisterMessage(musicianName, udpPort, clientId)
        synchronized(connectionLock) {
            serverHost = host
            serverControlPort = port
            stopped = false
            reconnectPolicy.reset()
            outageReported = false
            cancelPendingReconnectLocked()
        }
        scheduler.execute { attemptConnection() }
    }

    /**
     * Opens one connection to the stored server, on the control scheduler. Does nothing once the
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
            connectionProved = false
        }
        try {
            val socket = transport.open("ws://$host:$port", ConnectionListener(generation))
            // The attempt can fail, or the session can stop, before open returns: only a socket
            // that is still current is kept, and a stopped one is dropped before it opens.
            val kept = synchronized(connectionLock) {
                generations.isCurrent(generation).also { current -> if (current) currentSocket = socket }
            }
            if (!kept) socket.cancel()
        } catch (error: Exception) {
            if (running.get()) listener.onControlError("Could not start WebSocket: ${error.message ?: "unknown error"}")
        }
    }

    /**
     * Makes the socket of [generation] the open one, if that attempt is still current. Opening alone
     * does not reset the backoff: a server that accepts and drops at once must still back off.
     */
    private fun adoptOpenedSocket(generation: Long, socket: ControlSocket): Boolean {
        synchronized(connectionLock) {
            if (!generations.isCurrent(generation)) return false
            currentSocket = socket
            controlWebSocket = socket
            controlState = ControlState.CONNECTED
        }
        return true
    }

    /**
     * True when a message on connection [generation] should be handled, that is, while it is
     * current. The first message proves the connection healthy (the server sends `config` right
     * after connect): the backoff starts again from its shortest wait, and the next failure counts
     * as a new outage.
     */
    private fun connectionHeardFrom(generation: Long): Boolean {
        synchronized(connectionLock) {
            if (!generations.isCurrent(generation)) return false
            if (!connectionProved) {
                connectionProved = true
                reconnectPolicy.reset()
                outageReported = false
            }
        }
        return true
    }

    /**
     * Handles the end of connection [generation], whichever way it ended. Only the first end of the
     * current connection counts; a stale or repeated one is ignored. While the session runs and was
     * not stopped, the channel shows [ControlState.RECONNECTING] and schedules the next attempt.
     * Ownership of the mix is left exactly as it was.
     *
     * A failure is shown once per outage: the first one of the session, or the first after a
     * connection that had heard from the server. Retries that keep failing only update the state.
     */
    private fun connectionEnded(generation: Long, failure: String?) {
        val error: String?
        synchronized(connectionLock) {
            if (!generations.end(generation)) return
            currentSocket = null
            controlWebSocket = null
            val sessionRunning = running.get()
            val reconnect = ReconnectPolicy.shouldReconnect(sessionRunning, stopped)
            controlState = when {
                reconnect -> ControlState.RECONNECTING
                !sessionRunning -> ControlState.IDLE
                failure != null -> ControlState.UNAVAILABLE
                else -> ControlState.CLOSED
            }
            if (reconnect) scheduleReconnectLocked()
            error = failure?.takeIf { sessionRunning && !outageReported }
            if (error != null) outageReported = true
        }
        listener.onClosed()
        if (error != null) listener.onControlError(error)
    }

    /** Schedules the next attempt after the policy's wait. Call only while holding [connectionLock]. */
    private fun scheduleReconnectLocked() {
        // Null once the scheduler shut down: nothing reconnects after that.
        pendingReconnect = scheduler.schedule(reconnectPolicy.nextDelayMs()) { attemptConnection() }
    }

    /** Cancels a scheduled attempt, if any. Call only while holding [connectionLock]. */
    private fun cancelPendingReconnectLocked() {
        pendingReconnect?.cancel()
        pendingReconnect = null
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
        val message = buildRegisterMessage(name, udpPort, announcedClientId)
        registrationMessage = message
        pendingNameResend?.cancel()
        pendingNameResend = scheduler.schedule(NAME_RESEND_DEBOUNCE_MS) { sendRegistrationMessage(message) }
    }

    /**
     * Sends the registration once, on open, ahead of the first mix. Queued on the same scheduler as
     * the mix sends, so the server always sees the registration first.
     */
    private fun sendRegistration() {
        val message = registrationMessage ?: return
        scheduler.execute {
            sendRegistrationMessage(message)
        }
    }

    private fun sendRegistrationMessage(message: String) {
        val webSocket = controlWebSocket ?: return
        if (!webSocket.send(message) && running.get()) {
            listener.onControlError("WebSocket rejected the registration")
        }
    }

    /**
     * Sends the mix as it stands when the connection opens. It is read now, on the socket's thread,
     * before any message of this connection is handled, and only sent on the control scheduler: the
     * server's `config` arrives right after the open, and a mix read after it would carry the strips
     * `config` made up (neutral, for channels the phone never had) over the server's own values.
     *
     * So on a session's first connect, when the phone has no channels yet, it sends none (nor any
     * group level): the server keeps its channels and groups, and its answer, which arrives after
     * `config`, gives the phone the mix the server holds. Only the master volume, the level cap and
     * the mute, which are always the phone's own, are sent as they are. A reconnect reads the
     * phone's own mix, which the server takes back as it was.
     */
    private fun sendOpeningMix() {
        val message = outgoingMixMessage()
        scheduler.execute {
            val webSocket = controlWebSocket ?: return@execute
            if (!webSocket.send(message) && running.get()) {
                listener.onControlError("WebSocket rejected mix update")
            }
        }
    }

    /**
     * Clears ownership so a fresh session starts with the client owning its mix. The client re-sends
     * its mix on connect and regains ownership on the next acknowledgement; until then the local
     * master volume stays live even if the control channel never comes up.
     */
    fun clearMixOwnership() {
        mixAcknowledged = false
    }

    /**
     * Sends the current mix, on the control scheduler, under the newest change's number. Sends that
     * pile up behind a busy scheduler each read the mix and the number when they run, so the mix on
     * the wire is always the newest one.
     */
    fun sendMixControl() {
        scheduler.execute {
            val webSocket = controlWebSocket ?: return@execute
            if (!webSocket.send(outgoingMixMessage()) && running.get()) {
                listener.onControlError("WebSocket rejected mix update")
            }
        }
    }

    /** The `mix` message for the phone's current mix, under the newest change's number. */
    private fun outgoingMixMessage(): String {
        // The number is read before the mix: a change numbers itself after it lands, so the mix read
        // next is at least as new as the number, and an answer never vouches for a mix older than
        // the one it names.
        val sequence = mixSequence()
        val stored = currentSnapshot()
        // `More of me` is derived here, at send time, and never written back. The stored values stay
        // exactly as the musician left them, so turning the control off re-sends them.
        val outgoing = if (moreOfMeEnabled()) stored.moreOfMeFor(musicianChannels(), lockedChannels()) ?: stored else stored
        return buildMixMessage(outgoing, sequence)
    }

    /**
     * Sends one link-stats report on the control scheduler, behind any registration or mix already
     * queued. Called from the receive thread, so it only hands the report over and never waits.
     *
     * A report describes one interval and is worthless later, so it is dropped, never queued, when
     * the channel is not open or has shut down (the scheduler drops work after shutdown). A rejected
     * send is not shown as an error either: the next report follows within a second, and the
     * server's view of a link that stops reporting is exactly what the report is for.
     */
    fun sendLinkStats(report: LinkStatsReport) {
        if (controlWebSocket == null) return
        scheduler.execute {
            controlWebSocket?.send(buildStatsMessage(report))
        }
    }

    /**
     * Stops the session's control connection: cancels any scheduled reconnect, makes every socket's
     * callbacks stale and closes the current socket, open or still opening. Nothing reconnects until
     * the next [connect].
     */
    fun close(code: Int, reason: String) {
        val socket: ControlSocket?
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
        scheduler.shutdown()
        transport.shutdown()
    }

    private fun buildMixMessage(snapshot: MixSnapshot, sequence: Long?): String {
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
            // A null number puts no key at all, so the message is exactly what older servers read.
            .put("seq", sequence)
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

/** The phone's `stats` carry `jitter_ms`, `cushion_ms` and `backlog_ms` ([LinkStatsReport]). */
internal const val CAPABILITY_STATS_V2 = "stats.v2"

/**
 * What this build of the app implements, announced in every registration so the server only sends
 * a phone the messages it understands. A feature adds its name here, one line, once the phone
 * implements it: `playback.buffer` (the buffer set from the desk) and `redundancy.recovered` (the
 * packets a redundant copy brought) are still to come. Names are 1-32 of `[a-z0-9._-]`, at most 16.
 */
internal val CLIENT_CAPABILITIES: List<String> = listOf(
    CAPABILITY_STATS_V2,
)

/**
 * Builds the registration message a client sends when the control channel opens: its name, the
 * UDP port it listens on, when it has one its stable client id (see [ClientIdentity]), which lets
 * the server keep this musician's row when the phone returns from a new IP, and the [capabilities]
 * it implements. The server takes the address from the socket, so none is carried here. An older
 * server ignores the fields it does not know, so the capabilities never break a registration.
 */
internal fun buildRegisterMessage(
    name: String,
    udpPort: Int,
    clientId: String? = null,
    capabilities: List<String> = CLIENT_CAPABILITIES,
): String {
    val message = JSONObject()
        .put("type", "register")
        .put("name", name.trim())
        .put("udp_port", udpPort)
    if (clientId != null) message.put("client_id", clientId)
    if (capabilities.isNotEmpty()) message.put("capabilities", JSONArray(capabilities))
    return message.toString()
}

/** The longest duration a `stats` extra may name; the server reads a longer one as absent. */
private const val MAX_STATS_EXTRA_MS = 60_000.0

/**
 * Builds the link-stats message the client sends about once a second: the counts of one report
 * interval, all integers, then the extras of `stats.v2` that were measured, in milliseconds to the
 * hundredth. An extra not measured (`NaN`) or outside 0 to 60 000 ms is left out, never sent as a
 * value the server would refuse. The server matches it to this musician by the socket's IP, like a
 * mix, and an older server ignores the extras.
 */
internal fun buildStatsMessage(report: LinkStatsReport): String {
    val message = JSONObject()
        .put("type", "stats")
        .put("interval_ms", report.intervalMs)
        .put("packets_received", report.packetsReceived)
        .put("packets_lost", report.packetsLost)
        .put("underruns", report.underruns)
        .put("receive_timeouts", report.receiveTimeouts)
    message.putMilliseconds("jitter_ms", report.jitterMs)
    message.putMilliseconds("cushion_ms", report.cushionMs)
    message.putMilliseconds("backlog_ms", report.backlogMs)
    return message.toString()
}

private fun JSONObject.putMilliseconds(key: String, value: Double) {
    // A NaN fails both comparisons, so a value not measured stays out too.
    if (value >= 0.0 && value <= MAX_STATS_EXTRA_MS) {
        put(key, Math.round(value * 100.0) / 100.0)
    }
}

/**
 * Reads a whole mix as the server reports it, in the field names of the `mix` message: `mix_state`
 * carries it, and `mix_ack` shares the shape. The lists are the server's full tables (32 channels,
 * 16 groups); the client keeps the channels and groups it knows. The server always sends every
 * field, so a missing or mistyped one throws: such a push is reported as invalid, never applied, and
 * such an ack still acknowledges but shows nothing. The engineer's controls came later and are read
 * leniently ([parseEngineerControls]).
 */
internal fun parseMixReport(message: JSONObject): MixReport = MixReport(
    mix = MixSnapshot(
        channelGains = message.getJSONArray("channels").ints(),
        channelPans = message.getJSONArray("pans").ints(),
        channelMutes = message.getJSONArray("mutes").booleans(),
        channelSolos = message.getJSONArray("solos").booleans(),
        groupLevels = message.getJSONArray("group_levels").ints(),
        volumePercent = message.getInt("volume_percent"),
        maxLevelPercent = message.getInt("max_level_percent"),
        muted = message.getBoolean("muted"),
    ),
    engineer = parseEngineerControls(message),
)

/**
 * Reads the engineer's controls from a mix report: `engineer_limit_percent` (held to 0-100), `locked`
 * (one entry per channel, `true` for a locked one) and `following` (the name of the musician whose
 * mix this one follows). All are lenient: a server that predates them sends none, which reads as no
 * limit, nothing locked and following nobody; a limit that is not a number is no limit, a short list
 * leaves the rest open, an entry that is not a boolean is open, and a `following` that is null or not
 * a string follows nobody (an empty name is still followed).
 */
internal fun parseEngineerControls(message: JSONObject): EngineerControls {
    val limit = (message.opt("engineer_limit_percent") as? Number)?.toInt()?.coerceIn(0, 100) ?: 100
    val locked = message.optJSONArray("locked")
    val lockedChannels = if (locked == null) emptySet() else (0 until locked.length()).filter { locked.opt(it) == true }.toSet()
    return EngineerControls(limit, lockedChannels, following = message.opt("following") as? String)
}

/** The number of the phone's mix this answer names (`seq`), or null when it names none. */
private fun JSONObject.answeredSequence(): Long? = if (opt("seq") is Number) getLong("seq").takeIf { it >= 0 } else null

private fun JSONArray.ints(): IntArray = IntArray(length()) { getInt(it) }

private fun JSONArray.booleans(): BooleanArray = BooleanArray(length()) { getBoolean(it) }

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

/**
 * Reads the `channels` array of a `config` message: one [ChannelInfo] per announced source channel,
 * so exactly [count] entries. Returns null when the message has no array there, as an older
 * server's has none.
 *
 * It is lenient, like [parseGroups]: an entry past [count] is dropped, a missing one, one that is not
 * an object, or a field of the wrong type reads as the default (an untouched channel), and a kind
 * this build does not know reads as [ChannelKind.OTHER].
 */
internal fun parseChannelDetails(value: Any?, count: Int): List<ChannelInfo>? {
    val array = value as? JSONArray ?: return null
    return List(count) { index ->
        val entry = array.optJSONObject(index) ?: return@List ChannelInfo()
        ChannelInfo(
            name = entry.opt("name") as? String ?: "",
            kind = ChannelKind.fromWire(entry.opt("kind") as? String),
            hidden = entry.opt("hidden") == true,
            link = entry.opt("link") == true,
        )
    }
}
