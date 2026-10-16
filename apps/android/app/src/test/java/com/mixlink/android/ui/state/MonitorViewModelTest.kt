package com.mixlink.android.ui.state

import androidx.lifecycle.ViewModel
import androidx.lifecycle.ViewModelProvider
import androidx.lifecycle.ViewModelStore
import com.mixlink.android.DiscoveredServer
import com.mixlink.android.LinkStatsReport
import com.mixlink.android.MixBank
import com.mixlink.android.MixSnapshot
import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.components.PillKind
import com.mixlink.android.ui.screens.MonitorActions
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The ViewModel's wiring, driven through fakes of its seams and the real control channel. The
 * channel runs on a fake scheduler and transport, so what reaches the "server" is read straight from
 * the socket the test opened.
 */
class MonitorViewModelTest {
    private val store = FakeStore(name = "Ana")
    private val transport = FakeTransport()
    private val scheduler = FakeControlScheduler()
    private lateinit var receiver: FakeReceiver
    private lateinit var scanner: FakeScanner

    private val viewModel by lazy { newViewModel() }

    private fun newViewModel() = MonitorViewModel(
        MonitorCollaborators(
            store = store,
            clientId = { CLIENT_ID },
            receiverFactory = { listener, running, mixState -> FakeReceiver(listener, running, mixState).also { receiver = it } },
            scannerFactory = { listener -> FakeScanner(listener).also { scanner = it } },
            controlTransport = transport,
            controlScheduler = scheduler,
        ),
    )

    private val state get() = viewModel.state.value

    /** Builds the ViewModel now, for a test that acts on its collaborators first. */
    private fun started(): MonitorViewModel = viewModel

    /** Builds the ViewModel in [owner], as an Activity would, so clearing [owner] runs `onCleared`. */
    private fun ownedBy(owner: ViewModelStore): MonitorViewModel {
        val factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T = newViewModel() as T
        }
        return ViewModelProvider(owner, factory)[MonitorViewModel::class.java]
    }

    private fun socket() = transport.sockets.last()

    private fun lastMix(): JSONObject = socket().sentOfType("mix").last()

    private fun JSONObject.ints(key: String): List<Int> = getJSONArray(key).let { array -> (0 until array.length()).map(array::getInt) }

    private fun JSONArray.booleans(): List<Boolean> = (0 until length()).map { getBoolean(it) }

    /** Connects to the found server, opens the channel and lets the server announce [channels]. */
    private fun connected(channels: Int = 2): MonitorViewModel {
        viewModel.connect(SERVER)
        socket().open()
        socket().receive("""{"type":"config","source_channels":$channels,"groups":[]}""")
        return viewModel
    }

    /** How many of the phone's mixes on each socket the server has answered so far. */
    private val mixesAnswered = mutableMapOf<FakeSocket, Int>()

    /**
     * The server answers every mix the phone sent on the current socket and it has not answered yet,
     * in order, as it does: with the mix as applied and the mix's own number.
     */
    private fun answerMixes() {
        val socket = socket()
        val mixes = socket.sentOfType("mix")
        mixes.drop(mixesAnswered[socket] ?: 0).forEach { socket.receive(it.put("type", "mix_ack").toString()) }
        mixesAnswered[socket] = mixes.size
    }

    /**
     * A mix report of [type] as the server writes it (`mix_state` when the mix changed elsewhere,
     * `mix_ack` answering the phone's mix number [seq]): its full 32-channel and 16-group tables,
     * with [levels] on the first channels and the rest neutral.
     */
    private fun mixState(
        levels: List<Int>,
        volume: Int = 100,
        cap: Int = 100,
        muted: Boolean = false,
        type: String = "mix_state",
        seq: Long? = null,
    ): String = JSONObject()
        .put("type", type)
        .put("seq", seq)
        .put("volume_percent", volume)
        .put("max_level_percent", cap)
        .put("muted", muted)
        .put("channels", JSONArray(List(32) { levels.getOrElse(it) { 100 } }))
        .put("pans", JSONArray(List(32) { if (it % 2 == 0) 0 else 100 }))
        .put("mutes", JSONArray(List(32) { false }))
        .put("solos", JSONArray(List(32) { false }))
        .put("group_levels", JSONArray(List(16) { 100 }))
        .toString()

    @Test
    fun startingRestoresTheNameTheChannelAndTheSavedMixesAndListensForServers() {
        store.channel = 1
        store.banks += MixBank("Verse", MixSnapshot())

        assertEquals("Ana", state.musicianName)
        assertEquals(1, state.moreOfMe.musicianChannel)
        assertEquals(listOf("Verse"), state.savedMixes.names)
        assertNull(state.savedMixes.emptyMessage)
        assertEquals(1, scanner.starts)
    }

    @Test
    fun foundServersAreListed() {
        started()

        scanner.listener.onServersChanged(listOf(DiscoveredServer("10.0.0.2", 50001, lastSeenMs = 0)))

        assertEquals(listOf(ServerEndpoint("10.0.0.2", 50001)), state.servers)
    }

    @Test
    fun connectingToAFoundServerOpensItsControlChannelAndStartsTheReceiver() {
        viewModel.connect(SERVER)

        assertEquals(listOf("ws://192.168.1.20:50001"), transport.sockets.map { it.url })
        assertEquals(listOf("192.168.1.20" to DEFAULT_AUDIO_PORT), receiver.launches)
        assertTrue(receiver.running.get())
        assertEquals(1, scanner.stops)
        assertTrue(state.connection.sessionRunning)
        assertEquals(SERVER, state.connection.server)
        assertEquals(PillKind.CONNECTING, state.connection.pill)
    }

    @Test
    fun theRegistrationCarriesTheStoredClientIdAndTheName() {
        viewModel.connect(SERVER)

        socket().open()

        val register = socket().sentOfType("register").single()
        assertEquals(CLIENT_ID, register.getString("client_id"))
        assertEquals("Ana", register.getString("name"))
        assertEquals(DEFAULT_AUDIO_PORT, register.getInt("udp_port"))
        assertEquals(listOf("register", "mix"), socket().sent.map { JSONObject(it).getString("type") })
    }

    @Test
    fun controlStateChangesMoveThePill() {
        viewModel.connect(SERVER)
        assertEquals(PillKind.CONNECTING, state.connection.pill)

        socket().open()
        assertEquals(PillKind.CONNECTED, state.connection.pill)

        socket().receive("""{"type":"mix_ack"}""")
        assertTrue(state.connection.mixHeldByServer)
        assertTrue(receiver.listener.serverOwnsMix())

        socket().fail("wifi gone")
        assertEquals(PillKind.RECONNECTING, state.connection.pill)
        assertTrue(state.connection.reconnecting)
        assertTrue(state.connection.mixHeldByServer)
        assertEquals("WebSocket error: wifi gone", state.diagnostics.controlError)
        assertEquals(listOf(500L), scheduler.pendingDelays())
    }

    @Test
    fun disconnectStopsTheReceiverClosesTheChannelAndListensAgain() {
        connected()

        viewModel.disconnect()

        assertEquals(1, receiver.stops)
        assertFalse(receiver.running.get())
        assertEquals(1000 to "stopped", socket().closedWith)
        assertFalse(state.connection.sessionRunning)
        assertEquals(PillKind.NOT_CONNECTED, state.connection.pill)
        assertEquals(2, scanner.starts)

        viewModel.disconnect()

        assertEquals(1, receiver.stops)
    }

    @Test
    fun aReceiverThatEndsOnItsOwnEndsTheSession() {
        connected()

        receiver.endOnItsOwn()

        assertEquals(1000 to "receiver stopped", socket().closedWith)
        assertFalse(state.connection.sessionRunning)
        assertEquals(PillKind.NOT_CONNECTED, state.connection.pill)
        assertEquals(emptyList<Long>(), scheduler.pendingDelays())
        assertEquals(2, scanner.starts)
    }

    @Test
    fun theReceiversLateStopReportAfterADisconnectEndsNothingAgain() {
        connected()
        viewModel.disconnect()
        val ended = state

        receiver.finishStopping()

        assertEquals(1000 to "stopped", socket().closedWith)
        assertEquals(2, scanner.starts)
        assertEquals(ended, state)
    }

    @Test
    fun aReceiverThatEndsOnItsOwnEndsTheSessionOnlyOnce() {
        connected()
        receiver.endOnItsOwn()

        receiver.listener.onStopped()
        viewModel.disconnect()

        assertEquals(1000 to "receiver stopped", socket().closedWith)
        assertEquals(2, scanner.starts)
        assertEquals(0, receiver.stops)
        assertFalse(state.connection.sessionRunning)
    }

    @Test
    fun theLastSessionsLateStopReportCannotEndTheNextSession() {
        connected()
        viewModel.disconnect()
        // The musician connects again at once. The receiver counts as alive until the stopped
        // loop's report has been handled, so this Connect is refused rather than start a session
        // the late report could reach.
        viewModel.connect(SERVER)
        assertEquals(1, transport.sockets.size)

        receiver.finishStopping()
        viewModel.connect(SERVER)
        val next = socket()

        assertEquals(2, transport.sockets.size)
        assertNull(next.closedWith)
        assertTrue(receiver.running.get())
        assertTrue(state.connection.sessionRunning)
        assertNull(state.connection.problem)
        assertEquals(2, scanner.starts)

        // The next session ends only when its own receive loop does.
        receiver.endOnItsOwn()

        assertEquals(1000 to "receiver stopped", next.closedWith)
        assertFalse(state.connection.sessionRunning)
        assertEquals(3, scanner.starts)
    }

    @Test
    fun aLoopEndingWhileTheMusicianStopsAndReconnectsEndsOnlyItsOwnSession() {
        connected()
        val first = socket()

        // The loop ends on its own: as the latest launch it drops the flag, and before its report
        // takes the session's lock the musician taps Stop and Connect.
        receiver.endOnItsOwn(beforeReport = {
            viewModel.disconnect()
            viewModel.connect(SERVER)
        })

        // Stop found the flag already down and did nothing; Connect was refused while the loop
        // was still reporting; the report then ended the session it belongs to, once.
        assertEquals(0, receiver.stops)
        assertEquals("The previous receiver is still stopping", state.connection.problem)
        assertEquals(1, transport.sockets.size)
        assertEquals(1000 to "receiver stopped", first.closedWith)
        assertFalse(state.connection.sessionRunning)
        assertFalse(receiver.isAlive())
        assertEquals(2, scanner.starts)

        // The second session's report is counted as the second: after Stop it ends nothing again.
        viewModel.connect(SERVER)
        val second = socket()
        viewModel.disconnect()
        receiver.finishStopping()

        assertEquals(1000 to "stopped", second.closedWith)
        assertFalse(receiver.isAlive())
        assertEquals(3, scanner.starts)
    }

    @Test
    fun aStopReportArrivingAfterClearingTouchesNothing() {
        val owner = ViewModelStore()
        val model = ownedBy(owner)
        model.connect(SERVER)
        socket().open()
        owner.clear()
        val cleared = model.state.value

        receiver.finishStopping()

        assertEquals(2, scanner.starts)
        assertEquals(1000 to "stopped", socket().closedWith)
        assertEquals(cleared, model.state.value)
    }

    @Test
    fun aReceiverThatEndedOnItsOwnBeforeClearingAndReportsAfterTouchesNothing() {
        val owner = ViewModelStore()
        val model = ownedBy(owner)
        model.connect(SERVER)
        socket().open()
        lateinit var cleared: MonitorUiState

        // The receive loop ended and dropped the flag; the Activity finished before its report.
        receiver.endOnItsOwn(beforeReport = {
            owner.clear()
            cleared = model.state.value
        })

        assertEquals(1, scanner.starts)
        assertEquals(1000 to "destroyed", socket().closedWith)
        assertEquals(cleared, model.state.value)
    }

    @Test
    fun aManualAddressIsCheckedBeforeAnythingStarts() {
        viewModel.connectManual(host = "  ", controlPort = "50001", audioPort = "50000")
        assertEquals("Enter the server IP address or host name", state.connection.problem)

        viewModel.connectManual(host = "10.0.0.5", controlPort = "50001", audioPort = "0")
        assertEquals("Enter a valid UDP port between 1 and 65535", state.connection.problem)

        viewModel.connectManual(host = "10.0.0.5", controlPort = "x", audioPort = "50000")
        assertEquals("Enter a valid WebSocket control port between 1 and 65535", state.connection.problem)
        assertEquals(ManualAddress("10.0.0.5", "x", "50000"), state.connection.manual)

        assertTrue(transport.sockets.isEmpty())
        assertTrue(receiver.launches.isEmpty())
        assertFalse(state.connection.sessionRunning)

        viewModel.connectManual(host = " 10.0.0.5 ", controlPort = "50101", audioPort = "50100")

        assertEquals(listOf("ws://10.0.0.5:50101"), transport.sockets.map { it.url })
        assertEquals(listOf("10.0.0.5" to 50100), receiver.launches)
        assertNull(state.connection.problem)
    }

    @Test
    fun aFoundServerUsesTheAudioPortFromSettings() {
        viewModel.editManualAddress(ManualAddress(host = "", controlPort = "50001", audioPort = "50200"))

        viewModel.connect(SERVER)

        assertEquals(listOf("192.168.1.20" to 50200), receiver.launches)
    }

    @Test
    fun aSecondConnectWhileRunningIsIgnored() {
        viewModel.connect(SERVER)

        viewModel.connect(ServerEndpoint("10.0.0.9", 50001))

        assertEquals(1, transport.sockets.size)
        assertEquals(1, receiver.launches.size)
    }

    @Test
    fun connectWaitsForThePreviousReceiverToEnd() {
        connected()
        viewModel.disconnect()

        // The stopped loop has not reported yet, so its receiver still counts as alive.
        viewModel.connect(SERVER)

        assertEquals("The previous receiver is still stopping", state.connection.problem)
        assertEquals(1, transport.sockets.size)
        assertEquals(1, receiver.launches.size)
        assertFalse(state.connection.sessionRunning)
    }

    @Test
    fun channelChangesAreSentAsAMix() {
        connected(channels = 2)

        viewModel.setChannelLevel(1, 40)
        viewModel.setChannelMuted(0, true)
        viewModel.setChannelSoloed(1, true)
        viewModel.setChannelPan(0, 50)

        assertEquals(listOf(100, 40), lastMix().ints("channels"))
        assertEquals(listOf(true, false), lastMix().getJSONArray("mutes").booleans())
        assertEquals(listOf(false, true), lastMix().getJSONArray("solos").booleans())
        assertEquals(listOf(50, 100), lastMix().ints("pans"))
        assertEquals(listOf(100, 40), state.mix.levels)
    }

    @Test
    fun aPanFromTheScreensIsKeptAndSentAsAMix() {
        val actions: MonitorActions = connected(channels = 2)

        actions.setChannelPan(1, 35)

        assertEquals(listOf(0, 35), state.mix.pans)
        assertEquals(listOf(0, 35), lastMix().ints("pans"))
    }

    @Test
    fun aChangeThatChangesNothingSendsNothing() {
        connected(channels = 2)
        val sent = socket().sentOfType("mix").size

        viewModel.setChannelLevel(0, 100)
        viewModel.setChannelLevel(7, 20)

        assertEquals(sent, socket().sentOfType("mix").size)
    }

    @Test
    fun groupLevelsAreSentAsAMix() {
        viewModel.connect(SERVER)
        socket().open()
        socket().receive("""{"type":"config","source_channels":2,"groups":[{"name":"Band","channels":[0,1]}]}""")

        viewModel.setGroupLevel(0, 65)

        assertEquals(listOf(65), lastMix().ints("group_levels"))
        assertEquals("Band", state.mix.groups.single().name)
    }

    /** The server announces a keys pair on channels 0 and 1 and a voice on channel 2, as a `config`. */
    private fun announcePair() {
        socket().receive(
            """{"type":"config","source_channels":3,"groups":[],"channels":[
                {"name":"Keys","kind":"keys","hidden":false,"link":true},
                {"name":"","kind":"keys","hidden":false,"link":false},
                {"name":"Vox","kind":"voice","hidden":false,"link":false}]}""",
        )
    }

    @Test
    fun aPairsStripSendsBothSidesAndMoreOfMeLiftsBoth() {
        connected(channels = 3)
        announcePair()

        viewModel.setChannelLevel(0, 60)
        viewModel.setChannelPan(0, 75)

        assertEquals(listOf("Keys", "Vox"), state.mix.channels.map { it.label })
        assertEquals(listOf(60, 60, 100), lastMix().ints("channels"))
        assertEquals(listOf(50, 100, 0), lastMix().ints("pans"))

        viewModel.setMusicianChannel(0)
        viewModel.setMoreOfMe(true)

        assertEquals(listOf(100, 100, 50), lastMix().ints("channels"))
    }

    @Test
    fun aConfigThatPairsTheMusiciansChannelRemembersThePairAndSendsNothing() {
        connected(channels = 3)
        viewModel.setMusicianChannel(1)
        val sent = socket().sentOfType("mix").size

        announcePair()

        assertEquals(0, state.moreOfMe.musicianChannel)
        assertEquals(0, store.channel)
        // The server panned the new pair apart; its `mix_state` follows, so the phone's mix stays put.
        assertEquals(sent, socket().sentOfType("mix").size)
    }

    /** A `mix_state` of neutral levels carrying the engineer's [limit] and [locked] channels. */
    private fun engineerState(limit: Int, vararg locked: Int): String =
        JSONObject(mixState(levels = emptyList()))
            .put("engineer_limit_percent", limit)
            .put("locked", JSONArray(List(32) { it in locked }))
            .toString()

    @Test
    fun aConfigThatPairsTheMusiciansChannelWithMoreOfMeOnDropsTheLiftOnceTheLinksMixArrives() {
        connected(channels = 3)
        viewModel.setChannelLevel(0, 80)
        viewModel.setChannelLevel(1, 60)
        viewModel.setChannelLevel(2, 40)
        viewModel.setChannelPan(0, 30)
        viewModel.setChannelPan(1, 70)
        viewModel.setChannelPan(2, 50)
        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)
        assertEquals(listOf(40, 100, 20), lastMix().ints("channels"))
        answerMixes()
        val sent = socket().sentOfType("mix").size

        announcePair()

        assertEquals(0, state.moreOfMe.musicianChannel)
        assertEquals(0, store.channel)
        assertFalse(state.moreOfMe.enabled)
        assertEquals(MoreOfMeNotice.CHANNEL_PAIRED, state.moreOfMe.notice)
        // The link's own `mix_state` follows with the pans it moved; a send now would put the phone's
        // old pans back over them.
        assertEquals(sent, socket().sentOfType("mix").size)

        // That push still carries the lift (the second side took the first's lifted level) and the
        // pans the link moved apart.
        socket().receive(
            JSONObject(mixState(levels = listOf(40, 40, 20)))
                .put("pans", JSONArray(List(32) { if (it == 1) 100 else if (it == 2) 50 else 0 }))
                .toString(),
        )

        // One whole mix goes out: the musician's own levels (the pair's at its first side's) with the
        // moved pans, and the phone keeps the same.
        assertEquals(sent + 1, socket().sentOfType("mix").size)
        assertEquals(listOf(80, 80, 40), lastMix().ints("channels"))
        assertEquals(listOf(0, 100, 50), lastMix().ints("pans"))
        assertEquals(listOf(80, 80, 40), state.mix.levels)
        assertEquals(listOf(0, 100, 50), state.mix.pans)
        assertFalse(state.moreOfMe.enabled)

        answerMixes()
        viewModel.setMoreOfMe(true)

        assertEquals(listOf(100, 100, 20), lastMix().ints("channels"))
    }

    @Test
    fun theEngineersLimitHoldsThePhonesOwnPlaybackAndIsNeverSent() {
        connected(channels = 3)
        answerMixes()

        socket().receive(engineerState(60))
        viewModel.setLevelCap(90)

        assertEquals(60, state.mix.effectiveCap)
        assertEquals(60, receiver.mixState.maxLevelPercent)
        assertEquals(90, lastMix().getInt("max_level_percent"))
        assertFalse(lastMix().has("engineer_limit_percent"))
        assertFalse(lastMix().has("locked"))
    }

    @Test
    fun aLockedChannelsEditSendsNothingAndMoreOfMeLeavesIt() {
        connected(channels = 3)
        answerMixes()
        socket().receive(engineerState(100, 0))
        val sent = socket().sentOfType("mix").size

        viewModel.setChannelLevel(0, 40)
        viewModel.setChannelMuted(0, true)

        assertEquals(sent, socket().sentOfType("mix").size)

        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)

        assertEquals(listOf(100, 100, 50), lastMix().ints("channels"))
    }

    @Test
    fun aLimitOnlyReportMovesThePhonesOwnCeilingWithNoMasterEdit() {
        connected(channels = 3)
        answerMixes()
        assertEquals(100, receiver.mixState.maxLevelPercent)

        socket().receive(engineerState(60))

        assertEquals(60, receiver.mixState.maxLevelPercent)

        // An answer that only lowers the limit moves it too; the change it answers kept the cap.
        viewModel.setChannelLevel(0, 40)
        val answer = socket().sentOfType("mix").last()
            .put("type", "mix_ack")
            .put("engineer_limit_percent", 45)
            .put("locked", JSONArray(List(32) { false }))
        socket().receive(answer.toString())

        assertEquals(45, receiver.mixState.maxLevelPercent)
        assertEquals(100, state.mix.levelCap)
    }

    @Test
    fun moreOfMeLeavesBothSidesOfAHalfLockedPairInWhatItSends() {
        connected(channels = 3)
        announcePair()
        answerMixes()
        socket().receive(engineerState(100, 1))

        viewModel.setMusicianChannel(2)
        viewModel.setMoreOfMe(true)

        assertEquals(listOf(100, 100, 100), lastMix().ints("channels"))
    }

    /** A `mix_state` with every channel at [level] that names whose mix the phone follows. */
    private fun followingState(name: String?, level: Int = 100): String =
        JSONObject(mixState(levels = List(32) { level })).put("following", name ?: JSONObject.NULL).toString()

    @Test
    fun aPushedMixSaysWhoseMixThePhoneFollowsAndWhenItStops() {
        connected(channels = 3)
        answerMixes()

        socket().receive(followingState("Ana", level = 70))

        assertEquals("Ana", state.mix.following)
        assertEquals(listOf(70, 70, 70), state.mix.levels)

        socket().receive(followingState(null, level = 70))

        assertNull(state.mix.following)
        assertEquals(listOf(70, 70, 70), state.mix.levels)
    }

    @Test
    fun whileFollowingOnlyTheMasterMovesAndTheAnswerBringsTheLeadersChannels() {
        connected(channels = 3)
        answerMixes()
        socket().receive(followingState("Ana", level = 70))
        val sent = socket().sentOfType("mix").size

        viewModel.setChannelLevel(0, 40)
        viewModel.setChannelMuted(1, true)

        assertEquals(sent, socket().sentOfType("mix").size)

        viewModel.setMasterVolume(50)

        assertEquals(listOf(70, 70, 70), lastMix().ints("channels"))
        assertEquals(50, lastMix().getInt("volume_percent"))

        // The server keeps the leader's channels (moved on meanwhile) and the answer carries them.
        val answer = JSONObject(followingState("Ana", level = 65))
            .put("type", "mix_ack")
            .put("seq", lastMix().getLong("seq"))
            .put("volume_percent", 50)
        socket().receive(answer.toString())

        assertEquals(listOf(65, 65, 65), state.mix.levels)
        assertEquals(50, state.mix.masterVolume)
    }

    @Test
    fun recallingWhileFollowingSendsNothingAndSaysWhy() {
        store.banks += MixBank("Verse", MixSnapshot(channelGains = intArrayOf(10, 20, 30)))
        connected(channels = 3)
        answerMixes()
        socket().receive(followingState("Ana", level = 70))
        val sent = socket().sentOfType("mix").size

        viewModel.recallMix("Verse")

        assertEquals(sent, socket().sentOfType("mix").size)
        assertEquals("\"Verse\" was not recalled: your mix follows Ana's", state.savedMixes.notice)
        assertEquals(listOf(70, 70, 70), state.mix.levels)
    }

    /**
     * The server's answer to the phone's [sent] mix when it kept its own channels and groups: every
     * channel from [levels] on and the first group at [groupLevel].
     */
    private fun answerKeepingTheServersMix(sent: JSONObject, levels: List<Int>, groupLevel: Int): String =
        JSONObject(mixState(levels = levels, type = "mix_ack", seq = sent.getLong("seq")))
            .put("group_levels", JSONArray(List(16) { if (it == 0) groupLevel else 100 }))
            .toString()

    private val bandConfig = """{"type":"config","source_channels":3,"groups":[{"name":"Band","channels":[0,1]}]}"""

    @Test
    fun aConfigArrivingBeforeTheOpeningMixIsSentLeavesTheServersMixAloneAndThePhoneTakesIt() {
        scheduler.holding = true
        viewModel.connect(SERVER)
        scheduler.runHeld()
        socket().open()
        // The server's `config` follows the open at once; here it lands before the control thread
        // gets to the opening mix.
        socket().receive(bandConfig)
        scheduler.runHeld()
        scheduler.holding = false

        val opening = socket().sentOfType("mix").single()
        assertEquals(emptyList<Int>(), opening.ints("channels"))
        assertEquals(emptyList<Int>(), opening.ints("pans"))
        assertEquals(0, opening.getJSONArray("mutes").length())
        assertEquals(0, opening.getJSONArray("solos").length())
        assertEquals(emptyList<Int>(), opening.ints("group_levels"))

        socket().receive(answerKeepingTheServersMix(opening, listOf(40, 50, 60), groupLevel = 70))

        assertEquals(listOf(40, 50, 60), state.mix.levels)
        assertEquals(listOf(70), state.mix.groupLevels)
        assertEquals(1, socket().sentOfType("mix").size)
    }

    @Test
    fun onTheFirstConnectThePhoneShowsTheMixTheServerAlreadyHolds() {
        viewModel.connect(SERVER)
        socket().open()
        socket().receive(bandConfig)

        val opening = socket().sentOfType("mix").single()
        assertEquals(emptyList<Int>(), opening.ints("channels"))

        socket().receive(answerKeepingTheServersMix(opening, listOf(40, 50, 60), groupLevel = 70))

        assertEquals(listOf(40, 50, 60), state.mix.levels)
        assertEquals(listOf(70), state.mix.groupLevels)
        assertEquals(1, socket().sentOfType("mix").size)
    }

    @Test
    fun aReconnectStillSendsThePhonesOwnMix() {
        connected(channels = 3)
        viewModel.setChannelLevel(1, 30)
        answerMixes()

        socket().fail("Wi-Fi dropped")
        scheduler.runDelayed()
        socket().open()

        assertEquals(listOf(100, 30, 100), socket().sentOfType("mix").single().ints("channels"))
    }

    @Test
    fun masterChangesReachTheAudioThreadAndTheServer() {
        connected()

        viewModel.setMasterVolume(40)
        viewModel.setLevelCap(80)
        viewModel.setMasterMuted(true)

        assertEquals(40, receiver.mixState.volumePercent)
        assertEquals(80, receiver.mixState.maxLevelPercent)
        assertTrue(receiver.mixState.muted)
        assertEquals(40, lastMix().getInt("volume_percent"))
        assertEquals(80, lastMix().getInt("max_level_percent"))
        assertTrue(lastMix().getBoolean("muted"))
    }

    @Test
    fun moreOfMeLiftsTheMusiciansChannelOnlyInWhatIsSent() {
        connected(channels = 2)
        viewModel.setChannelLevel(0, 80)
        val sent = socket().sentOfType("mix").size

        viewModel.setMoreOfMe(true)

        assertEquals(MoreOfMeNotice.NEEDS_CHANNEL, state.moreOfMe.notice)
        assertEquals(sent, socket().sentOfType("mix").size)

        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)

        assertEquals(1, store.channel)
        assertEquals(listOf(40, 100), lastMix().ints("channels"))
        assertEquals(listOf(80, 100), state.mix.levels)

        viewModel.setMoreOfMe(false)

        assertEquals(listOf(80, 100), lastMix().ints("channels"))
    }

    @Test
    fun choosingNoChannelTurnsMoreOfMeOffAndResendsTheMix() {
        connected(channels = 2)
        viewModel.setMusicianChannel(0)
        viewModel.setMoreOfMe(true)
        assertEquals(listOf(100, 50), lastMix().ints("channels"))

        viewModel.setMusicianChannel(null)

        assertNull(store.channel)
        assertFalse(state.moreOfMe.enabled)
        assertEquals(listOf(100, 100), lastMix().ints("channels"))
    }

    @Test
    fun aChannelTheServerDidNotAnnounceIsNotStored() {
        connected(channels = 2)

        viewModel.setMusicianChannel(5)

        assertNull(store.channel)
        assertNull(state.moreOfMe.musicianChannel)
    }

    @Test
    fun aConfigWithoutTheRememberedChannelForgetsIt() {
        store.channel = 3

        connected(channels = 2)

        assertNull(state.moreOfMe.musicianChannel)
        assertNull(store.channel)
    }

    // A mix the server pushed.

    @Test
    fun aPushedMixIsShownAndHeardWithoutBeingSentBack() {
        connected(channels = 2)
        answerMixes()
        val sent = socket().sent.size

        socket().receive(mixState(levels = listOf(80, 70), volume = 50, cap = 90, muted = true))

        assertEquals(listOf(80, 70), state.mix.levels)
        assertEquals(50, state.mix.masterVolume)
        assertEquals(50, receiver.mixState.volumePercent)
        assertEquals(90, receiver.mixState.maxLevelPercent)
        assertTrue(receiver.mixState.muted)
        assertEquals(sent, socket().sent.size)
    }

    @Test
    fun aPushDuringMoreOfMeTurnsItOffWithoutSendingAndTheNextMoveSendsThePushedMix() {
        connected(channels = 2)
        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)
        answerMixes()
        val sent = socket().sent.size

        socket().receive(mixState(levels = listOf(80, 70)))

        assertFalse(state.moreOfMe.enabled)
        assertEquals(MoreOfMeNotice.MIX_CHANGED_ELSEWHERE, state.moreOfMe.notice)
        assertEquals(1, store.channel)
        assertEquals(sent, socket().sent.size)

        // A fader still moving when the push landed sends its own value with the pushed mix around it;
        // the server keeps whichever whole mix it got last.
        viewModel.setChannelLevel(1, 30)

        assertEquals(listOf(80, 30), lastMix().ints("channels"))
    }

    @Test
    fun theServerSendingBackTheMixThePhoneSentChangesNothing() {
        connected(channels = 2)
        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)
        answerMixes()
        val before = state
        val sent = socket().sent.size

        // After missing events the server re-sends the whole mix it holds, which is the phone's own.
        socket().receive(lastMix().put("type", "mix_state").toString())

        assertEquals(before, state)
        assertEquals(sent, socket().sent.size)
    }

    @Test
    fun aPushedMixIsTheCurrentMixAndLeavesTheSavedMixesAlone() {
        connected(channels = 2)
        viewModel.setChannelLevel(0, 30)
        viewModel.saveMix("Verse")
        answerMixes()

        socket().receive(mixState(levels = listOf(80, 70)))

        assertEquals(listOf(30, 100), store.banks.single().snapshot.channelGains.toList())

        viewModel.saveMix("Chorus")

        assertEquals(listOf(80, 70), store.banks.single { it.name == "Chorus" }.snapshot.channelGains.toList())
    }

    // The server's answers to the phone's mixes.

    @Test
    fun aPushOlderThanAMixStillInFlightGivesWayToTheAnswerToThatMix() {
        connected(channels = 2)
        answerMixes()
        viewModel.setChannelLevel(0, 40)
        val sent = socket().sent.size

        // The server pushed its mix before the phone's newer one reached it; the gesture has ended.
        socket().receive(mixState(levels = listOf(80, 70)))

        assertEquals(listOf(40, 100), state.mix.levels)

        answerMixes()

        assertEquals(listOf(40, 100), state.mix.levels)
        assertEquals(sent, socket().sent.size)
    }

    @Test
    fun withTwoMixesInFlightOnlyTheAnswerToTheNewestIsTaken() {
        connected(channels = 2)
        answerMixes()
        viewModel.setChannelLevel(0, 40)
        viewModel.setChannelLevel(0, 50)
        val (first, second) = socket().sentOfType("mix").takeLast(2).map { it.getLong("seq") }
        val sent = socket().sent.size

        socket().receive(mixState(levels = listOf(40, 100), type = "mix_ack", seq = first))

        assertEquals(listOf(50, 100), state.mix.levels)

        // What the server answers to the newest mix is what it plays, so the phone takes it.
        socket().receive(mixState(levels = listOf(50, 60), volume = 70, type = "mix_ack", seq = second))

        assertEquals(listOf(50, 60), state.mix.levels)
        assertEquals(70, state.mix.masterVolume)
        assertEquals(70, receiver.mixState.volumePercent)
        assertEquals(sent, socket().sent.size)
    }

    @Test
    fun anAnswerThatIsTheMixThePhonePlaysChangesNothing() {
        connected(channels = 2)
        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)
        val before = state
        val sent = socket().sent.size

        // The answers carry the mixes as sent, the last one lifted by More of me.
        answerMixes()

        assertEquals(before.mix, state.mix)
        assertEquals(before.moreOfMe, state.moreOfMe)
        assertTrue(state.moreOfMe.enabled)
        assertEquals(sent, socket().sent.size)
    }

    @Test
    fun anAnswerArrivingBeforeAQueuedChangeIsSentNeitherUndoesNorLosesIt() {
        connected(channels = 2)
        viewModel.setMusicianChannel(1)
        viewModel.setMoreOfMe(true)
        answerMixes()
        scheduler.holding = true
        viewModel.setChannelLevel(0, 40)
        scheduler.runHeld()

        // The musician moves on while the control thread is busy, so that send waits; meanwhile the
        // answer to the mix already sent arrives.
        viewModel.setChannelLevel(0, 50)
        answerMixes()

        assertEquals(listOf(50, 100), state.mix.levels)
        assertTrue(state.moreOfMe.enabled)

        scheduler.runHeld()

        assertEquals(listOf(25, 100), lastMix().ints("channels"))

        answerMixes()

        assertEquals(listOf(50, 100), state.mix.levels)
        assertTrue(state.moreOfMe.enabled)
    }

    @Test
    fun anErrorWithoutANumberNeverMakesAnOlderAnswerTheNewest() {
        connected(channels = 2)
        answerMixes()
        viewModel.setChannelLevel(0, 40)
        viewModel.setChannelLevel(0, 50)
        val (first, second) = socket().sentOfType("mix").takeLast(2)

        // An error answering a stats report, then the answer to the first mix, then a push: the second
        // mix is still on its way, so none of them is the server's latest word.
        socket().receive("""{"type":"error","message":"no UDP target configured for client IP"}""")
        socket().receive(first.put("type", "mix_ack").toString())
        socket().receive(mixState(levels = listOf(80, 70)))

        assertEquals(listOf(50, 100), state.mix.levels)

        socket().receive(mixState(levels = listOf(50, 60), type = "mix_ack", seq = second.getLong("seq")))

        assertEquals(listOf(50, 60), state.mix.levels)
    }

    @Test
    fun anAckWithoutANumberOnlySaysTheServerHoldsTheMix() {
        connected(channels = 2)
        answerMixes()

        socket().receive(mixState(levels = listOf(80, 70), type = "mix_ack"))

        assertEquals(listOf(100, 100), state.mix.levels)
        assertTrue(state.connection.mixHeldByServer)
    }

    @Test
    fun aReconnectSendsTheMixAgainUnderItsNewestNumberAndItsAnswerSettlesIt() {
        connected(channels = 2)
        answerMixes()
        viewModel.setChannelLevel(0, 40)
        val newest = lastMix().getLong("seq")
        socket().fail("wifi gone")
        scheduler.runDelayed()

        socket().open()

        assertEquals(newest, lastMix().getLong("seq"))
        assertEquals(listOf(40, 100), lastMix().ints("channels"))

        // Pushed before the server answered the mix sent again: that answer is the latest word.
        socket().receive(mixState(levels = listOf(80, 70)))

        assertEquals(listOf(40, 100), state.mix.levels)

        answerMixes()
        socket().receive(mixState(levels = listOf(80, 70)))

        assertEquals(listOf(80, 70), state.mix.levels)
    }

    @Test
    fun savingAndRecallingAMix() {
        connected(channels = 2)
        viewModel.setChannelLevel(0, 30)

        assertTrue(viewModel.saveMix(" Verse "))

        assertEquals(listOf("Verse"), store.banks.map { it.name })
        assertEquals(listOf(SavedMixUiState("Verse", true)), state.savedMixes.mixes)

        viewModel.setChannelLevel(0, 90)
        viewModel.setMasterVolume(50)
        viewModel.recallMix("Verse")

        assertEquals(listOf(30, 100), state.mix.levels)
        assertEquals(listOf(30, 100), lastMix().ints("channels"))
        assertEquals(100, receiver.mixState.volumePercent)
        assertEquals("Recalled \"Verse\"", state.savedMixes.notice)
    }

    @Test
    fun savingUnderAnExistingNameReplacesThatMix() {
        connected(channels = 2)
        viewModel.setChannelLevel(0, 30)
        viewModel.saveMix("Verse")
        viewModel.setChannelLevel(0, 70)

        // As on the old screen, saving under a name already used updates that mix; it is not refused.
        assertTrue(viewModel.saveMix("Verse"))

        assertEquals(listOf("Verse"), store.banks.map { it.name })
        assertEquals(listOf(70, 100), store.banks.single().snapshot.channelGains.toList())
    }

    @Test
    fun savingNeedsAName() {
        assertFalse(viewModel.saveMix("   "))

        assertTrue(store.banks.isEmpty())
        assertEquals("Enter a name for the mix", state.savedMixes.notice)
        assertEquals("No saved mixes yet", state.savedMixes.emptyMessage)
    }

    @Test
    fun recallingAMixThatIsGoneRefreshesTheList() {
        store.banks += MixBank("Verse", MixSnapshot())
        started()
        store.banks.clear()

        viewModel.recallMix("Verse")

        assertEquals("\"Verse\" is no longer saved", state.savedMixes.notice)
        assertEquals("No saved mixes yet", state.savedMixes.emptyMessage)
    }

    @Test
    fun renamingAndDeletingAMix() {
        store.banks += MixBank("Verse", MixSnapshot(volumePercent = 70))
        store.banks += MixBank("Chorus", MixSnapshot())

        viewModel.renameMix("Verse", "Chorus")

        assertEquals("A mix named \"Chorus\" already exists", state.savedMixes.notice)
        assertEquals(listOf("Verse", "Chorus"), store.banks.map { it.name })

        viewModel.renameMix("Verse", " Bridge ")

        assertEquals(listOf("Chorus", "Bridge"), state.savedMixes.names)
        assertEquals(70, store.banks.single { it.name == "Bridge" }.snapshot.volumePercent)

        viewModel.deleteMix("Chorus")

        assertEquals(listOf("Bridge"), store.banks.map { it.name })
        assertEquals(listOf("Bridge"), state.savedMixes.names)
    }

    @Test
    fun aNameChangeIsStoredAndAnnouncedAfterThePause() {
        connected()

        viewModel.setName("Bea ")

        assertEquals("Bea ", state.musicianName)
        assertEquals("Bea", store.name)
        assertEquals(1, socket().sentOfType("register").size)
        assertEquals(listOf(400L), scheduler.pendingDelays())

        scheduler.runDelayed()

        assertEquals("Bea", socket().sentOfType("register").last().getString("name"))
    }

    @Test
    fun receiverReportsReachTheDiagnosticsAndTheServer() {
        connected()

        receiver.listener.onStats(500, 5, 48_000, 100.0, 10.0, 0.3, 0, 0, 15.0)
        receiver.listener.onLinkStats(LinkStatsReport(1_000, 100, 0, 0, 0))
        receiver.listener.onError("invalid PMON packet")
        socket().receive("""{"type":"error","message":"mix refused"}""")

        assertEquals(500L, state.diagnostics.packetsReceived)
        assertEquals(15.0, state.diagnostics.playbackBufferMs, 0.0)
        assertEquals(LinkHealth.GOOD, state.diagnostics.linkHealth)
        assertEquals(1, socket().sentOfType("stats").size)
        assertEquals("invalid PMON packet", state.diagnostics.lastError)
        assertEquals("mix refused", state.diagnostics.controlError)
    }

    @Test
    fun selectingATabShowsIt() {
        viewModel.selectTab(MainTab.SETTINGS)

        assertEquals(MainTab.SETTINGS, state.tab)
    }

    @Test
    fun clearingTheViewModelStopsEverything() {
        val owner = ViewModelStore()
        val model = ownedBy(owner)
        model.connect(SERVER)
        socket().open()

        owner.clear()

        assertEquals(1, receiver.stops)
        assertFalse(receiver.running.get())
        assertEquals(1000 to "stopped", socket().closedWith)
        assertTrue(scheduler.shutDown)
        assertTrue(transport.shutDown)
        assertEquals(1, scanner.shutdowns)
    }

    private companion object {
        const val CLIENT_ID = "6f1c2a7e-0d4b-4c55-9a3e-2b8f6c1d9e70"
        val SERVER = ServerEndpoint("192.168.1.20", 50001)
    }
}
