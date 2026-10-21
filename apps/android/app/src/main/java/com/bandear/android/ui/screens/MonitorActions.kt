package com.bandear.android.ui.screens

import com.bandear.android.ui.components.MainTab
import com.bandear.android.ui.state.ManualAddress
import com.bandear.android.ui.state.ServerEndpoint

/**
 * Everything the screens can ask for, in one place, so the app is bound to its state holder once.
 *
 * The names and parameters are the monitor ViewModel's intents, and the ViewModel implements it.
 * The screens only ever see this, never the ViewModel.
 */
interface MonitorActions {
    fun connect(server: ServerEndpoint)

    fun connectManual(host: String, controlPort: String, audioPort: String)

    fun editManualAddress(address: ManualAddress)

    fun disconnect()

    fun setChannelLevel(channel: Int, level: Int)

    /** [pan] is 0 hard left, 50 centre, 100 hard right. */
    fun setChannelPan(channel: Int, pan: Int)

    fun setChannelMuted(channel: Int, muted: Boolean)

    fun setChannelSoloed(channel: Int, soloed: Boolean)

    fun setGroupLevel(group: Int, level: Int)

    fun setMasterVolume(level: Int)

    fun setMasterMuted(muted: Boolean)

    fun setLevelCap(level: Int)

    fun setMoreOfMe(on: Boolean)

    fun setMusicianChannel(channel: Int?)

    /** Saves the current mix as [name]; false when the name was refused (a notice says why). */
    fun saveMix(name: String): Boolean

    fun recallMix(name: String)

    fun renameMix(from: String, to: String)

    fun deleteMix(name: String)

    fun setName(name: String)

    fun selectTab(tab: MainTab)
}
