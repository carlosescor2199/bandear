package com.mixlink.android.ui.state

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import com.mixlink.android.ClientMixState
import com.mixlink.android.ControlChannel
import com.mixlink.android.ControlState
import com.mixlink.android.DiscoveredServer
import com.mixlink.android.GroupInfo
import com.mixlink.android.LinkStatsReport
import com.mixlink.android.PmonPlayer
import com.mixlink.android.ServerDiscovery
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.screens.MonitorActions
import com.mixlink.android.ui.state.MonitorEvent.AudioError
import com.mixlink.android.ui.state.MonitorEvent.ChannelLevelChanged
import com.mixlink.android.ui.state.MonitorEvent.ChannelMuteChanged
import com.mixlink.android.ui.state.MonitorEvent.ChannelPanChanged
import com.mixlink.android.ui.state.MonitorEvent.ChannelSoloChanged
import com.mixlink.android.ui.state.MonitorEvent.ConfigReceived
import com.mixlink.android.ui.state.MonitorEvent.ConnectRejected
import com.mixlink.android.ui.state.MonitorEvent.ControlChanged
import com.mixlink.android.ui.state.MonitorEvent.ControlError
import com.mixlink.android.ui.state.MonitorEvent.GroupLevelChanged
import com.mixlink.android.ui.state.MonitorEvent.LevelCapChanged
import com.mixlink.android.ui.state.MonitorEvent.LinkStatsReported
import com.mixlink.android.ui.state.MonitorEvent.ManualAddressEdited
import com.mixlink.android.ui.state.MonitorEvent.MasterMuteChanged
import com.mixlink.android.ui.state.MonitorEvent.MasterVolumeChanged
import com.mixlink.android.ui.state.MonitorEvent.MixAcknowledged
import com.mixlink.android.ui.state.MonitorEvent.MixDeleted
import com.mixlink.android.ui.state.MonitorEvent.MixMissing
import com.mixlink.android.ui.state.MonitorEvent.MixNotice
import com.mixlink.android.ui.state.MonitorEvent.MixRecalled
import com.mixlink.android.ui.state.MonitorEvent.MixRenamed
import com.mixlink.android.ui.state.MonitorEvent.MixSaved
import com.mixlink.android.ui.state.MonitorEvent.MixesLoaded
import com.mixlink.android.ui.state.MonitorEvent.MoreOfMeToggled
import com.mixlink.android.ui.state.MonitorEvent.MusicianChannelChosen
import com.mixlink.android.ui.state.MonitorEvent.NameChanged
import com.mixlink.android.ui.state.MonitorEvent.ServersChanged
import com.mixlink.android.ui.state.MonitorEvent.SessionEnded
import com.mixlink.android.ui.state.MonitorEvent.SessionStarted
import com.mixlink.android.ui.state.MonitorEvent.StatsUpdated
import com.mixlink.android.ui.state.MonitorEvent.TabSelected
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import java.util.concurrent.atomic.AtomicBoolean

/** The WebSocket close code for a deliberate, normal close. */
private const val NORMAL_CLOSE = 1000

private val PORTS = 1..65535

/**
 * Owns a monitoring session and everything the screens show, in one [state].
 *
 * It holds what `MainActivity` held: the session flag, the control channel, the audio receiver,
 * discovery, the saved mixes and the client id, all built from [MonitorCollaborators]. The screens
 * read [state] and call the intent functions; collaborator callbacks become [MonitorEvent]s for the
 * pure [reduce]. The behaviour is `MainActivity`'s: what starts and stops a session, who owns the
 * mix, how More of me is derived at send time, what a saved mix stores and recalls, and how a name
 * change is re-announced.
 *
 * Callbacks arrive on the receiver, control and discovery threads. The state is updated by an
 * atomic compare-and-set, so any thread may report. Starting and ending a session is serialised by
 * one lock, as the Activity's UI thread serialised it. A session ends once: the receiver's stop
 * report only ends the session it belongs to, so a report arriving late cannot end a session twice
 * or close a session started after it, and after [onCleared] it touches nothing. Intents are meant
 * for the main thread. They are the screens' [MonitorActions], so the app binds them directly.
 */
internal class MonitorViewModel(collaborators: MonitorCollaborators) : ViewModel(), MonitorActions {
    private val store = collaborators.store
    private val clientId = collaborators.clientId

    /** The shared session flag: the receive loop and the control channel both stop on it. */
    private val running = AtomicBoolean(false)

    /** The master values the receive thread reads for local protection; mirrored from [state]. */
    private val playerMix = ClientMixState()

    private val sessionLock = Any()

    // Which session a receiver stop report belongs to. The receiver reports exactly one stop for
    // each launch, from the launch's `finally`, in launch order, so the n-th report is the n-th
    // session's. All three are guarded by [sessionLock].

    /** Sessions launched so far; the current (or last) session is number [sessionsLaunched]. */
    private var sessionsLaunched = 0L

    /** Stop reports received so far. */
    private var stopsReported = 0L

    /** The last session torn down, by Stop or by its receiver; it is never torn down again. */
    private var lastEndedSession = 0L

    /** Set by [onCleared]: the channel and discovery are shut down, so nothing may end a session. */
    private var cleared = false

    private val _state = MutableStateFlow(restoredState())

    /** Everything the screens show. */
    val state: StateFlow<MonitorUiState> = _state.asStateFlow()

    private val receiverEvents = object : PmonPlayer.Listener {
        override fun serverOwnsMix(): Boolean = control.mixAcknowledged

        override fun onStats(
            packets: Long,
            lost: Long,
            sampleRate: Int?,
            packetsPerSecond: Double,
            interArrivalMs: Double,
            jitterMs: Double,
            discardedSamples: Long,
            packetsWithDiscardedSamples: Long,
            playbackBacklogMs: Double,
        ) {
            dispatch(
                StatsUpdated(
                    ReceiverStats(
                        packets = packets,
                        lost = lost,
                        sampleRate = sampleRate,
                        packetsPerSecond = packetsPerSecond,
                        interArrivalMs = interArrivalMs,
                        jitterMs = jitterMs,
                        discardedSamples = discardedSamples,
                        packetsWithDiscardedSamples = packetsWithDiscardedSamples,
                        playbackBacklogMs = playbackBacklogMs,
                    ),
                ),
            )
        }

        override fun onLinkStats(report: LinkStatsReport) {
            control.sendLinkStats(report)
            dispatch(LinkStatsReported(report))
        }

        override fun onError(message: String) {
            dispatch(AudioError(message))
        }

        override fun onStopped() = receiverStopped()
    }

    private val controlEvents = object : ControlChannel.Listener {
        override fun onOpened() = publishControlState()

        override fun onConfig(channels: Int, groups: List<GroupInfo>) = configReceived(channels, groups)

        override fun onAcknowledged() {
            dispatch(MixAcknowledged(control.controlState))
        }

        override fun onClosed() = publishControlState()

        override fun onControlError(message: String) {
            dispatch(ControlError(message))
        }
    }

    private val scannerEvents = object : ServerDiscovery.Listener {
        override fun onServersChanged(servers: List<DiscoveredServer>) {
            dispatch(ServersChanged(servers.map { ServerEndpoint(it.address, it.controlPort) }))
        }
    }

    private val control: ControlChannel = ControlChannel(
        listener = controlEvents,
        running = running,
        // The channel reads the stored mix at send time and derives More of me from it there, so
        // turning More of me off sends the mix exactly as the musician left it.
        currentSnapshot = { _state.value.mix.toSnapshot() },
        moreOfMeEnabled = { _state.value.moreOfMe.enabled },
        musicianChannel = { _state.value.moreOfMe.musicianChannel },
        transport = collaborators.controlTransport,
        scheduler = collaborators.controlScheduler,
    )
    private val receiver: AudioReceiver = collaborators.receiverFactory(receiverEvents, running, playerMix)
    private val scanner: ServerScanner = collaborators.scannerFactory(scannerEvents)

    init {
        scanner.start()
    }

    // The session.

    /** Connects to a found [server], listening for audio on the port set in Settings. */
    override fun connect(server: ServerEndpoint) {
        startSession(server.address, server.controlPort.toString(), _state.value.connection.manual.audioPort)
    }

    /** Connects to a typed address. The form keeps what was typed, so a mistake can be corrected. */
    override fun connectManual(host: String, controlPort: String, audioPort: String) {
        dispatch(ManualAddressEdited(ManualAddress(host, controlPort, audioPort)))
        startSession(host, controlPort, audioPort)
    }

    override fun editManualAddress(address: ManualAddress) {
        dispatch(ManualAddressEdited(address))
    }

    private fun startSession(hostText: String, controlPortText: String, audioPortText: String) {
        synchronized(sessionLock) {
            if (running.get()) return
            if (receiver.isAlive()) return reject("The previous receiver is still stopping")
            val host = hostText.trim()
            val audioPort = audioPortText.trim().toIntOrNull()?.takeIf { it in PORTS }
            val controlPort = controlPortText.trim().toIntOrNull()?.takeIf { it in PORTS }
            if (host.isEmpty()) return reject("Enter the server IP address or host name")
            if (audioPort == null) return reject("Enter a valid UDP port between 1 and 65535")
            if (controlPort == null) return reject("Enter a valid WebSocket control port between 1 and 65535")

            dispatch(SessionStarted(ServerEndpoint(host, controlPort), audioPort))
            scanner.stop()
            running.set(true)
            control.controlState = ControlState.CONNECTING
            control.clearMixOwnership()
            // The name and the UDP port travel in the registration, so the server adds this musician
            // as a target with no address typed; the client id keeps their row when the IP changes.
            control.connect(host, controlPort, audioPort, _state.value.musicianName, clientId())
            sessionsLaunched++
            receiver.launch(host, audioPort)
        }
    }

    private fun reject(message: String) {
        dispatch(ConnectRejected(message))
    }

    /** Ends the session: the receiver stops, the control channel closes and discovery listens again. */
    override fun disconnect() {
        synchronized(sessionLock) {
            if (!running.getAndSet(false)) return
            // Ended before the receiver is asked to stop, so its stop report finds nothing left to end.
            lastEndedSession = sessionsLaunched
            receiver.stop()
            closeSession("stopped")
        }
    }

    /**
     * The receiver ended, on request or on its own, and reported it. Only the latest session's
     * report ends a session, and only once: a report for a session already ended (by Stop, or one
     * replaced by a newer session) does nothing, and after [onCleared] no report does anything.
     */
    private fun receiverStopped() {
        synchronized(sessionLock) {
            // The real receiver never reports more stops than launches; a stray report counts as
            // the latest session's, so the counts cannot drift apart.
            stopsReported = minOf(stopsReported + 1, sessionsLaunched)
            val session = stopsReported
            if (cleared || session != sessionsLaunched || session == lastEndedSession) return
            lastEndedSession = session
            closeSession("receiver stopped")
        }
    }

    /** The end of a session: the control channel closes and stops reconnecting, and discovery listens again. */
    private fun closeSession(reason: String) {
        control.close(NORMAL_CLOSE, reason)
        control.controlState = ControlState.IDLE
        dispatch(SessionEnded)
        scanner.start()
    }

    private fun publishControlState() {
        dispatch(ControlChanged(control.controlState, control.mixAcknowledged))
    }

    /** A `config`; when it drops the musician's channel the phone forgets it, and resends if More of me went off. */
    private fun configReceived(channels: Int, groups: List<GroupInfo>) {
        val (before, after) = dispatch(ConfigReceived(channels, groups))
        if (before.moreOfMe.musicianChannel != after.moreOfMe.musicianChannel) {
            store.setMusicianChannel(after.moreOfMe.musicianChannel)
        }
        if (before.moreOfMe.enabled && !after.moreOfMe.enabled) control.sendMixControl()
    }

    // The mix. A change that changes nothing (a value already set, a channel that does not exist)
    // sends nothing, as a slider that did not move never did.

    override fun setChannelLevel(channel: Int, level: Int) = changeMix(ChannelLevelChanged(channel, level))

    fun setChannelPan(channel: Int, pan: Int) = changeMix(ChannelPanChanged(channel, pan))

    override fun setChannelMuted(channel: Int, muted: Boolean) = changeMix(ChannelMuteChanged(channel, muted))

    override fun setChannelSoloed(channel: Int, soloed: Boolean) = changeMix(ChannelSoloChanged(channel, soloed))

    override fun setGroupLevel(group: Int, level: Int) = changeMix(GroupLevelChanged(group, level))

    override fun setMasterVolume(level: Int) = changeMix(MasterVolumeChanged(level))

    override fun setMasterMuted(muted: Boolean) = changeMix(MasterMuteChanged(muted))

    override fun setLevelCap(level: Int) = changeMix(LevelCapChanged(level))

    private fun changeMix(event: MonitorEvent) {
        val (before, after) = dispatch(event)
        if (before.mix == after.mix) return
        syncPlayerMix()
        control.sendMixControl()
    }

    /**
     * Copies the master values to the mix state the receive thread reads. Only the main thread
     * changes them, so the copy always matches [state].
     */
    private fun syncPlayerMix() {
        val mix = _state.value.mix
        playerMix.volumePercent = mix.masterVolume
        playerMix.maxLevelPercent = mix.levelCap
        playerMix.muted = mix.masterMuted
    }

    // More of me.

    /** Turns More of me on or off; it stays off, with a notice, until a channel is chosen. */
    override fun setMoreOfMe(on: Boolean) {
        val (before, after) = dispatch(MoreOfMeToggled(on))
        if (before.moreOfMe.enabled != after.moreOfMe.enabled) control.sendMixControl()
    }

    /** Takes [channel] as the musician's, or none, and remembers it on the phone. */
    override fun setMusicianChannel(channel: Int?) {
        val (before, after) = dispatch(MusicianChannelChosen(channel))
        if (after.moreOfMe.musicianChannel != channel) return
        store.setMusicianChannel(channel)
        // Either More of me now lifts another channel, or it just went off: the mix sent changes.
        if (before.moreOfMe.enabled) control.sendMixControl()
    }

    // Saved mixes. They hold the stored mix, never the More of me derivation.

    /**
     * Saves the current mix as [name], trimmed; a name already used is updated. Returns whether it
     * was saved: a blank name is refused with a notice, and the screen keeps what was typed.
     */
    override fun saveMix(name: String): Boolean {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) {
            dispatch(MixNotice(NAME_NEEDED))
            return false
        }
        store.saveBank(trimmed, _state.value.mix.toSnapshot())
        dispatch(MixSaved(trimmed, savedNames()))
        return true
    }

    /** Applies a saved mix and sends it, exactly as if the musician had moved every control. */
    override fun recallMix(name: String) {
        val bank = store.loadBanks().firstOrNull { it.name == name }
        if (bank == null) {
            dispatch(MixMissing(name, savedNames()))
            return
        }
        dispatch(MixRecalled(name, bank.snapshot))
        syncPlayerMix()
        control.sendMixControl()
    }

    /** Renames a saved mix. A name another mix already has is refused, so no mix is overwritten. */
    override fun renameMix(from: String, to: String) {
        val target = to.trim()
        val banks = store.loadBanks()
        val bank = banks.firstOrNull { it.name == from }
        when {
            target.isEmpty() -> dispatch(MixNotice(NAME_NEEDED))
            bank == null -> dispatch(MixMissing(from, banks.map { it.name }))
            target == from -> Unit
            banks.any { it.name == target } -> dispatch(MixNotice("A mix named \"$target\" already exists"))
            else -> {
                store.saveBank(target, bank.snapshot)
                store.deleteBank(from)
                dispatch(MixRenamed(from, target, savedNames()))
            }
        }
    }

    override fun deleteMix(name: String) {
        store.deleteBank(name)
        dispatch(MixDeleted(name, savedNames()))
    }

    private fun savedNames(): List<String> = store.loadBanks().map { it.name }

    // The rest.

    /**
     * Keeps the name as typed, remembers it and re-announces it on the open control channel. The
     * channel waits for a pause in typing before it sends, so a word is one message, not one per key.
     */
    override fun setName(name: String) {
        dispatch(NameChanged(name))
        store.setMusicianName(name)
        control.updateRegistrationName(name)
    }

    override fun selectTab(tab: MainTab) {
        dispatch(TabSelected(tab))
    }

    override fun onCleared() {
        synchronized(sessionLock) {
            disconnect()
            // A receiver report arriving from now on must not reach the channel or discovery,
            // which are shut down below.
            cleared = true
        }
        control.close(NORMAL_CLOSE, "destroyed")
        control.shutdown()
        scanner.shutdown()
    }

    /** Applies [event] atomically and returns the state before and after it. */
    private fun dispatch(event: MonitorEvent): Pair<MonitorUiState, MonitorUiState> {
        while (true) {
            val before = _state.value
            val after = reduce(before, event)
            if (_state.compareAndSet(before, after)) return before to after
        }
    }

    /** The state at launch: the name, the musician channel and the saved mixes kept on the phone. */
    private fun restoredState(): MonitorUiState {
        val restored = MonitorUiState(
            musicianName = store.musicianName(),
            moreOfMe = MoreOfMeUiState(musicianChannel = store.musicianChannel()),
        )
        return reduce(restored, MixesLoaded(savedNames()))
    }

    companion object {
        private const val NAME_NEEDED = "Enter a name for the mix"

        /** Builds the ViewModel on the real network, audio and storage of the application. */
        val Factory: ViewModelProvider.Factory = viewModelFactory {
            initializer {
                val application = checkNotNull(this[ViewModelProvider.AndroidViewModelFactory.APPLICATION_KEY])
                MonitorViewModel(MonitorCollaborators.production(application))
            }
        }
    }
}
