package com.bandear.android.ui.state

import android.content.Context
import com.bandear.android.ClientIdentity
import com.bandear.android.ClientMixState
import com.bandear.android.ControlScheduler
import com.bandear.android.ControlTransport
import com.bandear.android.ExecutorControlScheduler
import com.bandear.android.MixBank
import com.bandear.android.MixBankStore
import com.bandear.android.MixSnapshot
import com.bandear.android.OkHttpControlTransport
import com.bandear.android.PmonPlayer
import com.bandear.android.ServerDiscovery
import java.util.concurrent.atomic.AtomicBoolean

/*
 * The seams between [MonitorViewModel] and the classes that touch the network, the audio hardware
 * and the phone's storage. Production wraps the existing classes, unchanged; the JVM tests hand in
 * fakes. The control channel itself is not wrapped: it already takes its transport and scheduler as
 * seams, so the ViewModel builds the real channel in both.
 */

/** The audio receiver of a session; [PmonPlayer] in production. */
internal interface AudioReceiver {
    /** True while the previous session's receiver thread is still ending. */
    fun isAlive(): Boolean

    /** Starts receiving from [host] on the local UDP [port]. */
    fun launch(host: String, port: Int)

    /** Asks the receiver to end; it reports `onStopped` once it has. */
    fun stop()
}

/** Listens for server beacons while no session runs; [ServerDiscovery] in production. */
internal interface ServerScanner {
    fun start()

    /** Pauses listening; [start] resumes it. */
    fun stop()

    /** Stops listening for good. */
    fun shutdown()
}

/** Saved mixes, the musician channel and the musician's name on this phone; [MixBankStore] in production. */
internal interface MonitorStore {
    fun loadBanks(): List<MixBank>

    /** Saves under [name], replacing a mix with the same trimmed name; see [MixBankStore.saveBank]. */
    fun saveBank(name: String, snapshot: MixSnapshot)

    fun deleteBank(name: String): Boolean
    fun musicianChannel(): Int?
    fun setMusicianChannel(channel: Int?)
    fun musicianName(): String
    fun setMusicianName(name: String)
}

/**
 * What [MonitorViewModel] is built from. The receiver and the scanner are made by factories because
 * they report to listeners the ViewModel owns; the receiver also shares the session flag and the
 * mix state the audio thread reads.
 */
internal class MonitorCollaborators(
    val store: MonitorStore,
    /** This installation's client id; read when a session starts. */
    val clientId: () -> String,
    val receiverFactory: (PmonPlayer.Listener, AtomicBoolean, ClientMixState) -> AudioReceiver,
    val scannerFactory: (ServerDiscovery.Listener) -> ServerScanner,
    val controlTransport: ControlTransport,
    val controlScheduler: ControlScheduler,
) {
    companion object {
        /** The real network, audio and storage, exactly as `MainActivity` builds them. */
        fun production(context: Context): MonitorCollaborators {
            val appContext = context.applicationContext
            val identity = ClientIdentity(appContext)
            return MonitorCollaborators(
                store = BankStoreAdapter(MixBankStore(appContext)),
                clientId = { identity.clientId },
                receiverFactory = { listener, running, mixState ->
                    PlayerReceiver(PmonPlayer(listener, running, mixState))
                },
                scannerFactory = { listener -> DiscoveryScanner(ServerDiscovery(listener)) },
                controlTransport = OkHttpControlTransport(),
                controlScheduler = ExecutorControlScheduler(),
            )
        }
    }
}

private class PlayerReceiver(private val player: PmonPlayer) : AudioReceiver {
    override fun isAlive(): Boolean = player.isReceiverAlive()

    override fun launch(host: String, port: Int) = player.launch(host, port)

    override fun stop() = player.stopReceiving()
}

private class DiscoveryScanner(private val discovery: ServerDiscovery) : ServerScanner {
    override fun start() = discovery.start()

    override fun stop() = discovery.stop()

    override fun shutdown() = discovery.shutdown()
}

private class BankStoreAdapter(private val store: MixBankStore) : MonitorStore {
    override fun loadBanks(): List<MixBank> = store.loadBanks()

    override fun saveBank(name: String, snapshot: MixSnapshot) = store.saveBank(name, snapshot)

    override fun deleteBank(name: String): Boolean = store.deleteBank(name)

    override fun musicianChannel(): Int? = store.musicianChannel()

    override fun setMusicianChannel(channel: Int?) = store.setMusicianChannel(channel)

    override fun musicianName(): String = store.musicianName()

    override fun setMusicianName(name: String) = store.setMusicianName(name)
}
