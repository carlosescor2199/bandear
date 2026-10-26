import Foundation

/// Work the session asked to run later, which it can still call off.
@MainActor
public protocol ScheduledTask: AnyObject {
    /// Stops the work from running, if it has not run yet.
    func cancel()
}

/// When the session's retries run.
///
/// The session waits for ``ReconnectPolicy``'s delay through this seam rather than a clock of its
/// own, so its tests fire a retry by hand and never sleep. Android draws the same seam
/// (`ControlScheduler`, `ControlTransport.kt:57-71`) for the same reason.
@MainActor
public protocol ControlScheduler: AnyObject {
    /// Runs `work` on the main actor once `delay` has passed, unless the returned task is cancelled
    /// first.
    func schedule(after delay: Duration, _ work: @escaping @MainActor () -> Void) -> any ScheduledTask
}

/// The scheduler the app uses: the main queue, where the session lives.
public final class MainQueueScheduler: ControlScheduler {
    public init() {}

    public func schedule(after delay: Duration, _ work: @escaping @MainActor () -> Void)
        -> any ScheduledTask
    {
        let item = DispatchWorkItem { MainActor.assumeIsolated { work() } }
        DispatchQueue.main.asyncAfter(deadline: .now() + delay.seconds, execute: item)
        return MainQueueTask(item)
    }
}

/// A block waiting on the main queue.
private final class MainQueueTask: ScheduledTask {
    private let item: DispatchWorkItem

    init(_ item: DispatchWorkItem) {
        self.item = item
    }

    func cancel() {
        item.cancel()
    }
}

extension Duration {
    /// The duration in seconds, as Dispatch reads a delay.
    var seconds: Double {
        let (seconds, attoseconds) = components
        return Double(seconds) + Double(attoseconds) / 1e18
    }
}
