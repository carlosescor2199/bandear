import BandEarDiscovery
import BandEarPlayback
import BandEarProtocol
import Foundation

/// The phone's audio receiver as ``MonitorAudio`` drives it: ``AudioReceiver``, or a fake in tests.
///
/// Android's `AudioReceiver` seam over `PmonPlayer` (`MonitorCollaborators.kt:25-34`). A launch
/// takes only the port: the receiver takes datagrams from any sender, so it needs no host.
public protocol AudioReceiving: AnyObject {
    /// True from ``start(port:)`` until the latest launch's loop has ended and reported its stop.
    var isAlive: Bool { get }

    /// Launches a receiver on `port`. Called only while ``isAlive`` is false, since an older loop
    /// may still hold the port.
    func start(port: UInt16)

    /// Asks the latest launch to end; it reports ``ReceiverListener/onStopped()`` once it has.
    func stop()
}

extension AudioReceiver: AudioReceiving {}

/// Why ``MonitorAudio/start(_:musicianName:)`` started nothing. The words are Android's
/// (`MonitorViewModel.kt:241`), for the connect screen to show.
public enum MonitorAudioError: Error, Equatable, Sendable, CustomStringConvertible {
    /// The last session's receiver has not finished stopping, so its loop may still hold the port.
    case previousReceiverStopping

    public var description: String {
        switch self {
        case .previousReceiverStopping: "The previous receiver is still stopping"
        }
    }
}

/// The audio side of a session: the receiver that plays the desk's mix, started and stopped with the
/// ``ControlSession``, and playing by what the session holds.
///
/// Ported from what the Android client's `MonitorViewModel.kt` does around its `PmonPlayer`:
/// - Whenever the session changes, the ``PlayerControls`` the receive loop reads follow it
///   (`syncPlayerMix`, `MonitorViewModel.kt:401-409`, and the buffer and hold listeners,
///   `:177-185`): the master volume and mute, the cap (``MixState/effectiveCap``, the lower of the
///   musician's cap and the engineer's limit, which only the phone plays by and never sends),
///   whether the desk owns the mix (``ControlSession/mixHeldByDesk``, Android's
///   `mixAcknowledged`), the desk's buffer and its hold for delayed copies. Nothing here computes
///   the mix: each value is the session's.
/// - ``start(_:musicianName:)`` starts the session, then the receiver on the session's audio port
///   (`MonitorViewModel.kt:238-263`). A new session starts on Auto with no hold and the mix not
///   held by the desk, as the session does, so its receiver's first wake-up reads exactly that.
/// - ``stop()`` stops the receiver, then the session (`:272-281`). A session stopped some other
///   way takes its receiver with it too.
/// - A receiver that ends on its own (its socket failed) ends the session (`:287-305`), so the
///   musician is not left connected to a desk they cannot hear. A lost control connection does not
///   stop the receiver: the session reconnects while the audio goes on.
/// - Each link report goes to the desk through ``ControlSession/send(stats:)``, which sends only
///   while the connection is open, as ``LinkStats/init(_:)`` builds it (`:149-152`).
///
/// A receiver stays alive until its loop has reported its stop, and the receive loop holds the
/// audio port until then, so a session cannot start while the previous receiver is alive: the
/// start is refused with ``MonitorAudioError/previousReceiverStopping``, as Android refuses it, and
/// the musician connects again a moment later. The receiver reports one stop per launch, in launch
/// order, so the n-th report belongs to the n-th session, as Android counts them
/// (`MonitorViewModel.kt:96-109,287-297`): a report only ends the latest session, and only when
/// nothing ended it already, so a late report can never end a session twice or end the next one.
///
/// Everything happens on the main actor. The receiver reports from its receive thread, and every
/// report is handed to the main queue in the order it came, without waiting for it, so the receive
/// thread never blocks on the main thread and the state here is only touched on the main actor.
@MainActor
public final class MonitorAudio {
    /// Builds the receiver from the output's factory, the controls it plays by and the listener it
    /// reports to.
    public typealias ReceiverFactory =
        (any PlaybackSinkFactory, PlayerControls, any ReceiverListener) -> any AudioReceiving

    /// The control session the audio follows. The mix is edited through it
    /// (``ControlSession/apply(_:)``); start and stop the session here, not on it.
    public let session: ControlSession
    /// The receiver's latest metrics, or nil before the session's first packet played; a new
    /// session starts without any, as Android's diagnostics start again (`MonitorReducer.kt:70-72`).
    public private(set) var receiverStats: ReceiverStats?
    /// The latest link report the receiver made, as sent to the desk, or nil before the first one
    /// and once the session ended, when its link is no longer known (Android's `linkHealth` goes
    /// back to unknown then, `MonitorReducer.kt:78-81`).
    public private(set) var linkReport: LinkStatsReport?
    /// Why the receiver last refused a packet, failed to open the output, or lost its socket, or nil
    /// since the session started.
    public private(set) var receiverError: (any Error)?
    /// Called on the main actor after anything above, or anything the session holds, may have
    /// changed. Observe this, not the session's own ``ControlSession/stateDidChange``, which the
    /// coordinator takes over.
    public var stateDidChange: (() -> Void)?

    private let controls: PlayerControls
    private let receiver: any AudioReceiving
    /// Whether a session with its receiver runs: from a start until a stop, the session stopping,
    /// or the receiver ending on its own (Android's `running`).
    private var running = false
    /// Receivers launched so far; the current (or last) session is number `launched`.
    private var launched = 0
    /// Stop reports received so far.
    private var stopsReported = 0
    /// The last session ended, by a stop or by its receiver; it is never ended again.
    private var lastEnded = 0

    /// A coordinator for `session`, whose receiver plays through the outputs `sinkFactory` opens.
    /// `makeReceiver` builds the receiver, once, here; by default an ``AudioReceiver`` on a UDP
    /// socket.
    ///
    /// The coordinator takes over the session's ``ControlSession/stateDidChange``. Keep it for as
    /// long as the session, and ``stop()`` it before letting it go: a receiver still running keeps
    /// its own thread, and nothing else would stop it.
    public init(
        session: ControlSession, sinkFactory: any PlaybackSinkFactory,
        makeReceiver: ReceiverFactory = {
            AudioReceiver(sinkFactory: $0, controls: $1, listener: $2)
        }
    ) {
        self.session = session
        let controls = PlayerControls()
        let events = ReceiverEvents()
        self.controls = controls
        receiver = makeReceiver(sinkFactory, controls, events)
        events.owner = self
        session.stateDidChange = { [weak self] in self?.sessionDidChange() }
        followSession()
    }

    /// Starts a session with the desk `request` names (``ControlSession/start(_:musicianName:)``),
    /// then the receiver on `request`'s audio port.
    ///
    /// Does nothing while a session runs, as Android ignores a second Connect: stop it first.
    /// Throws, and starts nothing, while the previous session's receiver is still stopping
    /// (``MonitorAudioError/previousReceiverStopping``), or for a name the desk would store as no
    /// name (``ClientMessageError/nameIsBlank``).
    public func start(_ request: ConnectionRequest, musicianName: String) throws {
        guard !running else { return }
        guard !receiver.isAlive else { throw MonitorAudioError.previousReceiverStopping }
        // The session resets the desk's settings as it starts, and the controls follow it at once,
        // so the receiver launched below never plays by the last desk's buffer or hold.
        try session.start(request, musicianName: musicianName)
        running = true
        launched += 1
        receiverStats = nil
        linkReport = nil
        receiverError = nil
        receiver.start(port: request.audioPort)
        stateDidChange?()
    }

    /// Ends the session, if one runs: the receiver is asked to stop, then the session stops. The
    /// receiver's stop report, when it comes, ends nothing more.
    public func stop() {
        guard running else { return }
        endSession(stoppingTheReceiver: true)
        session.stop()
    }

    /// The session changed: the controls follow it, and a session that ended without ``stop()``
    /// takes its receiver with it.
    private func sessionDidChange() {
        followSession()
        if running, session.connection == .idle || !session.mixSession.isSessionRunning {
            endSession(stoppingTheReceiver: true)
        }
        stateDidChange?()
    }

    /// Copies what the receive loop plays by from the session, all in one step, so the loop never
    /// reads half a change.
    private func followSession() {
        let mix = session.mixSession.mix
        let serverOwnsMix = session.mixHeldByDesk
        let playbackBuffer = session.playbackBuffer
        let redundancyHoldPackets = Int(session.redundancyHoldBlocks)
        controls.update { settings in
            settings.volumePercent = mix.masterVolume
            // The lower of the phone's cap and the engineer's limit, as the desk plays it.
            settings.maxLevelPercent = mix.effectiveCap
            settings.muted = mix.masterMuted
            settings.serverOwnsMix = serverOwnsMix
            settings.playbackBuffer = playbackBuffer
            settings.redundancyHoldPackets = redundancyHoldPackets
        }
    }

    /// The running session is over: no report can end it again, and its link is no longer known.
    private func endSession(stoppingTheReceiver: Bool) {
        running = false
        lastEnded = launched
        linkReport = nil
        if stoppingTheReceiver { receiver.stop() }
    }

    /// One report from the receiver, on the main actor, in the order the receiver made them.
    fileprivate func handle(_ event: ReceiverEvent) {
        switch event {
        case .stats(let stats):
            receiverStats = stats
        case .linkStats(let report):
            session.send(stats: LinkStats(report))
            linkReport = report
        case .failed(let failure):
            receiverError = failure.error
        case .stopped:
            receiverStopped()
            return
        }
        stateDidChange?()
    }

    /// A receiver ended, on request or on its own, and reported it. Only the latest session's report
    /// ends a session, and only once: a report for a session already ended (by a stop, or one
    /// replaced by a newer session) does nothing (`MonitorViewModel.kt:287-297`).
    private func receiverStopped() {
        // The real receiver never reports more stops than launches; a stray report counts as the
        // latest session's, so the counts cannot drift apart.
        stopsReported = min(stopsReported + 1, launched)
        guard stopsReported == launched, stopsReported != lastEnded else { return }
        // The latest session's receiver ended on its own: the session ends with it.
        endSession(stoppingTheReceiver: false)
        session.stop()
    }
}

/// What the receiver reports, as it crosses from the receive thread to the main actor.
private enum ReceiverEvent: Sendable {
    case stats(ReceiverStats)
    case linkStats(LinkStatsReport)
    case failed(ReceiverFailure)
    case stopped
}

/// An error the receive thread hands the main actor. The receive thread lets go of it once it is
/// reported and never touches it again, so only the main actor reads it from then on.
private struct ReceiverFailure: @unchecked Sendable {
    let error: any Error
}

/// The receiver's listener: hands each report to the main queue in the order it came and returns at
/// once, so the receive thread, which also feeds the output, never waits for the main thread.
///
/// The main queue runs blocks in the order they were queued, so the reports keep their order, the
/// stop report last of its launch.
private final class ReceiverEvents: ReceiverListener, @unchecked Sendable {
    /// Set once on the main actor, while the coordinator is made and before any receiver starts;
    /// only read after that.
    weak var owner: MonitorAudio?

    func onStats(_ stats: ReceiverStats) {
        deliver(.stats(stats))
    }

    func onLinkStats(_ report: LinkStatsReport) {
        deliver(.linkStats(report))
    }

    func onError(_ error: any Error) {
        deliver(.failed(ReceiverFailure(error: error)))
    }

    func onStopped() {
        deliver(.stopped)
    }

    private func deliver(_ event: ReceiverEvent) {
        DispatchQueue.main.async { [weak owner = self.owner] in
            MainActor.assumeIsolated { owner?.handle(event) }
        }
    }
}
