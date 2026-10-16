package com.mixlink.android.ui.state

import com.mixlink.android.ChannelInfo
import com.mixlink.android.ControlState
import com.mixlink.android.EngineerControls
import com.mixlink.android.GroupInfo
import com.mixlink.android.MixSnapshot
import com.mixlink.android.controlStatusLabel
import com.mixlink.android.defaultPan
import com.mixlink.android.moreOfMeFor
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
import com.mixlink.android.ui.state.MonitorEvent.MixAnswered
import com.mixlink.android.ui.state.MonitorEvent.MixDeleted
import com.mixlink.android.ui.state.MonitorEvent.MixMissing
import com.mixlink.android.ui.state.MonitorEvent.MixNotice
import com.mixlink.android.ui.state.MonitorEvent.MixPushed
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
        // Counters and errors describe one session, so a new one starts them again. So does what the
        // engineer set: this server reports its own (an older one never does, and leaves all open).
        diagnostics = DiagnosticsUiState(),
        mix = state.mix.copy(engineer = EngineerControls()),
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
    is ConfigReceived -> applyConfig(state, event.channels, event.groups, event.details)

    is ChannelLevelChanged -> state.withStrip(event.channel) { mix, sides ->
        mix.copy(levels = mix.levels.replacedAt(sides, event.level.coerceIn(PERCENT)))
    }
    is ChannelPanChanged -> state.withStrip(event.channel) { mix, sides ->
        mix.copy(pans = mix.pans.panned(sides, event.pan.coerceIn(PERCENT)))
    }
    is ChannelMuteChanged -> state.withStrip(event.channel) { mix, sides ->
        mix.copy(mutes = mix.mutes.replacedAt(sides, event.muted))
    }
    is ChannelSoloChanged -> state.withStrip(event.channel) { mix, sides ->
        mix.copy(solos = mix.solos.replacedAt(sides, event.soloed))
    }
    is GroupLevelChanged -> setGroupLevel(state, event.group, event.level.coerceIn(PERCENT))
    is MasterVolumeChanged -> state.copy(mix = state.mix.copy(masterVolume = event.level.coerceIn(PERCENT)))
    is MasterMuteChanged -> state.copy(mix = state.mix.copy(masterMuted = event.muted))
    is LevelCapChanged -> state.copy(mix = state.mix.copy(levelCap = event.level.coerceIn(PERCENT)))
    // A push while a change of the phone's is on its way is overtaken by that change's answer; the
    // engineer's controls it carries hold at once (see [withEngineer]).
    is MixPushed -> state.withEngineer(event.engineer).let { reported ->
        if (reported.mixSync.settled) applyServerMix(reported, event.snapshot) else reported
    }
    is MixAnswered -> answerMix(state, event.seq, event.applied, event.engineer)

    is MoreOfMeToggled -> state.copy(moreOfMe = toggleMoreOfMe(state, event.on))
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

/**
 * [reduce] for a change the musician made. When [sends] says the change goes to the server, the same
 * step numbers it as the phone's newest mix ([MixSyncState.changed]): the change and its number land
 * in one state, so a send still queued already counts, and no answer from the server can be matched
 * to a mix older than the one on screen.
 */
internal fun reduceChange(
    state: MonitorUiState,
    event: MonitorEvent,
    sends: (before: MonitorUiState, after: MonitorUiState) -> Boolean,
): MonitorUiState {
    val next = reduce(state, event)
    return if (sends(state, next)) next.copy(mixSync = next.mixSync.changed()) else next
}

/** The connection showing a control channel in [controlState], with the mix held where [mixAcknowledged] says. */
private fun ConnectionUiState.showing(controlState: ControlState, mixAcknowledged: Boolean) = copy(
    pill = pillKindFor(controlState),
    mixHeldByServer = mixAcknowledged,
    statusDetail = controlStatusLabel(controlState, mixAcknowledged),
)

/**
 * A `config`: resizes the per-channel lists when the channel count changed, takes the announced
 * groups and channel details (which rebuild the strips), and fits the musician's channel to them.
 *
 * The lists follow the old rule exactly: a channel that still exists keeps what the musician set,
 * a new one starts neutral with the captured pan layout, and a repeated count changes nothing.
 * Group levels are kept by position and a new group starts neutral. Nothing is sent: a pair the
 * engineer just linked was panned apart on the server, and the `mix_state` that follows says so.
 */
private fun applyConfig(
    state: MonitorUiState,
    channels: Int,
    groups: List<GroupInfo>,
    details: List<ChannelInfo>?,
): MonitorUiState {
    val mix = state.mix
    val unchanged = mix.sourceChannels == channels && mix.levels.size == channels
    val resized = if (unchanged) mix else mix.copy(
        sourceChannels = channels,
        levels = List(channels) { mix.levels.getOrElse(it) { NEUTRAL } },
        pans = List(channels) { mix.pans.getOrElse(it) { defaultPan(it) } },
        mutes = List(channels) { mix.mutes.getOrElse(it) { false } },
        solos = List(channels) { mix.solos.getOrElse(it) { false } },
    )
    val announced = resized.copy(
        groupDefinitions = groups,
        groupLevels = List(groups.size) { mix.groupLevels.getOrElse(it) { NEUTRAL } },
        channelDetails = details,
    )
    return state.copy(mix = announced, moreOfMe = state.moreOfMe.fittedTo(announced).offIfLocked(announced))
}

/**
 * The engineer's [controls] as the server reported them with a mix. They hold at once, even when the
 * mix they came with is overtaken by a change of the phone's still on its way: the phone never sends
 * them, so nothing of its own can overtake them, and the server reports them in order. A report
 * belongs to a session, so a late one changes nothing once the session ended. More of me goes off
 * when the musician's channel is now locked.
 */
private fun MonitorUiState.withEngineer(controls: EngineerControls): MonitorUiState {
    if (!connection.sessionRunning || mix.engineer == controls) return this
    val held = mix.copy(engineer = controls)
    return copy(mix = held, moreOfMe = moreOfMe.offIfLocked(held))
}

/**
 * More of me off, saying why, when it is on and the engineer locked the musician's channel: the
 * server keeps that channel as the engineer left it, so it cannot be lifted.
 */
private fun MoreOfMeUiState.offIfLocked(mix: MixUiState): MoreOfMeUiState {
    val channel = musicianChannel ?: return this
    return if (enabled && mix.stripLocked(channel)) copy(enabled = false, notice = MoreOfMeNotice.CHANNEL_LOCKED) else this
}

/**
 * Fits the musician's channel to the strips of the channels [mix] announces. A channel on the second
 * side of a stereo pair becomes the pair's first, the channel its strip is known by, and More of me
 * keeps lifting the pair. A channel with no strip (past the announced count, or hidden, and so in no
 * mix) is forgotten: More of me, if it was on, goes off and says why; a channel that was only
 * remembered is dropped quietly.
 */
private fun MoreOfMeUiState.fittedTo(mix: MixUiState): MoreOfMeUiState {
    val channel = musicianChannel ?: return this
    if (mix.sourceChannels <= 0) return this
    val strip = stripLayout(mix.sourceChannels, mix.channelDetails).firstOrNull { channel in it.sides }
    return when {
        strip == null && enabled -> copy(musicianChannel = null, enabled = false, notice = MoreOfMeNotice.CHANNEL_OUT_OF_RANGE)
        strip == null -> copy(musicianChannel = null)
        else -> copy(musicianChannel = strip.first)
    }
}

/**
 * Applies [change] to the mix when [channel] has an entry, with the channels the edit writes: both
 * sides when [channel]'s strip is a stereo pair, otherwise [channel] alone. Any other channel changes
 * nothing, and neither does a strip the engineer locked (a pair when either side is): the server
 * would keep its values anyway, so the phone's whole mix never carries a changed locked value.
 */
private inline fun MonitorUiState.withStrip(
    channel: Int,
    change: (mix: MixUiState, sides: List<Int>) -> MixUiState,
): MonitorUiState {
    if (channel !in mix.levels.indices || mix.stripLocked(channel)) return this
    return copy(mix = change(mix, mix.stripSides(channel)))
}

/**
 * The pans with [pan] written for [sides]: a single channel takes it as it is, and a stereo pair
 * takes it as its balance, written as both pans ([balanceToPans]).
 */
private fun List<Int>.panned(sides: List<Int>, pan: Int): List<Int> {
    if (sides.size < 2) return replacedAt(sides, pan)
    val (first, second) = balanceToPans(pan)
    return toMutableList().also {
        it[sides[0]] = first
        it[sides[1]] = second
    }
}

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

/**
 * More of me turns on only with a channel to lift: asking without one, or while the engineer locked
 * it, leaves it off and says why.
 */
private fun toggleMoreOfMe(state: MonitorUiState, on: Boolean): MoreOfMeUiState {
    val moreOfMe = state.moreOfMe
    val channel = moreOfMe.musicianChannel
    return when {
        on && channel == null -> moreOfMe.copy(enabled = false, notice = MoreOfMeNotice.NEEDS_CHANNEL)
        on && state.mix.stripLocked(channel!!) -> moreOfMe.copy(enabled = false, notice = MoreOfMeNotice.CHANNEL_LOCKED)
        else -> moreOfMe.copy(enabled = on, notice = null)
    }
}

/**
 * Takes [channel] as the musician's, or none. Only a channel the picker offers can be chosen: one
 * the server announced, shown, and known by its strip (a stereo pair by its first channel). Choosing
 * none while More of me is on turns it off, since there is nothing left to lift; so does choosing a
 * channel the engineer locked. The musician can still call a locked channel theirs.
 */
private fun chooseMusicianChannel(state: MonitorUiState, channel: Int?): MonitorUiState {
    if (channel != null && channel !in state.musicianChannelChoices) return state
    val moreOfMe = state.moreOfMe
    val next = if (channel == null && moreOfMe.enabled) {
        moreOfMe.copy(musicianChannel = null, enabled = false, notice = MoreOfMeNotice.NO_CHANNEL_SELECTED)
    } else {
        moreOfMe.copy(musicianChannel = channel, notice = null).offIfLocked(state.mix)
    }
    return state.copy(moreOfMe = next)
}

/**
 * The mix with a saved [snapshot] applied, as if every control had been moved by hand. What the
 * server announced (the channel count and the groups) is kept; the strips take the saved mix's size
 * until the server announces again, as the old screen did. A channel the engineer locked keeps the
 * level, pan, mute and solo it has, as a moved control on it would.
 */
private fun MixUiState.recalled(snapshot: MixSnapshot): MixUiState {
    val count = snapshot.channelGains.size
    fun <T> kept(index: Int, current: List<T>, saved: () -> T): T =
        if (engineer.isLocked(index) && index in current.indices) current[index] else saved()
    return copy(
        levels = List(count) { kept(it, levels) { snapshot.channelGains[it] } },
        pans = List(count) { kept(it, pans) { snapshot.channelPans.getOrElse(it) { defaultPan(it) } } },
        mutes = List(count) { kept(it, mutes) { snapshot.channelMutes.getOrElse(it) { false } } },
        solos = List(count) { kept(it, solos) { snapshot.channelSolos.getOrElse(it) { false } } },
        groupLevels = snapshot.groupLevels.toList(),
        masterVolume = snapshot.volumePercent,
        levelCap = snapshot.maxLevelPercent,
        masterMuted = snapshot.muted,
    )
}

/**
 * The server answered the phone's change number [seq], with the whole mix it now plays ([applied])
 * or with nothing (it refused the mix). The server answers in order, so every older change is
 * answered too. Only the answer to the newest change is shown: an older one's mix is overtaken by
 * a change still on its way, whose own answer follows. The engineer's [engineer] controls of any
 * readable answer hold at once ([withEngineer]); a refusal carries none and leaves them as they were.
 */
private fun answerMix(
    state: MonitorUiState,
    seq: Long,
    applied: MixSnapshot?,
    engineer: EngineerControls,
): MonitorUiState {
    if (applied == null) return state.copy(mixSync = state.mixSync.answeredUpTo(seq))
    val answered = state.copy(mixSync = state.mixSync.answeredUpTo(seq)).withEngineer(engineer)
    return if (seq == state.mixSync.latest) applyServerMix(answered, applied) else answered
}

/**
 * The server's whole mix, pushed because it changed elsewhere (such as on the desktop) or answering
 * the phone's newest change: it becomes the phone's current mix, exactly as the server plays it. The
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

/**
 * The mix as the phone sends it: with More of me on, its channel lifted (both sides of a stereo
 * pair) and the others lowered, except the channels the engineer locked, which stay as they are.
 */
private fun MixUiState.asPlayed(moreOfMe: MoreOfMeUiState): MixUiState {
    if (!moreOfMe.enabled) return this
    val channel = moreOfMe.musicianChannel ?: return this
    val lifted = toSnapshot().moreOfMeFor(stripSides(channel), engineer.lockedChannels) ?: return this
    return copy(levels = lifted.channelGains.toList())
}

/** The saved mixes listing [names]; the active mix stays active only while it is still saved. */
private fun SavedMixesUiState.listing(names: List<String>): SavedMixesUiState =
    copy(names = names, activeName = activeName?.takeIf { it in names })

private fun <T> List<T>.replacedAt(indices: List<Int>, value: T): List<T> =
    toMutableList().also { list -> indices.forEach { list[it] = value } }
