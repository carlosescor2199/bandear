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
        started()
        receiver.alive = true

        viewModel.connect(SERVER)

        assertEquals("The previous receiver is still stopping", state.connection.problem)
        assertTrue(transport.sockets.isEmpty())
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

    @Test
    fun savingAndRecallingAMix() {
        connected(channels = 2)
        viewModel.setChannelLevel(0, 30)

        viewModel.saveMix(" Verse ")

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
    fun savingNeedsAName() {
        viewModel.saveMix("   ")

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
        val factory = object : ViewModelProvider.Factory {
            @Suppress("UNCHECKED_CAST")
            override fun <T : ViewModel> create(modelClass: Class<T>): T = newViewModel() as T
        }
        val model = ViewModelProvider(owner, factory)[MonitorViewModel::class.java]
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
