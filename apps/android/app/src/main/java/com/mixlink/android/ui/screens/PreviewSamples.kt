package com.mixlink.android.ui.screens

import com.mixlink.android.ChannelInfo
import com.mixlink.android.ChannelKind
import com.mixlink.android.EngineerControls
import com.mixlink.android.GroupInfo
import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.components.PillKind
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.DiagnosticsUiState
import com.mixlink.android.ui.state.ManualAddress
import com.mixlink.android.ui.state.MixUiState
import com.mixlink.android.ui.state.MonitorUiState
import com.mixlink.android.ui.state.MoreOfMeNotice
import com.mixlink.android.ui.state.MoreOfMeUiState
import com.mixlink.android.ui.state.SavedMixesUiState
import com.mixlink.android.ui.state.ServerEndpoint

/** The phone the artboards were drawn for, in dp; previews use it so they read like the mockup. */
internal const val PREVIEW_WIDTH_DP = 390
internal const val PREVIEW_HEIGHT_DP = 844

/** The page colour, for previews' backgrounds. */
internal const val PREVIEW_BACKGROUND = 0xFF0D1117

/** Sample states for the screens' previews: one musician, one server, a full mix. */
internal object PreviewSamples {
    val server = ServerEndpoint("192.168.1.34", 50001)

    private val mix = MixUiState(
        sourceChannels = 8,
        levels = listOf(72, 64, 88, 55, 60, 70, 45, 80),
        pans = listOf(35, 60, 50, 0, 100, 50, 20, 80),
        mutes = listOf(false, false, false, true, false, false, false, false),
        solos = listOf(false, false, false, false, false, true, false, false),
        groupDefinitions = listOf(
            GroupInfo("Drums", listOf(0, 1)),
            GroupInfo("Vocals", listOf(2, 7)),
            GroupInfo("Keys", listOf(3, 4)),
            GroupInfo("Guitar", listOf(5, 6)),
        ),
        groupLevels = listOf(80, 92, 70, 65),
        masterVolume = 85,
        levelCap = 100,
        // Named inputs, a keys pair on CH 4 and CH 5 shown as one strip, and a spare input hidden.
        channelDetails = listOf(
            ChannelInfo("Kick", ChannelKind.DRUMS),
            ChannelInfo("Snare", ChannelKind.DRUMS),
            ChannelInfo("Vox", ChannelKind.VOICE),
            ChannelInfo("Keys", ChannelKind.KEYS, link = true),
            ChannelInfo("", ChannelKind.KEYS),
            ChannelInfo("Gtr", ChannelKind.GUITAR),
            ChannelInfo("Bass", ChannelKind.BASS),
            ChannelInfo("Spare", hidden = true),
        ),
    )

    private val healthy = DiagnosticsUiState(
        packetsReceived = 182_340,
        packetsLost = 12,
        packetsPerSecond = 100.0,
        interArrivalMs = 10.0,
        jitterMs = 1.4,
        playbackBufferMs = 34.0,
        sampleRateHz = 48_000,
        linkHealth = LinkHealth.GOOD,
    )

    /** On stage: connected, the server holds the mix, More of me lifts Vox (CH 3). */
    val connected = MonitorUiState(
        musicianName = "Carlos D",
        connection = ConnectionUiState(
            pill = PillKind.CONNECTED,
            sessionRunning = true,
            server = server,
            mixHeldByServer = true,
            statusDetail = "connected - mix applied on the server",
            manual = ManualAddress(host = server.address),
        ),
        servers = listOf(server),
        mix = mix,
        moreOfMe = MoreOfMeUiState(enabled = true, musicianChannel = 2),
        savedMixes = SavedMixesUiState(
            names = listOf("Soundcheck", "Main set", "Acoustic"),
            activeName = "Soundcheck",
            notice = "Recalled \"Soundcheck\"",
        ),
        diagnostics = healthy,
    )

    val moreOfMeOff = connected.copy(moreOfMe = MoreOfMeUiState(enabled = false, musicianChannel = 2))

    /** A server that announces no channel details: every input by number, no icons, nothing joined. */
    val olderServer = connected.copy(mix = mix.copy(channelDetails = null))

    /** The engineer locked the drums and limits this musician to 60%, under the phone's cap of 80%. */
    val engineerControls = moreOfMeOff.copy(
        mix = mix.copy(levelCap = 80, engineer = EngineerControls(limitPercent = 60, lockedChannels = setOf(0, 1))),
    )

    /** More of me asked for before a channel was chosen. */
    val noChannelChosen = connected.copy(
        moreOfMe = MoreOfMeUiState(enabled = false, musicianChannel = null, notice = MoreOfMeNotice.NEEDS_CHANNEL),
    )

    /** The Wi-Fi dropped: the control channel retries while the server holds the mix. */
    val reconnecting = connected.copy(
        connection = connected.connection.copy(
            pill = PillKind.RECONNECTING,
            statusDetail = "reconnecting - mix held at the server",
        ),
        diagnostics = healthy.copy(linkHealth = LinkHealth.WARNING, underruns = 3),
    )

    /** The control channel gave up; this phone still mixes on its own. */
    val lostServer = connected.copy(
        connection = connected.connection.copy(
            pill = PillKind.NOT_CONNECTED,
            mixHeldByServer = false,
            statusDetail = "unavailable - mixing on this device",
        ),
        diagnostics = healthy.copy(linkHealth = LinkHealth.LOST, controlError = "Connection refused"),
    )

    /** Just connected: the server has not announced its channels yet. */
    val waitingForChannels = connected.copy(
        connection = connected.connection.copy(pill = PillKind.CONNECTING, mixHeldByServer = false),
        mix = MixUiState(),
        moreOfMe = MoreOfMeUiState(),
        diagnostics = DiagnosticsUiState(),
    )

    val emptyMixes = connected.copy(savedMixes = SavedMixesUiState(), tab = MainTab.MIXES)

    /** No session: one server found on the network. */
    val notConnected = MonitorUiState(
        musicianName = "Carlos D",
        servers = listOf(server),
        savedMixes = connected.savedMixes.copy(notice = null),
    )

    /** No session and nothing found yet; the name has not been typed either. */
    val searching = MonitorUiState()

    /** A manual Connect that was refused. */
    val refused = notConnected.copy(
        connection = ConnectionUiState(
            problem = "Enter a valid UDP port between 1 and 65535",
            manual = ManualAddress(host = "192.168.1.34", audioPort = "70000"),
        ),
    )
}

/** Actions that do nothing, for previews. */
internal object PreviewActions : MonitorActions {
    override fun connect(server: ServerEndpoint) = Unit

    override fun connectManual(host: String, controlPort: String, audioPort: String) = Unit

    override fun editManualAddress(address: ManualAddress) = Unit

    override fun disconnect() = Unit

    override fun setChannelLevel(channel: Int, level: Int) = Unit

    override fun setChannelPan(channel: Int, pan: Int) = Unit

    override fun setChannelMuted(channel: Int, muted: Boolean) = Unit

    override fun setChannelSoloed(channel: Int, soloed: Boolean) = Unit

    override fun setGroupLevel(group: Int, level: Int) = Unit

    override fun setMasterVolume(level: Int) = Unit

    override fun setMasterMuted(muted: Boolean) = Unit

    override fun setLevelCap(level: Int) = Unit

    override fun setMoreOfMe(on: Boolean) = Unit

    override fun setMusicianChannel(channel: Int?) = Unit

    override fun saveMix(name: String) = false

    override fun recallMix(name: String) = Unit

    override fun renameMix(from: String, to: String) = Unit

    override fun deleteMix(name: String) = Unit

    override fun setName(name: String) = Unit

    override fun selectTab(tab: MainTab) = Unit
}
