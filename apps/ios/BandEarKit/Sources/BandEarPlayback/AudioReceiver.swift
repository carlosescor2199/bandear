import Foundation

/// The phone's audio receiver: a thread of its own that opens the UDP socket and runs the
/// ``ReceiveLoop`` on it while the session flag is up.
///
/// The part of the Android client's `PmonPlayer` around its loop (`PmonPlayer.kt:108-122,153-357`):
/// - ``start(port:)`` raises the flag and launches a thread named `pmon-udp-receiver`, at the
///   highest quality of service, which opens the socket (``UDPDatagramSource``) and runs the loop.
/// - ``stop()`` lowers the flag, then wakes the socket with an empty datagram
///   (``DatagramSource/wake()``). The receive answers at once, the loop finds the flag down and
///   ends, and its socket is released, so the port can be bound again straight away. Android
///   closes the socket from the stopping thread instead, which wakes a Java `receive` but not a
///   Darwin `recv`.
/// - When a loop ends, on request or on its own, its thread closes the output, then the socket,
///   and then reports through ``ReceiverLaunches``: the flag goes down only if this was the latest
///   launch, and ``ReceiverListener/onStopped()`` comes once per launch.
///
/// A socket that cannot open, or that fails, is reported through
/// ``ReceiverListener/onError(_:)`` while the session still runs, and ends the loop.
///
/// Android also resolves the desk's host name before it opens the socket, only to report a name
/// that does not resolve; the socket takes datagrams from any sender, so the port is all this
/// receiver needs.
///
/// Thread-safe: ``start(port:)``, ``stop()`` and ``isAlive`` may be called from any thread. The
/// factory, controls, listener and clock are used on the receiver thread; a loop still finishing
/// may use them while a newer launch starts.
public final class AudioReceiver: @unchecked Sendable {
    /// The receiver thread's name, Android's.
    public static let threadName = "pmon-udp-receiver"

    private let sinkFactory: any PlaybackSinkFactory
    private let controls: PlayerControls
    private let listener: any ReceiverListener
    private let running: RunningFlag
    private let clock: any MonotonicClock
    private let openSource: (UInt16) throws -> any DatagramSource
    private let launches: ReceiverLaunches<Thread>

    private let lock = NSLock()
    /// The latest launch's socket, once its thread opened it, until its loop ended.
    private var source: (any DatagramSource)?

    /// A receiver that plays through outputs `sinkFactory` opens, by the `controls`, telling
    /// `listener`, while `running` is up. `running` is the session's flag, shared with whoever owns
    /// the session, as on Android; `openSource` opens the socket for a port.
    public init(
        sinkFactory: any PlaybackSinkFactory, controls: PlayerControls,
        listener: any ReceiverListener, running: RunningFlag = RunningFlag(),
        clock: any MonotonicClock = SystemMonotonicClock(),
        openSource: @escaping (UInt16) throws -> any DatagramSource = {
            try UDPDatagramSource(port: $0)
        }
    ) {
        self.sinkFactory = sinkFactory
        self.controls = controls
        self.listener = listener
        self.running = running
        self.clock = clock
        self.openSource = openSource
        launches = ReceiverLaunches(running: running)
    }

    /// True from ``start(port:)`` until the latest launch's loop has ended and reported its stop.
    public var isAlive: Bool { launches.current != nil }

    /// Raises the session flag and launches a receiver on `port` (0 lets the system choose).
    ///
    /// As on Android, a launch does not wait for an older one: call it again only once
    /// ``isAlive`` is false, or the older loop may still hold the port.
    public func start(port: UInt16) {
        running.value = true
        let thread = Thread { [self] in receive(port: port) }
        thread.name = Self.threadName
        thread.qualityOfService = .userInteractive
        launches.launched(thread)
        thread.start()
    }

    /// Lowers the session flag, then wakes the latest socket. Returns at once; the loop ends as soon
    /// as its receive answers, then reports ``ReceiverListener/onStopped()``. A socket not open yet
    /// needs no wake-up: its loop finds the flag down before it receives.
    public func stop() {
        running.value = false
        lock.lock()
        let current = source
        lock.unlock()
        current?.wake()
    }

    /// The receiver thread's body.
    private func receive(port: UInt16) {
        var opened: (any DatagramSource)?
        defer {
            // The socket is released here, on its own thread, after its last receive.
            if let opened {
                forget(opened)
                opened.close()
            }
            // The flag goes down (only if this is still the latest launch) before the receiver is
            // free again, so no new session can start in between and be ended by this loop.
            launches.finished(Thread.current) { listener.onStopped() }
        }
        do {
            let source = try openSource(port)
            opened = source
            remember(source)
            ReceiveLoop(
                source: source, sinkFactory: sinkFactory, controls: controls, listener: listener,
                clock: clock
            ).run(while: running)
        } catch {
            if running.value { listener.onError(error) }
        }
    }

    private func remember(_ opened: any DatagramSource) {
        lock.lock()
        defer { lock.unlock() }
        source = opened
    }

    private func forget(_ opened: any DatagramSource) {
        lock.lock()
        defer { lock.unlock() }
        if source === opened { source = nil }
    }
}
