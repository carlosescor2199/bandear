package com.mixlink.android.ui.state

import com.mixlink.android.ControlState
import com.mixlink.android.GroupInfo
import com.mixlink.android.LinkStatsReport
import com.mixlink.android.MixSnapshot
import com.mixlink.android.ui.components.LinkHealth
import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.components.PillKind
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
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** The pure reducer, one behaviour per test; every state here is built only from events. */
class MonitorReducerTest {
    private val idle = MonitorUiState()
    private val server = ServerEndpoint("192.168.1.20", 50001)

    private fun MonitorUiState.after(vararg events: MonitorEvent): MonitorUiState =
        events.fold(this) { state, event -> reduce(state, event) }

    private fun running(): MonitorUiState = idle.after(SessionStarted(server, DEFAULT_AUDIO_PORT))

    private fun withChannels(count: Int, groups: List<GroupInfo> = emptyList()): MonitorUiState =
        idle.after(ConfigReceived(count, groups))

    private fun stats(packets: Long = 990, lost: Long = 10) = ReceiverStats(
        packets = packets,
        lost = lost,
        sampleRate = 48_000,
        packetsPerSecond = 100.0,
        interArrivalMs = 10.0,
        jitterMs = 0.4,
        discardedSamples = 2,
        packetsWithDiscardedSamples = 1,
        playbackBacklogMs = 12.5,
    )

    private fun report(received: Long = 100, underruns: Long = 0) =
        LinkStatsReport(intervalMs = 1_000, packetsReceived = received, packetsLost = 0, underruns = underruns, receiveTimeouts = 0)

    // The session and the control channel.

    @Test
    fun aStartedSessionShowsConnectingToItsServerAndLandsOnTheMix() {
        val state = idle.after(TabSelected(MainTab.SETTINGS), SessionStarted(server, DEFAULT_AUDIO_PORT))

        assertTrue(state.connection.sessionRunning)
        assertEquals(server, state.connection.server)
        assertEquals(PillKind.CONNECTING, state.connection.pill)
        assertEquals("connecting...", state.connection.statusDetail)
        assertFalse(state.connection.mixHeldByServer)
        assertEquals(MainTab.MIX, state.tab)
    }

    @Test
    fun aStartedSessionFillsTheManualFormLikeTappingAFoundServerDid() {
        val state = idle.after(SessionStarted(server, 50_010))

        assertEquals(ManualAddress("192.168.1.20", "50001", "50010"), state.connection.manual)
    }

    @Test
    fun aStartedSessionClearsTheLastSessionsCountersErrorsAndProblem() {
        val used = idle.after(
            ConnectRejected("Enter the server IP address or host name"),
            SessionStarted(server, DEFAULT_AUDIO_PORT),
            StatsUpdated(stats()),
            LinkStatsReported(report(underruns = 1)),
            AudioError("UDP socket error: closed"),
            ControlError("WebSocket rejected mix update"),
            SessionEnded,
        )

        val fresh = used.after(SessionStarted(server, DEFAULT_AUDIO_PORT))

        assertEquals(DiagnosticsUiState(), fresh.diagnostics)
        assertNull(fresh.connection.problem)
    }

    @Test
    fun theControlStateMapsToThePill() {
        val started = running()

        assertEquals(PillKind.CONNECTED, started.after(ControlChanged(ControlState.CONNECTED, false)).connection.pill)
        assertEquals(PillKind.CONNECTING, started.after(ControlChanged(ControlState.CONNECTING, false)).connection.pill)
        for (gone in listOf(ControlState.UNAVAILABLE, ControlState.CLOSED, ControlState.IDLE)) {
            assertEquals(gone.name, PillKind.NOT_CONNECTED, started.after(ControlChanged(gone, false)).connection.pill)
        }
    }

    @Test
    fun reconnectingMapsToTheReconnectingPillAndSaysWhoHoldsTheMix() {
        val held = running().after(
            ControlChanged(ControlState.CONNECTED, false),
            MixAcknowledged(ControlState.CONNECTED),
            ControlChanged(ControlState.RECONNECTING, true),
        )
        val local = running().after(ControlChanged(ControlState.RECONNECTING, false))

        assertEquals(PillKind.RECONNECTING, held.connection.pill)
        assertTrue(held.connection.reconnecting)
        assertTrue(held.connection.mixHeldByServer)
        assertEquals("reconnecting - mix held at the server", held.connection.statusDetail)
        assertEquals(PillKind.RECONNECTING, local.connection.pill)
        assertFalse(local.connection.mixHeldByServer)
        assertEquals("reconnecting - mixing on this device", local.connection.statusDetail)
    }

    @Test
    fun anAcknowledgedMixIsHeldByTheServer() {
        val state = running().after(ControlChanged(ControlState.CONNECTED, false), MixAcknowledged(ControlState.CONNECTED))

        assertEquals(PillKind.CONNECTED, state.connection.pill)
        assertTrue(state.connection.mixHeldByServer)
        assertFalse(state.connection.reconnecting)
        assertEquals("connected - mix applied on the server", state.connection.statusDetail)
    }

    @Test
    fun anEndedSessionIsNotConnectedAndForgetsItsServerAndLinkHealth() {
        val ended = running().after(
            ControlChanged(ControlState.CONNECTED, true),
            LinkStatsReported(report()),
            SessionEnded,
        )

        assertFalse(ended.connection.sessionRunning)
        assertNull(ended.connection.server)
        assertEquals(PillKind.NOT_CONNECTED, ended.connection.pill)
        assertEquals("not connected", ended.connection.statusDetail)
        assertEquals(LinkHealth.UNKNOWN, ended.diagnostics.linkHealth)
    }

    @Test
    fun aLateControlChangeCannotRaiseThePillOnceTheSessionEnded() {
        val ended = running().after(
            ControlChanged(ControlState.CONNECTED, false),
            SessionEnded,
            ControlChanged(ControlState.RECONNECTING, false),
        )

        assertEquals(PillKind.NOT_CONNECTED, ended.connection.pill)
        assertEquals("not connected", ended.connection.statusDetail)
    }

    @Test
    fun aLateAcknowledgementChangesNothingOnceTheSessionEnded() {
        val ended = running().after(ControlChanged(ControlState.CONNECTED, false), SessionEnded)

        assertEquals(ended, ended.after(MixAcknowledged(ControlState.CONNECTED)))
        assertEquals(ended, ended.after(MixAcknowledged(ControlState.IDLE)))
        assertEquals(idle, idle.after(MixAcknowledged(ControlState.CONNECTED)))
    }

    @Test
    fun anAcknowledgementInTheNextSessionIsHeldByTheServer() {
        val next = running().after(
            SessionEnded,
            SessionStarted(server, DEFAULT_AUDIO_PORT),
            ControlChanged(ControlState.CONNECTED, false),
            MixAcknowledged(ControlState.CONNECTED),
        )

        assertEquals(PillKind.CONNECTED, next.connection.pill)
        assertTrue(next.connection.mixHeldByServer)
        assertEquals("connected - mix applied on the server", next.connection.statusDetail)
    }

    @Test
    fun aRefusedConnectSaysWhyAndStartsNothing() {
        val message = "Enter a valid UDP port between 1 and 65535"

        val state = idle.after(ConnectRejected(message))

        assertEquals(message, state.connection.problem)
        assertEquals(message, state.diagnostics.lastError)
        assertFalse(state.connection.sessionRunning)
        assertEquals(PillKind.NOT_CONNECTED, state.connection.pill)
    }

    @Test
    fun theManualFormKeepsWhatWasTyped() {
        val typed = ManualAddress(host = "studio.local", controlPort = "6000", audioPort = "abc")

        assertEquals(typed, idle.after(ManualAddressEdited(typed)).connection.manual)
    }

    @Test
    fun foundServersAreListedAndALostOneDisappears() {
        val first = ServerEndpoint("10.0.0.2", 50001)
        val second = ServerEndpoint("10.0.0.3", 50001)

        val both = idle.after(ServersChanged(listOf(first, second)))
        val oneLost = both.after(ServersChanged(listOf(second)))

        assertEquals(listOf(first, second), both.servers)
        assertEquals(listOf(second), oneLost.servers)
        assertEquals("10.0.0.2:50001", first.label)
    }

    // What the server announces.

    @Test
    fun theFirstConfigBuildsNeutralStripsWithTheCapturedPanLayout() {
        val state = withChannels(3)

        assertEquals(3, state.mix.sourceChannels)
        assertEquals(listOf(100, 100, 100), state.mix.levels)
        assertEquals(listOf(0, 100, 0), state.mix.pans)
        assertEquals(listOf(false, false, false), state.mix.mutes)
        assertEquals(listOf(false, false, false), state.mix.solos)
        assertEquals(listOf("CH 1", "CH 2", "CH 3"), state.mix.channels.map { it.label })
        assertEquals(listOf(0, 1, 2), state.musicianChannelChoices)
    }

    @Test
    fun aConfigWithANewCountKeepsTheChannelsThatStillExist() {
        val set = withChannels(3).after(
            ChannelLevelChanged(0, 40),
            ChannelPanChanged(1, 30),
            ChannelMuteChanged(2, true),
        )

        val grown = set.after(ConfigReceived(4, emptyList()))
        val shrunk = set.after(ConfigReceived(2, emptyList()))

        assertEquals(listOf(40, 100, 100, 100), grown.mix.levels)
        assertEquals(listOf(0, 30, 0, 100), grown.mix.pans)
        assertEquals(listOf(false, false, true, false), grown.mix.mutes)
        assertEquals(listOf(40, 100), shrunk.mix.levels)
        assertEquals(listOf(0, 30), shrunk.mix.pans)
        assertEquals(2, shrunk.mix.sourceChannels)
    }

    @Test
    fun aRepeatedConfigLeavesTheStripsAlone() {
        val set = withChannels(2).after(ChannelLevelChanged(1, 70), ChannelSoloChanged(0, true))

        assertEquals(set.mix, set.after(ConfigReceived(2, emptyList())).mix)
    }

    @Test
    fun announcedGroupsTagTheirChannelsAndKeepTheirLevels() {
        val groups = listOf(GroupInfo("Drums", listOf(0, 1)), GroupInfo("Keys", listOf(2)))
        val state = withChannels(4, groups).after(GroupLevelChanged(1, 60))

        assertEquals(
            listOf(GroupTag(0, "Drums"), GroupTag(0, "Drums"), GroupTag(1, "Keys"), null),
            state.mix.channels.map { it.group },
        )
        assertEquals(listOf(GroupUiState(0, "Drums", listOf(0, 1), 100), GroupUiState(1, "Keys", listOf(2), 60)), state.mix.groups)

        val reannounced = state.after(ConfigReceived(4, groups + GroupInfo("Vox", listOf(3))))

        assertEquals(listOf(100, 60, 100), reannounced.mix.groupLevels)
        assertEquals(GroupTag(2, "Vox"), reannounced.mix.channels[3].group)
    }

    @Test
    fun aConfigThatDropsTheMusiciansChannelTurnsMoreOfMeOff() {
        val on = withChannels(4).after(MusicianChannelChosen(3), MoreOfMeToggled(true))

        val dropped = on.after(ConfigReceived(2, emptyList()))

        assertNull(dropped.moreOfMe.musicianChannel)
        assertFalse(dropped.moreOfMe.enabled)
        assertEquals(MoreOfMeNotice.CHANNEL_OUT_OF_RANGE, dropped.moreOfMe.notice)
    }

    @Test
    fun aRememberedChannelOutsideTheFirstConfigIsForgottenQuietly() {
        val remembered = MonitorUiState(moreOfMe = MoreOfMeUiState(musicianChannel = 6))

        val state = remembered.after(ConfigReceived(4, emptyList()))

        assertNull(state.moreOfMe.musicianChannel)
        assertNull(state.moreOfMe.notice)
    }

    @Test
    fun aRememberedChannelInsideTheConfigIsKept() {
        val remembered = MonitorUiState(moreOfMe = MoreOfMeUiState(musicianChannel = 3))

        assertEquals(3, remembered.after(ConfigReceived(4, emptyList())).moreOfMe.musicianChannel)
    }

    // The mix.

    @Test
    fun channelControlsChangeOnlyTheirChannel() {
        val state = withChannels(3).after(
            ChannelLevelChanged(1, 25),
            ChannelPanChanged(1, 50),
            ChannelMuteChanged(1, true),
            ChannelSoloChanged(2, true),
        )

        assertEquals(ChannelUiState(1, "CH 2", null, 25, 50, muted = true, soloed = false), state.mix.channels[1])
        assertEquals(ChannelUiState(0, "CH 1", null, 100, 0, muted = false, soloed = false), state.mix.channels[0])
        assertTrue(state.mix.channels[2].soloed)
        assertEquals(listOf(100, 25, 100), state.mix.levels)
    }

    @Test
    fun levelsAndPansStayBetweenZeroAndAHundred() {
        val state = withChannels(1, listOf(GroupInfo("Band", listOf(0)))).after(
            ChannelLevelChanged(0, 140),
            ChannelPanChanged(0, -5),
            GroupLevelChanged(0, 101),
            MasterVolumeChanged(120),
            LevelCapChanged(-1),
        )

        assertEquals(100, state.mix.levels[0])
        assertEquals(0, state.mix.pans[0])
        assertEquals(100, state.mix.groupLevels[0])
        assertEquals(100, state.mix.masterVolume)
        assertEquals(0, state.mix.levelCap)
    }

    @Test
    fun aControlForAChannelOrGroupThatDoesNotExistChangesNothing() {
        val state = withChannels(2)

        assertEquals(
            state,
            state.after(
                ChannelLevelChanged(5, 10),
                ChannelMuteChanged(-1, true),
                ChannelPanChanged(2, 10),
                ChannelSoloChanged(9, true),
                GroupLevelChanged(0, 50),
            ),
        )
    }

    @Test
    fun aGroupLevelFillsTheOtherGroupsAsTheyRead() {
        val groups = listOf(GroupInfo("A", listOf(0)), GroupInfo("B", listOf(1)), GroupInfo("C", listOf(1)))
        val recalledShort = withChannels(2).after(MixRecalled("Old", MixSnapshot(channelGains = intArrayOf(100, 100), groupLevels = intArrayOf(20))))
        val announced = recalledShort.after(ConfigReceived(2, groups))

        val state = announced.after(GroupLevelChanged(2, 70))

        assertEquals(listOf(20, 100, 70), state.mix.groupLevels)
    }

    @Test
    fun masterControlsChangeTheMaster() {
        val state = withChannels(2).after(MasterVolumeChanged(80), MasterMuteChanged(true), LevelCapChanged(90))

        assertEquals(80, state.mix.masterVolume)
        assertTrue(state.mix.masterMuted)
        assertEquals(90, state.mix.levelCap)
    }

    @Test
    fun theMixBecomesTheSnapshotTheControlChannelSends() {
        val state = withChannels(2, listOf(GroupInfo("Band", listOf(0, 1)))).after(
            ChannelLevelChanged(0, 30),
            ChannelSoloChanged(1, true),
            GroupLevelChanged(0, 70),
            MasterVolumeChanged(60),
            LevelCapChanged(85),
            MasterMuteChanged(true),
        )

        val snapshot = state.mix.toSnapshot()

        assertArrayEquals(intArrayOf(30, 100), snapshot.channelGains)
        assertArrayEquals(intArrayOf(0, 100), snapshot.channelPans)
        assertArrayEquals(booleanArrayOf(false, false), snapshot.channelMutes)
        assertArrayEquals(booleanArrayOf(false, true), snapshot.channelSolos)
        assertArrayEquals(intArrayOf(70), snapshot.groupLevels)
        assertEquals(60, snapshot.volumePercent)
        assertEquals(85, snapshot.maxLevelPercent)
        assertTrue(snapshot.muted)
    }

    // More of me.

    @Test
    fun moreOfMeNeedsAChannelBeforeItTurnsOn() {
        val state = withChannels(2).after(MoreOfMeToggled(true))

        assertFalse(state.moreOfMe.enabled)
        assertEquals(MoreOfMeNotice.NEEDS_CHANNEL, state.moreOfMe.notice)
    }

    @Test
    fun moreOfMeTurnsOnAndOffOnceAChannelIsChosen() {
        val on = withChannels(2).after(MoreOfMeToggled(true), MusicianChannelChosen(1), MoreOfMeToggled(true))

        assertTrue(on.moreOfMe.enabled)
        assertNull(on.moreOfMe.notice)
        assertEquals("CH 2", on.moreOfMe.channelLabel)
        assertFalse(on.after(MoreOfMeToggled(false)).moreOfMe.enabled)
    }

    @Test
    fun moreOfMeIsNeverWrittenIntoTheMix() {
        val on = withChannels(2).after(ChannelLevelChanged(0, 80), MusicianChannelChosen(1), MoreOfMeToggled(true))

        assertEquals(listOf(80, 100), on.mix.levels)
    }

    @Test
    fun choosingNoChannelTurnsMoreOfMeOff() {
        val off = withChannels(2).after(MusicianChannelChosen(0), MoreOfMeToggled(true), MusicianChannelChosen(null))

        assertNull(off.moreOfMe.musicianChannel)
        assertFalse(off.moreOfMe.enabled)
        assertEquals(MoreOfMeNotice.NO_CHANNEL_SELECTED, off.moreOfMe.notice)
    }

    @Test
    fun choosingAnotherChannelKeepsMoreOfMeOn() {
        val moved = withChannels(3).after(MusicianChannelChosen(0), MoreOfMeToggled(true), MusicianChannelChosen(2))

        assertTrue(moved.moreOfMe.enabled)
        assertEquals(2, moved.moreOfMe.musicianChannel)
    }

    @Test
    fun onlyAnAnnouncedChannelCanBeChosen() {
        val state = withChannels(2).after(MusicianChannelChosen(0))

        assertEquals(state, state.after(MusicianChannelChosen(2), MusicianChannelChosen(-1)))
        assertEquals(idle, idle.after(MusicianChannelChosen(0)))
    }

    // A mix the server pushed.

    /** A session whose server announced [count] channels and [groups]. */
    private fun connectedWith(count: Int, groups: List<GroupInfo> = emptyList()): MonitorUiState =
        running().after(ConfigReceived(count, groups))

    /** The server's whole mix, as `mix_state` carries it: full 32-channel and 16-group tables. */
    private val serverMix = MixSnapshot(
        channelGains = IntArray(32) { if (it < 2) 80 - 10 * it else 100 },
        channelPans = IntArray(32) { if (it == 0) 25 else 100 },
        channelMutes = BooleanArray(32) { it == 0 },
        channelSolos = BooleanArray(32) { it == 1 },
        groupLevels = IntArray(16) { if (it == 0) 60 else 100 },
        volumePercent = 50,
        maxLevelPercent = 90,
        muted = true,
    )

    @Test
    fun aPushedMixTakesTheServersValuesForTheAnnouncedChannelsAndGroups() {
        val groups = listOf(GroupInfo("Band", listOf(0, 1)))
        val state = connectedWith(2, groups)

        val pushed = state.after(MixPushed(serverMix))

        assertEquals(2, pushed.mix.sourceChannels)
        assertEquals(listOf(80, 70), pushed.mix.levels)
        assertEquals(listOf(25, 100), pushed.mix.pans)
        assertEquals(listOf(true, false), pushed.mix.mutes)
        assertEquals(listOf(false, true), pushed.mix.solos)
        assertEquals(groups, pushed.mix.groupDefinitions)
        assertEquals(listOf(60), pushed.mix.groupLevels)
        assertEquals(50, pushed.mix.masterVolume)
        assertEquals(90, pushed.mix.levelCap)
        assertTrue(pushed.mix.masterMuted)
    }

    @Test
    fun aPushedMixIsClampedAndKeepsWhatTheServerDidNotSend() {
        val state = connectedWith(2, listOf(GroupInfo("Band", listOf(0, 1)))).after(ChannelLevelChanged(1, 40))

        val pushed = state.after(
            MixPushed(MixSnapshot(channelGains = intArrayOf(140), channelPans = intArrayOf(-5), volumePercent = 120, maxLevelPercent = -1)),
        )

        assertEquals(listOf(100, 40), pushed.mix.levels)
        assertEquals(listOf(0, 100), pushed.mix.pans)
        assertEquals(listOf(100), pushed.mix.groupLevels)
        assertEquals(100, pushed.mix.masterVolume)
        assertEquals(0, pushed.mix.levelCap)
    }

    @Test
    fun aPushedMixBeforeTheFirstConfigKeepsTheStripsThePhoneShows() {
        val recalled = running().after(MixRecalled("Old", MixSnapshot(channelGains = intArrayOf(30, 30, 30), groupLevels = intArrayOf(20))))

        val pushed = recalled.after(MixPushed(serverMix))

        assertEquals(listOf(80, 70, 100), pushed.mix.levels)
        assertEquals(listOf(60), pushed.mix.groupLevels)
    }

    @Test
    fun aPushedMixChangesNothingOutsideASession() {
        val idleWithChannels = withChannels(2)
        val ended = connectedWith(2).after(SessionEnded)

        assertEquals(idleWithChannels, idleWithChannels.after(MixPushed(serverMix)))
        assertEquals(ended, ended.after(MixPushed(serverMix)))
    }

    @Test
    fun aPushedMixTurnsMoreOfMeOffAndKeepsTheMixAsTheServerPlaysIt() {
        val on = connectedWith(2).after(MusicianChannelChosen(1), MoreOfMeToggled(true))

        val pushed = on.after(MixPushed(serverMix))

        assertFalse(pushed.moreOfMe.enabled)
        assertEquals(1, pushed.moreOfMe.musicianChannel)
        assertEquals(MoreOfMeNotice.MIX_CHANGED_ELSEWHERE, pushed.moreOfMe.notice)
        assertEquals(listOf(80, 70), pushed.mix.levels)
    }

    @Test
    fun aPushOfTheMixThePhoneAlreadyPlaysChangesNothing() {
        val plain = connectedWith(2).after(ChannelLevelChanged(0, 80))
        val lifted = plain.after(MusicianChannelChosen(1), MoreOfMeToggled(true))

        assertEquals(plain, plain.after(MixPushed(plain.mix.toSnapshot())))
        // With More of me on the server plays the lifted mix; hearing it back keeps More of me on.
        assertEquals(lifted, lifted.after(MixPushed(lifted.mix.toSnapshot().moreOfMe(1)!!)))
    }

    @Test
    fun aPushedMixLeavesTheSavedMixesAsTheyWere() {
        val recalled = connectedWith(2).after(
            MixesLoaded(listOf("Verse")),
            MixRecalled("Verse", MixSnapshot(channelGains = intArrayOf(30, 30))),
        )

        assertEquals(recalled.savedMixes, recalled.after(MixPushed(serverMix)).savedMixes)
    }

    // Saved mixes.

    @Test
    fun loadedMixesAreListedInStoredOrder() {
        val state = idle.after(MixesLoaded(listOf("Verse", "Chorus")))

        assertEquals(listOf(SavedMixUiState("Verse", false), SavedMixUiState("Chorus", false)), state.savedMixes.mixes)
        assertNull(state.savedMixes.emptyMessage)
    }

    @Test
    fun theEmptyStateShowsWhenNothingIsSaved() {
        assertEquals("No saved mixes yet", idle.after(MixesLoaded(emptyList())).savedMixes.emptyMessage)
    }

    @Test
    fun theEmptyStateNeverShowsWhileOneMixIsSaved() {
        val oneSaved = listOf(
            idle.after(MixesLoaded(listOf("Verse"))),
            idle.after(MixesLoaded(emptyList()), MixSaved("Verse", listOf("Verse"))),
            idle.after(MixesLoaded(listOf("Verse", "Chorus")), MixDeleted("Chorus", listOf("Verse"))),
            idle.after(MixesLoaded(listOf("Verse")), MixNotice("Enter a name for the mix")),
            idle.after(MixesLoaded(listOf("Old")), MixRenamed("Old", "Verse", listOf("Verse"))),
            idle.after(MixesLoaded(listOf("Gone", "Verse")), MixMissing("Gone", listOf("Verse"))),
            idle.after(MixesLoaded(listOf("Verse")), MixRecalled("Verse", MixSnapshot())),
        )

        for (state in oneSaved) {
            assertNull("the empty state shows next to a saved mix", state.savedMixes.emptyMessage)
            assertEquals(listOf("Verse"), state.savedMixes.mixes.map { it.name })
        }
    }

    @Test
    fun deletingTheLastMixShowsTheEmptyState() {
        val state = idle.after(MixesLoaded(listOf("Verse")), MixDeleted("Verse", emptyList()))

        assertEquals("No saved mixes yet", state.savedMixes.emptyMessage)
        assertEquals("Deleted \"Verse\"", state.savedMixes.notice)
    }

    @Test
    fun aSavedMixBecomesTheActiveOne() {
        val state = idle.after(MixesLoaded(listOf("Chorus")), MixSaved("Verse", listOf("Chorus", "Verse")))

        assertEquals(listOf(SavedMixUiState("Chorus", false), SavedMixUiState("Verse", true)), state.savedMixes.mixes)
        assertEquals("Saved \"Verse\"", state.savedMixes.notice)
    }

    @Test
    fun recallingAMixAppliesItsValuesAndKeepsWhatTheServerAnnounced() {
        val groups = listOf(GroupInfo("Band", listOf(0, 1)))
        val live = withChannels(2, groups).after(MusicianChannelChosen(1), MixesLoaded(listOf("Verse")))
        val snapshot = MixSnapshot(
            channelGains = intArrayOf(30, 60),
            channelPans = intArrayOf(20, 80),
            channelMutes = booleanArrayOf(true, false),
            channelSolos = booleanArrayOf(false, true),
            groupLevels = intArrayOf(40),
            volumePercent = 70,
            maxLevelPercent = 90,
            muted = true,
        )

        val recalled = live.after(MixRecalled("Verse", snapshot))

        assertEquals(listOf(30, 60), recalled.mix.levels)
        assertEquals(listOf(20, 80), recalled.mix.pans)
        assertEquals(listOf(true, false), recalled.mix.mutes)
        assertEquals(listOf(false, true), recalled.mix.solos)
        assertEquals(listOf(40), recalled.mix.groupLevels)
        assertEquals(70, recalled.mix.masterVolume)
        assertEquals(90, recalled.mix.levelCap)
        assertTrue(recalled.mix.masterMuted)
        assertEquals(2, recalled.mix.sourceChannels)
        assertEquals(groups, recalled.mix.groupDefinitions)
        assertEquals(1, recalled.moreOfMe.musicianChannel)
        assertTrue(recalled.savedMixes.mixes.single().active)
        assertEquals("Recalled \"Verse\"", recalled.savedMixes.notice)
    }

    @Test
    fun aMixRecalledBeforeTheServerAnnouncesKeepsItsGroupLevels() {
        val recalled = idle.after(
            MixRecalled("Verse", MixSnapshot(channelGains = intArrayOf(50, 50), groupLevels = intArrayOf(30, 40))),
        )

        val announced = recalled.after(ConfigReceived(2, listOf(GroupInfo("A", listOf(0)), GroupInfo("B", listOf(1)))))

        assertEquals(listOf(30, 40), announced.mix.groups.map { it.level })
        assertEquals(listOf(50, 50), announced.mix.levels)
    }

    @Test
    fun aRecalledMixWithAnotherChannelCountShowsItsStripsUntilTheServerAnnouncesAgain() {
        val recalled = withChannels(2).after(
            MixRecalled("Big", MixSnapshot(channelGains = intArrayOf(10, 20, 30, 40))),
        )

        assertEquals(listOf(10, 20, 30, 40), recalled.mix.levels)
        assertEquals(listOf(0, 100, 0, 100), recalled.mix.pans)

        val reannounced = recalled.after(ConfigReceived(2, emptyList()))

        assertEquals(listOf(10, 20), reannounced.mix.levels)
    }

    @Test
    fun deletingTheActiveMixLeavesNoneActive() {
        val state = idle.after(
            MixesLoaded(listOf("Verse", "Chorus")),
            MixRecalled("Verse", MixSnapshot()),
            MixDeleted("Verse", listOf("Chorus")),
        )

        assertNull(state.savedMixes.activeName)
        assertEquals(listOf(SavedMixUiState("Chorus", false)), state.savedMixes.mixes)
    }

    @Test
    fun renamingTheActiveMixKeepsItActive() {
        val state = idle.after(
            MixesLoaded(listOf("Verse", "Chorus")),
            MixSaved("Verse", listOf("Chorus", "Verse")),
            MixRenamed("Verse", "Bridge", listOf("Chorus", "Bridge")),
        )

        assertEquals(listOf(SavedMixUiState("Chorus", false), SavedMixUiState("Bridge", true)), state.savedMixes.mixes)
        assertEquals("Renamed \"Verse\" to \"Bridge\"", state.savedMixes.notice)
    }

    @Test
    fun aMissingMixIsReportedAndTheListRefreshed() {
        val state = idle.after(
            MixesLoaded(listOf("Verse")),
            MixRecalled("Verse", MixSnapshot()),
            MixMissing("Verse", emptyList()),
        )

        assertEquals("\"Verse\" is no longer saved", state.savedMixes.notice)
        assertNull(state.savedMixes.activeName)
        assertEquals("No saved mixes yet", state.savedMixes.emptyMessage)
    }

    @Test
    fun aReloadThatLosesTheActiveMixLeavesNoneActive() {
        val state = idle.after(MixesLoaded(listOf("Verse")), MixSaved("Verse", listOf("Verse")), MixesLoaded(listOf("Chorus")))

        assertNull(state.savedMixes.activeName)
    }

    @Test
    fun aRefusedSaveSaysWhy() {
        val state = idle.after(MixesLoaded(listOf("Verse")), MixNotice("Enter a name for the mix"))

        assertEquals("Enter a name for the mix", state.savedMixes.notice)
        assertEquals(listOf("Verse"), state.savedMixes.names)
    }

    // Diagnostics.

    @Test
    fun statsFillTheDiagnostics() {
        val diagnostics = running().after(StatsUpdated(stats(packets = 990, lost = 10))).diagnostics

        assertEquals(990L, diagnostics.packetsReceived)
        assertEquals(10L, diagnostics.packetsLost)
        assertEquals(1.0, diagnostics.lossPercent, 1e-9)
        assertEquals(48_000, diagnostics.sampleRateHz)
        assertEquals(100.0, diagnostics.packetsPerSecond, 0.0)
        assertEquals(10.0, diagnostics.interArrivalMs, 0.0)
        assertEquals(0.4, diagnostics.jitterMs, 0.0)
        assertEquals(12.5, diagnostics.playbackBufferMs, 0.0)
        assertEquals(2L, diagnostics.discardedSamples)
        assertEquals(1L, diagnostics.packetsWithDiscardedSamples)
    }

    @Test
    fun lossIsZeroBeforeAnyPacket() {
        assertEquals(0.0, idle.diagnostics.lossPercent, 0.0)
    }

    @Test
    fun linkReportsSetTheLinkHealthAndAddUpUnderruns() {
        val warned = running().after(LinkStatsReported(report(underruns = 1)))
        val recovered = warned.after(LinkStatsReported(report(underruns = 0)))
        val silent = recovered.after(LinkStatsReported(report(received = 0)))

        assertEquals(LinkHealth.WARNING, warned.diagnostics.linkHealth)
        assertEquals(LinkHealth.GOOD, recovered.diagnostics.linkHealth)
        assertEquals(1L, recovered.diagnostics.underruns)
        assertEquals(LinkHealth.SILENT, silent.diagnostics.linkHealth)
    }

    @Test
    fun errorsAreKeptPerSource() {
        val state = running().after(AudioError("UDP socket error: closed"), ControlError("WebSocket rejected mix update"))

        assertEquals("UDP socket error: closed", state.diagnostics.lastError)
        assertEquals("WebSocket rejected mix update", state.diagnostics.controlError)
    }

    // The rest.

    @Test
    fun theNameIsKeptAsTyped() {
        assertEquals(" Ana ", idle.after(NameChanged(" Ana ")).musicianName)
    }

    @Test
    fun selectingATabShowsIt() {
        assertEquals(MainTab.MIXES, idle.after(TabSelected(MainTab.MIXES)).tab)
    }
}
