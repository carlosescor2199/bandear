import Foundation

/// The session flag every receive loop runs while: Android's shared `AtomicBoolean running`.
///
/// iOS 16 has no `Synchronization.Atomic`, so a lock guards the one value. The receive thread reads
/// it once per wake-up, which is not real time, so the lock costs nothing that matters.
public final class RunningFlag: @unchecked Sendable {
    private let lock = NSLock()
    private var raised: Bool

    public init(_ value: Bool = false) {
        raised = value
    }

    /// True while the session runs.
    public var value: Bool {
        get {
            lock.lock()
            defer { lock.unlock() }
            return raised
        }
        set {
            lock.lock()
            defer { lock.unlock() }
            raised = newValue
        }
    }
}

/// Which receive loop owns the shared session flag: the one launched last.
///
/// A port of the Android client's `ReceiverLaunches` (`PmonPlayer.kt:18-57`). Every launch's loop
/// runs while the ``RunningFlag`` is up, and a loop that ends lowers it, so a session whose
/// receiver dies on its own ends too. Only the latest launch may do that: a loop still finishing
/// after a newer launch leaves the flag, and so the newer session, alone.
///
/// A finishing loop lowers the flag, reports its stop, and only then stops being ``current``. A
/// caller that launches again only once no receiver is current therefore never finds the receiver
/// free with the old flag still to come down, and gets one stop report per launch, in launch order.
///
/// Launches are told apart by identity, as Android compares its threads with `===`.
final class ReceiverLaunches<Receiver: AnyObject>: @unchecked Sendable {
    private let lock = NSLock()
    private let running: RunningFlag
    private var latest: Receiver?

    init(running: RunningFlag) {
        self.running = running
    }

    /// The latest launch, until its loop has finished and reported; `nil` when none is running.
    var current: Receiver? {
        lock.lock()
        defer { lock.unlock() }
        return latest
    }

    /// `receiver` is the latest launch from now on; an older loop can no longer end the session.
    func launched(_ receiver: Receiver) {
        lock.lock()
        defer { lock.unlock() }
        latest = receiver
    }

    /// `receiver`'s loop ended. While it is still the latest launch it lowers the session flag; it
    /// reports through `report` either way, and then stops being ``current``, even when the report
    /// throws.
    func finished(_ receiver: Receiver, report: () throws -> Void) rethrows {
        lock.lock()
        if latest === receiver { running.value = false }
        lock.unlock()
        defer {
            lock.lock()
            if latest === receiver { latest = nil }
            lock.unlock()
        }
        try report()
    }
}
