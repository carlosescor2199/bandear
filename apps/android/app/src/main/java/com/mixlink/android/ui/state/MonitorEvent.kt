package com.mixlink.android.ui.state

import com.mixlink.android.ControlState
import com.mixlink.android.GroupInfo
import com.mixlink.android.LinkStatsReport
import com.mixlink.android.MixSnapshot
import com.mixlink.android.ui.components.MainTab

/** The receiver's running metrics, as one value; see `PmonPlayer.Listener.onStats`. */
internal data class ReceiverStats(
    val packets: Long,
    val lost: Long,
    val sampleRate: Int?,
    val packetsPerSecond: Double,
    val interArrivalMs: Double,
    val jitterMs: Double,
    val discardedSamples: Long,
    val packetsWithDiscardedSamples: Long,
    val playbackBacklogMs: Double,
)

/**
 * Everything the app reacts to, whether the musician did it or a collaborator reported it.
 *
 * Each event is a fact that already happened (or a request the reducer may refuse, such as turning
 * More of me on with no channel). [reduce] turns a state and an event into the next state and does
 * nothing else; the side effects belong to the ViewModel.
 */
internal sealed interface MonitorEvent {
    // The session and the control channel.

    /** A session started to [server], listening for audio on [audioPort]. */
    data class SessionStarted(val server: ServerEndpoint, val audioPort: Int) : MonitorEvent

    /** The session ended: the musician disconnected or the receiver stopped on its own. */
    data object SessionEnded : MonitorEvent

    /** Connect was refused before anything started, for the reason in [message]. */
    data class ConnectRejected(val message: String) : MonitorEvent

    /** The control channel is now in [state]; [mixAcknowledged] is whether the server took the mix. */
    data class ControlChanged(val state: ControlState, val mixAcknowledged: Boolean) : MonitorEvent

    /** The server acknowledged a mix while the control channel was in [state]. */
    data class MixAcknowledged(val state: ControlState) : MonitorEvent

    /** Discovery now hears [servers]; a server missing from the list was lost. */
    data class ServersChanged(val servers: List<ServerEndpoint>) : MonitorEvent

    /** The manual connection form now reads [address]. */
    data class ManualAddressEdited(val address: ManualAddress) : MonitorEvent

    /** The server announced [channels] source channels and [groups]. */
    data class ConfigReceived(val channels: Int, val groups: List<GroupInfo>) : MonitorEvent

    // The mix. Channel and group indices are 0-based; levels are percentages.

    data class ChannelLevelChanged(val channel: Int, val level: Int) : MonitorEvent
    data class ChannelPanChanged(val channel: Int, val pan: Int) : MonitorEvent
    data class ChannelMuteChanged(val channel: Int, val muted: Boolean) : MonitorEvent
    data class ChannelSoloChanged(val channel: Int, val soloed: Boolean) : MonitorEvent
    data class GroupLevelChanged(val group: Int, val level: Int) : MonitorEvent
    data class MasterVolumeChanged(val level: Int) : MonitorEvent
    data class MasterMuteChanged(val muted: Boolean) : MonitorEvent
    data class LevelCapChanged(val level: Int) : MonitorEvent

    // More of me.

    /** The musician asked to turn More of me [on] or off. */
    data class MoreOfMeToggled(val on: Boolean) : MonitorEvent

    /** The musician chose [channel] as theirs, or none. */
    data class MusicianChannelChosen(val channel: Int?) : MonitorEvent

    // Saved mixes. [names] is always the list as the phone now stores it.

    data class MixesLoaded(val names: List<String>) : MonitorEvent
    data class MixSaved(val name: String, val names: List<String>) : MonitorEvent
    data class MixRecalled(val name: String, val snapshot: MixSnapshot) : MonitorEvent

    /** A mix the musician asked for was no longer saved. */
    data class MixMissing(val name: String, val names: List<String>) : MonitorEvent
    data class MixDeleted(val name: String, val names: List<String>) : MonitorEvent
    data class MixRenamed(val from: String, val to: String, val names: List<String>) : MonitorEvent

    /** A save or rename was refused, for the reason in [message]. */
    data class MixNotice(val message: String) : MonitorEvent

    // Diagnostics.

    data class StatsUpdated(val stats: ReceiverStats) : MonitorEvent

    /** The receiver produced its once-a-second link report. */
    data class LinkStatsReported(val report: LinkStatsReport) : MonitorEvent

    /** The receiver reported a problem. */
    data class AudioError(val message: String) : MonitorEvent

    /** The control channel reported a problem. */
    data class ControlError(val message: String) : MonitorEvent

    // The rest.

    data class NameChanged(val name: String) : MonitorEvent
    data class TabSelected(val tab: MainTab) : MonitorEvent
}
