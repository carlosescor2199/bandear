package com.mixlink.android.ui.state

import com.mixlink.android.ControlState
import com.mixlink.android.GroupInfo
import com.mixlink.android.MixSnapshot
import com.mixlink.android.controlStatusLabel
import com.mixlink.android.defaultPan
import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.components.pillKindFor
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
import com.mixlink.android.ui.state.MonitorEvent.ServerMixReceived
import com.mixlink.android.ui.state.MonitorEvent.ServersChanged
import com.mixlink.android.ui.state.MonitorEvent.SessionEnded
import com.mixlink.android.ui.state.MonitorEvent.SessionStarted
import com.mixlink.android.ui.state.MonitorEvent.StatsUpdated
import com.mixlink.android.ui.state.MonitorEvent.TabSelected

private const val NEUTRAL = 100
private val PERCENT = 0..100

/**
 * Returns the state that follows [state] once [event] happened. Pure: no IO, no clock, no threads,
 * so the whole app's screen logic is an ordinary JVM test.
 *
 * Requests the rules refuse come back unchanged or with a notice: a control for a channel that does
 * not exist, a musician channel the server did not announce, More of me with no channel.
 */
internal fun reduce(state: MonitorUiState, event: MonitorEvent): MonitorUiState = when (event) {
    is SessionStarted -> state.copy(
        connection = state.connection.copy(
            sessionRunning = true,
            server = event.server,
            problem = null,
            manual = ManualAddress(
                host = event.server.address,
                controlPort = event.server.controlPort.toString(),
                audioPort = event.audioPort.toString(),
            ),
        ).showing(ControlState.CONNECTING, mixAcknowledged = false),
        // Counters and errors describe one session, so a new one starts them again.
        diagnostics = DiagnosticsUiState(),
        tab = MainTab.MIX,
    )
    SessionEnded -> state.copy(
        connection = state.connection.copy(sessionRunning = false, server = null)
            .showing(ControlState.IDLE, state.connection.mixHeldByServer),
        diagnostics = state.diagnostics.copy(linkHealth = LinkHealth.UNKNOWN),
    )
    is ConnectRejected -> state.copy(
        connection = state.connection.copy(problem = event.message),
        diagnostics = state.diagnostics.copy(lastError = event.message),
    )
    is ControlChanged ->
        // Once the session ended only "idle" is true: a late callback cannot raise the pill again.
        if (!state.connection.sessionRunning && event.state != ControlState.IDLE) {
            state
        } else {
            state.copy(connection = state.connection.showing(event.state, event.mixAcknowledged))
        }
    is MixAcknowledged ->
        // An acknowledgement belongs to a session too: a late one cannot mark the mix held while idle.
        if (!state.connection.sessionRunning) {
            state
        } else {
            state.copy(connection = state.connection.showing(event.state, mixAcknowledged = true))
        }
    is ServersChanged -> state.copy(servers = event.servers)
    is ManualAddressEdited -> state.copy(connection = state.connection.copy(manual = event.address))
    is ConfigReceived -> applyConfig(state, event.channels, event.groups)

    is ChannelLevelChanged -> state.withChannel(event.channel) { mix ->
        mix.copy(levels = mix.levels.replaced(event.channel, event.level.coerceIn(PERCENT)))
    }
    is ChannelPanChanged -> state.withChannel(event.channel) { mix ->
        mix.copy(pans = mix.pans.replaced(event.channel, event.pan.coerceIn(PERCENT)))
    }
    is ChannelMuteChanged -> state.withChannel(event.channel) { mix ->
        mix.copy(mutes = mix.mutes.replaced(event.channel, event.muted))
    }
    is ChannelSoloChanged -> state.withChannel(event.channel) { mix ->
        mix.copy(solos = mix.solos.replaced(event.channel, event.soloed))
    }
    is GroupLevelChanged -> setGroupLevel(state, event.group, event.level.coerceIn(PERCENT))
    is MasterVolumeChanged -> state.copy(mix = state.mix.copy(masterVolume = event.level.coerceIn(PERCENT)))
    is MasterMuteChanged -> state.copy(mix = state.mix.copy(masterMuted = event.muted))
    is LevelCapChanged -> state.copy(mix = state.mix.copy(levelCap = event.level.coerceIn(PERCENT)))
    is ServerMixReceived -> applyServerMix(state, event.snapshot)

    is MoreOfMeToggled -> state.copy(moreOfMe = toggleMoreOfMe(state.moreOfMe, event.on))
    is MusicianChannelChosen -> chooseMusicianChannel(state, event.channel)

    is MixesLoaded -> state.copy(savedMixes = state.savedMixes.listing(event.names))
    is MixSaved -> state.copy(
        savedMixes = state.savedMixes.copy(activeName = event.name, notice = "Saved \"${event.name}\"")
            .listing(event.names),
    )
    is MixRecalled -> state.copy(
        mix = state.mix.recalled(event.snapshot),
        savedMixes = state.savedMixes.copy(activeName = event.name, notice = "Recalled \"${event.name}\"")
            .listing(state.savedMixes.names),
    )
    is MixMissing -> state.copy(
        savedMixes = state.savedMixes.copy(notice = "\"${event.name}\" is no longer saved").listing(event.names),
    )
    is MixDeleted -> state.copy(
        savedMixes = state.savedMixes.copy(notice = "Deleted \"${event.name}\"").listing(event.names),
    )
    is MixRenamed -> state.copy(
        savedMixes = state.savedMixes.copy(
            activeName = if (state.savedMixes.activeName == event.from) event.to else state.savedMixes.activeName,
            notice = "Renamed \"${event.from}\" to \"${event.to}\"",
        ).listing(event.names),
    )
    is MixNotice -> state.copy(savedMixes = state.savedMixes.copy(notice = event.message))

    is StatsUpdated -> state.copy(
        diagnostics = state.diagnostics.copy(
            packetsReceived = event.stats.packets,
            packetsLost = event.stats.lost,
            sampleRateHz = event.stats.sampleRate,
            packetsPerSecond = event.stats.packetsPerSecond,
            interArrivalMs = event.stats.interArrivalMs,
            jitterMs = event.stats.jitterMs,
            discardedSamples = event.stats.discardedSamples,
            packetsWithDiscardedSamples = event.stats.packetsWithDiscardedSamples,
            playbackBufferMs = event.stats.playbackBacklogMs,
        ),
    )
    is LinkStatsReported -> state.copy(
        diagnostics = state.diagnostics.copy(
            linkHealth = phoneLinkHealth(event.report),
            underruns = state.diagnostics.underruns + event.report.underruns,
        ),
    )
    is AudioError -> state.copy(diagnostics = state.diagnostics.copy(lastError = event.message))
    is ControlError -> state.copy(diagnostics = state.diagnostics.copy(controlError = event.message))

    is NameChanged -> state.copy(musicianName = event.name)
    is TabSelected -> state.copy(tab = event.tab)
}

/** The connection showing a control channel in [controlState], with the mix held where [mixAcknowledged] says. */
private fun ConnectionUiState.showing(controlState: ControlState, mixAcknowledged: Boolean) = copy(
    pill = pillKindFor(controlState),
    mixHeldByServer = mixAcknowledged,
    statusDetail = controlStatusLabel(controlState, mixAcknowledged),
)

/**
 * A `config`: resizes the strips when the channel count changed, takes the announced groups, and
 * forgets a musician channel the new count no longer has.
 *
 * The strips follow the old rule exactly: a channel that still exists keeps what the musician set,
 * a new one starts neutral with the captured pan layout, and a repeated count changes nothing.
 * Group levels are kept by position and a new group starts neutral.
 */
private fun applyConfig(state: MonitorUiState, channels: Int, groups: List<GroupInfo>): MonitorUiState {
    val mix = state.mix
    val unchanged = mix.sourceChannels == channels && mix.levels.size == channels
    val resized = if (unchanged) mix else mix.copy(
        sourceChannels = channels,
        levels = List(channels) { mix.levels.getOrElse(it) { NEUTRAL } },
        pans = List(channels) { mix.pans.getOrElse(it) { defaultPan(it) } },
        mutes = List(channels) { mix.mutes.getOrElse(it) { false } },
        solos = List(channels) { mix.solos.getOrElse(it) { false } },
    )
    val withGroups = resized.copy(
        groupDefinitions = groups,
        groupLevels = List(groups.size) { mix.groupLevels.getOrElse(it) { NEUTRAL } },
    )
    return state.copy(mix = withGroups, moreOfMe = if (unchanged) state.moreOfMe else state.moreOfMe.fittedTo(channels))
}

/**
 * Forgets a musician channel outside [channels]. More of me, if it was on, goes off and says why;
 * a channel that was only remembered is dropped quietly.
 */
private fun MoreOfMeUiState.fittedTo(channels: Int): MoreOfMeUiState {
    val channel = musicianChannel ?: return this
    if (channels <= 0 || channel in 0 until channels) return this
    return if (enabled) {
        copy(musicianChannel = null, enabled = false, notice = MoreOfMeNotice.CHANNEL_OUT_OF_RANGE)
    } else {
        copy(musicianChannel = null)
    }
}

/** Applies [change] to the mix when [channel] has a strip; any other channel changes nothing. */
private inline fun MonitorUiState.withChannel(channel: Int, change: (MixUiState) -> MixUiState): MonitorUiState =
    if (channel in mix.levels.indices) copy(mix = change(mix)) else this

/**
 * Sets one group's level. The levels list is rebuilt to one entry per announced group, the others
 * reading as they do on screen, exactly as the old group fader did.
 */
private fun setGroupLevel(state: MonitorUiState, group: Int, level: Int): MonitorUiState {
    val mix = state.mix
    if (group !in mix.groupDefinitions.indices) return state
    val levels = List(mix.groupDefinitions.size) { index ->
        if (index == group) level else mix.groupLevels.getOrElse(index) { NEUTRAL }
    }
    return state.copy(mix = mix.copy(groupLevels = levels))
}

/** More of me turns on only with a channel to lift; asking without one leaves it off and says why. */
private fun toggleMoreOfMe(moreOfMe: MoreOfMeUiState, on: Boolean): MoreOfMeUiState =
    if (on && moreOfMe.musicianChannel == null) {
        moreOfMe.copy(enabled = false, notice = MoreOfMeNotice.NEEDS_CHANNEL)
    } else {
        moreOfMe.copy(enabled = on, notice = null)
    }

/**
 * Takes [channel] as the musician's, or none. Only an announced channel can be chosen. Choosing none
 * while More of me is on turns it off, since there is nothing left to lift.
 */
private fun chooseMusicianChannel(state: MonitorUiState, channel: Int?): MonitorUiState {
    if (channel != null && channel !in 0 until state.mix.sourceChannels) return state
    val moreOfMe = state.moreOfMe
    val next = if (channel == null && moreOfMe.enabled) {
        moreOfMe.copy(musicianChannel = null, enabled = false, notice = MoreOfMeNotice.NO_CHANNEL_SELECTED)
    } else {
        moreOfMe.copy(musicianChannel = channel, notice = null)
    }
    return state.copy(moreOfMe = next)
}

/**
 * The mix with a saved [snapshot] applied, as if every control had been moved by hand. What the
 * server announced (the channel count and the groups) is kept; the strips take the saved mix's size
 * until the server announces again, as the old screen did.
 */
private fun MixUiState.recalled(snapshot: MixSnapshot): MixUiState {
    val count = snapshot.channelGains.size
    return copy(
        levels = snapshot.channelGains.toList(),
        pans = List(count) { snapshot.channelPans.getOrElse(it) { defaultPan(it) } },
        mutes = List(count) { snapshot.channelMutes.getOrElse(it) { false } },
        solos = List(count) { snapshot.channelSolos.getOrElse(it) { false } },
        groupLevels = snapshot.groupLevels.toList(),
        masterVolume = snapshot.volumePercent,
        levelCap = snapshot.maxLevelPercent,
        masterMuted = snapshot.muted,
    )
}

/**
 * The server's whole mix, pushed because it changed elsewhere (such as on the desktop) or answering
 * the phone's newest mix: it becomes the phone's current mix, exactly as the server plays it. The
 * saved mixes are left alone.
 *
 * A report belongs to a session, so a late one changes nothing once the session ended. A report of
 * the mix the phone already plays (an answer to its own mix, or the whole mix the server re-sends
 * after it missed events) changes nothing either. Otherwise More of me goes off and says why: the
 * reported levels are what the server plays, so lifting them again on the next send would lift them
 * twice.
 */
private fun applyServerMix(state: MonitorUiState, snapshot: MixSnapshot): MonitorUiState {
    if (!state.connection.sessionRunning) return state
    val reported = state.mix.fromServer(snapshot)
    if (reported == state.mix.asPlayed(state.moreOfMe)) return state
    val moreOfMe = if (state.moreOfMe.enabled) {
        state.moreOfMe.copy(enabled = false, notice = MoreOfMeNotice.MIX_CHANGED_ELSEWHERE)
    } else {
        state.moreOfMe
    }
    return state.copy(mix = reported, moreOfMe = moreOfMe)
}

/**
 * The mix with the server's [snapshot] applied to the channels and groups the server announced; the
 * snapshot holds its full tables (32 channels, 16 groups) and the rest is not shown. Before the first
 * `config` the strips the phone shows keep their count. A value the snapshot lacks keeps what the
 * phone has, and levels and pans stay between 0 and 100.
 */
private fun MixUiState.fromServer(snapshot: MixSnapshot): MixUiState {
    val announced = sourceChannels > 0
    val channelCount = if (announced) sourceChannels else levels.size
    val groupCount = if (announced) groupDefinitions.size else groupLevels.size
    return copy(
        levels = List(channelCount) { snapshot.channelGains.percentAt(it) ?: levels.getOrElse(it) { NEUTRAL } },
        pans = List(channelCount) { snapshot.channelPans.percentAt(it) ?: pans.getOrElse(it) { defaultPan(it) } },
        mutes = List(channelCount) { snapshot.channelMutes.getOrNull(it) ?: mutes.getOrElse(it) { false } },
        solos = List(channelCount) { snapshot.channelSolos.getOrNull(it) ?: solos.getOrElse(it) { false } },
        groupLevels = List(groupCount) { snapshot.groupLevels.percentAt(it) ?: groupLevels.getOrElse(it) { NEUTRAL } },
        masterVolume = snapshot.volumePercent.coerceIn(PERCENT),
        levelCap = snapshot.maxLevelPercent.coerceIn(PERCENT),
        masterMuted = snapshot.muted,
    )
}

/** The value at [index] kept between 0 and 100, or null when the array has no such entry. */
private fun IntArray.percentAt(index: Int): Int? = getOrNull(index)?.coerceIn(PERCENT)

/** The mix as the phone sends it: with More of me on, its channel lifted and the others lowered. */
private fun MixUiState.asPlayed(moreOfMe: MoreOfMeUiState): MixUiState {
    if (!moreOfMe.enabled) return this
    val lifted = toSnapshot().moreOfMe(moreOfMe.musicianChannel) ?: return this
    return copy(levels = lifted.channelGains.toList())
}

/** The saved mixes listing [names]; the active mix stays active only while it is still saved. */
private fun SavedMixesUiState.listing(names: List<String>): SavedMixesUiState =
    copy(names = names, activeName = activeName?.takeIf { it in names })

private fun <T> List<T>.replaced(index: Int, value: T): List<T> =
    toMutableList().also { it[index] = value }
