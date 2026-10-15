package com.mixlink.android.ui.state

import com.mixlink.android.GroupInfo
import com.mixlink.android.MixSnapshot
import com.mixlink.android.defaultPan
import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.components.PillKind

/** The UDP port this phone listens on for audio until the musician sets another. */
const val DEFAULT_AUDIO_PORT = 50000

/** The server's WebSocket control port until the musician or a beacon says otherwise. */
const val DEFAULT_CONTROL_PORT = 50001

/** A neutral level for a channel, a group or the master: unchanged gain. */
private const val NEUTRAL_LEVEL = 100

/**
 * Everything the Connect, Mix, Saved mixes and Settings screens show, as one immutable value.
 *
 * The screens only read it; every change goes through [reduce], so the same events always give the
 * same screen. Lists here are plain immutable lists and every nested value is a data class, so two
 * states are equal exactly when the screens would look the same.
 */
data class MonitorUiState(
    /** The name this musician registers under, exactly as typed. */
    val musicianName: String = "",
    val connection: ConnectionUiState = ConnectionUiState(),
    /** The servers advertising on the network right now, in the order they were first heard. */
    val servers: List<ServerEndpoint> = emptyList(),
    val mix: MixUiState = MixUiState(),
    val moreOfMe: MoreOfMeUiState = MoreOfMeUiState(),
    val savedMixes: SavedMixesUiState = SavedMixesUiState(),
    val diagnostics: DiagnosticsUiState = DiagnosticsUiState(),
    /** The tab of the connected app that is showing. */
    val tab: MainTab = MainTab.MIX,
) {
    /** The channels the musician can call theirs: one per source channel the server announced. */
    val musicianChannelChoices: List<Int>
        get() = (0 until mix.sourceChannels).toList()
}

/** A MixLink server's control endpoint: where it was heard from and its WebSocket port. */
data class ServerEndpoint(val address: String, val controlPort: Int) {
    /** How the server is named on screen. Beacons carry no server name, so the address names it. */
    val label: String
        get() = "$address:$controlPort"
}

/**
 * The manual connection form in Settings, kept as typed so a mistake can be corrected.
 *
 * [audioPort] is this phone's own UDP listen port. It is used for a discovered server too, exactly
 * as the old screen kept its port field when a found server filled the address.
 */
data class ManualAddress(
    val host: String = "",
    val controlPort: String = DEFAULT_CONTROL_PORT.toString(),
    val audioPort: String = DEFAULT_AUDIO_PORT.toString(),
)

/** The session and its control channel, as the status pill, the header and Settings show them. */
data class ConnectionUiState(
    val pill: PillKind = PillKind.NOT_CONNECTED,
    /** True from Connect until Disconnect, or until the receiver ends on its own. */
    val sessionRunning: Boolean = false,
    /** The server of the running session; null when no session runs. */
    val server: ServerEndpoint? = null,
    /**
     * True once the server acknowledged a mix: it applies the mix and keeps it while the control
     * channel is away. False means this phone still mixes on its own. The reconnecting banner says
     * which one holds the mix.
     */
    val mixHeldByServer: Boolean = false,
    /** The control channel's state in words, including who holds the mix. */
    val statusDetail: String = "not connected",
    /** Why the last Connect was refused (an empty address, a bad port); null when it was not. */
    val problem: String? = null,
    val manual: ManualAddress = ManualAddress(),
) {
    /** True while the control channel retries on its own; the reconnecting banner shows then. */
    val reconnecting: Boolean
        get() = pill == PillKind.RECONNECTING
}

/**
 * The mix as this phone keeps it: the values sent to the server and saved in a mix.
 *
 * The per-channel lists always have one entry per channel strip. Their size is the source channel
 * count the server announced, or the size of a recalled mix until the server announces again,
 * exactly as the old controls behaved. [groupLevels] is kept as recalled or set, even before the
 * server announces its groups, so a mix recalled before connecting keeps its group levels.
 */
data class MixUiState(
    /** Source channels the server announced in its last `config`; 0 before the first one. */
    val sourceChannels: Int = 0,
    val levels: List<Int> = emptyList(),
    val pans: List<Int> = emptyList(),
    val mutes: List<Boolean> = emptyList(),
    val solos: List<Boolean> = emptyList(),
    /** The groups the server announced, in its order; their index is the index of their level. */
    val groupDefinitions: List<GroupInfo> = emptyList(),
    val groupLevels: List<Int> = emptyList(),
    val masterVolume: Int = NEUTRAL_LEVEL,
    val masterMuted: Boolean = false,
    /** The ceiling the phone never plays above, in percent. */
    val levelCap: Int = NEUTRAL_LEVEL,
) {
    /** One strip per channel, left to right. */
    val channels: List<ChannelUiState>
        get() = levels.indices.map { index ->
            ChannelUiState(
                index = index,
                label = channelName(index),
                group = groupOf(index),
                level = levels[index],
                pan = pans.getOrElse(index) { defaultPan(index) },
                muted = mutes.getOrElse(index) { false },
                soloed = solos.getOrElse(index) { false },
            )
        }

    /** One fader per announced group; a group with no level yet reads neutral. */
    val groups: List<GroupUiState>
        get() = groupDefinitions.mapIndexed { index, group ->
            GroupUiState(
                index = index,
                name = group.name,
                channels = group.channels,
                level = groupLevels.getOrElse(index) { NEUTRAL_LEVEL },
            )
        }

    /** The first announced group that lists [channel], which is the one its strip names. */
    private fun groupOf(channel: Int): GroupTag? {
        val index = groupDefinitions.indexOfFirst { channel in it.channels }
        return if (index < 0) null else GroupTag(index, groupDefinitions[index].name)
    }

    /** The mix as the control channel sends it and a saved mix stores it. */
    fun toSnapshot(): MixSnapshot = MixSnapshot(
        channelGains = levels.toIntArray(),
        channelPans = pans.toIntArray(),
        channelMutes = mutes.toBooleanArray(),
        channelSolos = solos.toBooleanArray(),
        groupLevels = groupLevels.toIntArray(),
        volumePercent = masterVolume,
        maxLevelPercent = levelCap,
        muted = masterMuted,
    )
}

/** One channel strip: its fader, pan, Mute and Solo. [index] is 0-based; [label] is "CH 1" up. */
data class ChannelUiState(
    val index: Int,
    val label: String,
    /** The group the strip's chip names; null when no announced group lists the channel. */
    val group: GroupTag?,
    val level: Int,
    /** 0 is hard left, 50 centre, 100 hard right. */
    val pan: Int,
    val muted: Boolean,
    val soloed: Boolean,
)

/** A group as a channel's chip names it; [index] picks the chip's tone. */
data class GroupTag(val index: Int, val name: String)

/** One group fader. [channels] are the 0-based channels the server put in the group. */
data class GroupUiState(val index: Int, val name: String, val channels: List<Int>, val level: Int)

/** Why More of me is not doing what the musician last asked; the toggle's caption shows it. */
enum class MoreOfMeNotice(val message: String) {
    NEEDS_CHANNEL("Choose your channel first, then turn More of me on"),
    NO_CHANNEL_SELECTED("More of me is off: no channel is chosen"),
    CHANNEL_OUT_OF_RANGE("More of me is off: your channel is no longer in the mix"),
}

/**
 * More of me and the channel it lifts. [musicianChannel] is remembered on the phone, apart from any
 * saved mix, because it says who the musician is rather than how the mix sounds.
 */
data class MoreOfMeUiState(
    val enabled: Boolean = false,
    val musicianChannel: Int? = null,
    val notice: MoreOfMeNotice? = null,
) {
    /** The musician's channel as its strip names it, or null when none is chosen. */
    val channelLabel: String?
        get() = musicianChannel?.let(::channelName)
}

/** One saved mix in the list. [active] marks the mix last saved or recalled. */
data class SavedMixUiState(val name: String, val active: Boolean)

/**
 * The saved mixes, in the order the phone keeps them, and the outcome of the last action on them.
 *
 * The empty state is derived from the list itself, never kept as a separate message, so it can
 * never show while a mix exists.
 */
data class SavedMixesUiState(
    val names: List<String> = emptyList(),
    /** The mix last saved or recalled, while it is still saved. */
    val activeName: String? = null,
    /** What the last save, recall, rename or delete did, or why it could not. */
    val notice: String? = null,
) {
    val mixes: List<SavedMixUiState>
        get() = names.map { SavedMixUiState(it, it == activeName) }

    /** Shown in place of the list when there is nothing saved; null whenever one mix exists. */
    val emptyMessage: String?
        get() = if (names.isEmpty()) "No saved mixes yet" else null
}

/**
 * What the receiver and the control channel report, for the diagnostics in Settings.
 *
 * Counters are for the running session and start again at every Connect. [linkHealth] is judged on
 * the phone from the same report the server judges, with the same thresholds.
 */
data class DiagnosticsUiState(
    val packetsReceived: Long = 0,
    /** Packets the stream skipped, from the sequence numbers. */
    val packetsLost: Long = 0,
    val packetsPerSecond: Double = 0.0,
    val interArrivalMs: Double = 0.0,
    val jitterMs: Double = 0.0,
    /** How far playback runs behind the newest sample written, smoothed. */
    val playbackBufferMs: Double = 0.0,
    /** The sample rate of the last packet; null before the first one. */
    val sampleRateHz: Int? = null,
    val discardedSamples: Long = 0,
    val packetsWithDiscardedSamples: Long = 0,
    /** Playback underruns reported in this session's link reports. */
    val underruns: Long = 0,
    val linkHealth: LinkHealth = LinkHealth.UNKNOWN,
    /** The last receiver or connection problem; null when there was none. */
    val lastError: String? = null,
    /** The last control channel problem; null when there was none. */
    val controlError: String? = null,
) {
    /** Lost packets as a percentage of those expected (received plus lost); 0 before any. */
    val lossPercent: Double
        get() {
            val expected = packetsReceived + packetsLost
            return if (expected == 0L) 0.0 else packetsLost * 100.0 / expected
        }
}

/** How a channel is named on its strip and in the More of me caption: "CH 1" for index 0. */
fun channelName(index: Int): String = "CH ${index + 1}"
