package com.mixlink.android.ui.screens

import com.mixlink.android.ui.components.MainTab
import com.mixlink.android.ui.state.ManualAddress
import com.mixlink.android.ui.state.ServerEndpoint

/**
 * Everything the screens can ask for, in one place, so the app is bound to its state holder once.
 *
 * The names and parameters are the monitor ViewModel's intents, so the ViewModel can implement it
 * as it is. The screens only ever see this, never the ViewModel.
 */
interface MonitorActions {
    fun connect(server: ServerEndpoint)

    fun connectManual(host: String, controlPort: String, audioPort: String)

    fun editManualAddress(address: ManualAddress)

    fun disconnect()

    fun setChannelLevel(channel: Int, level: Int)

    fun setChannelMuted(channel: Int, muted: Boolean)

    fun setChannelSoloed(channel: Int, soloed: Boolean)

    fun setGroupLevel(group: Int, level: Int)

    fun setMasterVolume(level: Int)

    fun setMasterMuted(muted: Boolean)

    fun setLevelCap(level: Int)

    fun setMoreOfMe(on: Boolean)

    fun setMusicianChannel(channel: Int?)

    fun saveMix(name: String)

    fun recallMix(name: String)

    fun renameMix(from: String, to: String)

    fun deleteMix(name: String)

    fun setName(name: String)

    fun selectTab(tab: MainTab)
}
