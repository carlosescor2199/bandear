package com.mixlink.android.ui.state

import com.mixlink.android.ChannelInfo
import com.mixlink.android.ChannelKind
import com.mixlink.android.EngineerControls
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

/** The longest musician name the server keeps (it cuts longer ones); the name fields stop there. */
const val MAX_NAME_LENGTH = 64

/** A neutral level for a channel, a group or the master: unchanged gain. */
private const val NEUTRAL_LEVEL = 100

/**
 * Everything the Connect, Mix, Saved mixes and Settings screens show, as one immutable value.
 *
 * The screens only read it; every change goes through [reduce], so the same events always give the
 * same screen. Lists here are plain immutable lists and every nested value is a data class, so two
 * states are equal exactly when the screens would look the same and [mixSync], the one value no
 * screen shows, is the same too.
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
    /**
     * Which of the phone's mixes the server has answered. Not shown; it lives here so a change and
     * its number land in one state, and an answer is judged against the mix it is shown over.
     */
    val mixSync: MixSyncState = MixSyncState(),
) {
    /** The channels the musician can call theirs: one per strip of the channels the server announced. */
    val musicianChannelChoices: List<Int>
        get() = mix.musicianChoices.map { it.index }

    /**
     * The channels More of me lifts: the musician's channel and, when its strip is a stereo pair, the
     * other side too; empty when no channel is chosen.
     */
    val moreOfMeChannels: List<Int>
        get() = moreOfMe.musicianChannel?.let(mix::stripSides).orEmpty()
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
 * The per-channel lists always have one entry per source channel, hidden ones and both sides of a
 * stereo pair included, since that is what the control channel sends and a saved mix stores. Their
 * size is the source channel count the server announced, or the size of a recalled mix until the
 * server announces again, exactly as the old controls behaved. [groupLevels] is kept as recalled or
 * set, even before the server announces its groups, so a mix recalled before connecting keeps its
 * group levels.
 *
 * The strips are derived from the lists and the server's [channelDetails] ([stripLayout]): a hidden
 * channel has none, and a stereo pair has one, which reads its first channel and the mean of its
 * two pans.
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
    /**
     * The details the server announced for its channels (names, kinds, hidden channels and pairs),
     * one per source channel; null when its `config` carried none, as an older server's does.
     */
    val channelDetails: List<ChannelInfo>? = null,
    /**
     * What the engineer set for this musician (a limit and locked channels), as the server last
     * reported it in this session. Never sent and never saved: the server alone sets it.
     */
    val engineer: EngineerControls = EngineerControls(),
) {
    /** The loudest the musician hears: the lower of the phone's [levelCap] and the engineer's limit. */
    val effectiveCap: Int
        get() = minOf(levelCap, engineer.limitPercent)

    /** True when the engineer locked [channel]'s strip: a pair is locked when either side is. */
    fun stripLocked(channel: Int): Boolean = stripSides(channel).any(engineer::isLocked)

    /** Which channels each strip controls, left to right. */
    val layout: List<StripChannels>
        get() = stripLayout(levels.size, channelDetails)

    /** One strip per shown channel or stereo pair, left to right. */
    val channels: List<ChannelUiState>
        get() = layout.map(::stripState)

    /**
     * One fader per announced group; a group with no level yet reads neutral. Without details from
     * the server its members are its channels by number, as they always were.
     */
    val groups: List<GroupUiState>
        get() {
            val strips = layout
            return groupDefinitions.mapIndexed { index, group ->
                GroupUiState(
                    index = index,
                    name = group.name,
                    channels = group.channels,
                    level = groupLevels.getOrElse(index) { NEUTRAL_LEVEL },
                    members = if (channelDetails == null) {
                        group.channels.map(::channelName)
                    } else {
                        strips.filter { strip -> strip.sides.any { it in group.channels } }.map { labelOf(it.first) }
                    },
                )
            }
        }

    /** The channels the musician can call theirs: one per strip of the announced channels, by name. */
    val musicianChoices: List<ChannelChoice>
        get() = stripLayout(sourceChannels, channelDetails).map { ChannelChoice(it.first, labelOf(it.first)) }

    /** How [channel]'s strip is named: the engineer's name, or "CH n" when it has none. */
    fun labelOf(channel: Int): String = channelLabel(channel, channelDetails?.getOrNull(channel))

    /**
     * The channels an edit of [channel] writes: both sides when [channel] is the first of a stereo
     * pair's strip, otherwise [channel] alone.
     */
    fun stripSides(channel: Int): List<Int> = layout.firstOrNull { it.first == channel }?.sides ?: listOf(channel)

    private fun stripState(strip: StripChannels): ChannelUiState {
        val index = strip.first
        val second = strip.second
        val pan = if (second == null) panOf(index) else pansToBalance(panOf(index), panOf(second))
        return ChannelUiState(
            index = index,
            label = labelOf(index),
            group = groupOf(index),
            level = levels[index],
            pan = pan,
            muted = mutes.getOrElse(index) { false },
            soloed = solos.getOrElse(index) { false },
            kind = channelDetails?.let { it.getOrNull(index)?.kind ?: ChannelKind.OTHER },
            pairedWith = second,
            locked = strip.sides.any(engineer::isLocked),
        )
    }

    private fun panOf(channel: Int): Int = pans.getOrElse(channel) { defaultPan(channel) }

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

/**
 * The phone numbers every change it sends to the server, and the server answers each `mix` in order
 * with that number (`seq`): with `mix_ack` and the whole mix it now plays, or with an `error`.
 *
 * [latest] is the number of the newest change, taken in the same step as the change, so a send that
 * is still queued already counts. [answered] is the newest number the server answered. Only the
 * answer to [latest] is the server's latest word on the mix; while an older one is outstanding, any
 * mix the server reports is overtaken by a mix of the phone's still on its way. Numbers only grow,
 * across reconnects too: every open sends the mix again under [latest], so its answer settles it.
 */
data class MixSyncState(val latest: Long = 0, val answered: Long = 0) {
    /** True when the server answered the newest change, so nothing of the phone's is on its way. */
    val settled: Boolean
        get() = answered >= latest

    /** A new change, numbered as the newest. */
    fun changed(): MixSyncState = copy(latest = latest + 1)

    /** The server answered change [seq]; it answers in order, so every older one is answered too. */
    fun answeredUpTo(seq: Long): MixSyncState = copy(answered = maxOf(answered, seq))
}

/**
 * One channel strip: its fader, pan, Mute and Solo. [index] is the 0-based channel it is known by
 * (a stereo pair's first channel); [label] is the engineer's name for it, or "CH 1" up.
 */
data class ChannelUiState(
    val index: Int,
    val label: String,
    /** The group the strip's chip names; null when no announced group lists the channel. */
    val group: GroupTag?,
    val level: Int,
    /** 0 is hard left, 50 centre, 100 hard right; a stereo pair's balance, read the same way. */
    val pan: Int,
    val muted: Boolean,
    val soloed: Boolean,
    /** What the channel carries; null when the server announced no details, so no icon shows. */
    val kind: ChannelKind? = null,
    /** The pair's second channel when the strip is a stereo pair; null for a single channel. */
    val pairedWith: Int? = null,
    /**
     * True when the engineer locked the strip (a pair when either side is): its level, pan, mute
     * and solo cannot be changed from the phone.
     */
    val locked: Boolean = false,
) {
    /** True when the strip is a stereo pair, whose pan is a balance. */
    val stereo: Boolean
        get() = pairedWith != null
}

/** A group as a channel's chip names it; [index] picks the chip's tone. */
data class GroupTag(val index: Int, val name: String)

/**
 * One group fader. [channels] are the 0-based channels the server put in the group; [members] names
 * the group's strips as they are shown, a hidden channel left out and a stereo pair named once.
 */
data class GroupUiState(
    val index: Int,
    val name: String,
    val channels: List<Int>,
    val level: Int,
    val members: List<String> = channels.map(::channelName),
)

/** A channel the musician can call theirs, by the 0-based channel its strip is known by. */
data class ChannelChoice(val index: Int, val label: String)

/** Why More of me is not doing what the musician last asked; the toggle's caption shows it. */
enum class MoreOfMeNotice(val message: String) {
    NEEDS_CHANNEL("Choose your channel first, then turn More of me on"),
    NO_CHANNEL_SELECTED("More of me is off: no channel is chosen"),
    CHANNEL_OUT_OF_RANGE("More of me is off: your channel is no longer in the mix"),

    /** The server pushed a mix changed elsewhere; the phone shows it as played, so nothing is lifted. */
    MIX_CHANGED_ELSEWHERE("More of me is off: your mix was changed from the desktop"),

    /** The engineer locked the musician's channel, so the phone cannot lift it. */
    CHANNEL_LOCKED("More of me is off: the engineer locked your channel"),
}

/**
 * More of me and the channel it lifts. [musicianChannel] is remembered on the phone, apart from any
 * saved mix, because it says who the musician is rather than how the mix sounds.
 */
data class MoreOfMeUiState(
    val enabled: Boolean = false,
    val musicianChannel: Int? = null,
    val notice: MoreOfMeNotice? = null,
)

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
