/// How long the session waits before each attempt to open a lost connection again.
///
/// Ported from the Android client's `ReconnectPolicy.kt`. The first retries come quickly, so a short
/// Wi-Fi blip or a desk restart costs the musician well under a few seconds; then the wait grows and
/// settles at ``maximumDelay``, so a desk that stays away is retried every five seconds for as long
/// as the session runs, without hammering the network meanwhile. A connection that proves itself, by
/// receiving its first message from the desk, calls ``reset()``, so the next loss starts from the
/// short end again; a desk that accepts and drops at once does not (`ReconnectPolicy.kt:5-14`).
///
/// A value: the session owns one and nothing else touches it.
public struct ReconnectPolicy: Equatable, Sendable {
    /// The waits of the first attempts, in order (`ReconnectPolicy.kt:35`). Every attempt after them
    /// waits ``maximumDelay``.
    public static let schedule: [Duration] = [
        .milliseconds(500), .seconds(1), .seconds(2), .seconds(4),
    ]

    /// The longest wait between two attempts (`ReconnectPolicy.kt:38`).
    public static let maximumDelay: Duration = .seconds(5)

    /// How many attempts of the schedule have been handed out since the last ``reset()``.
    private var attempt = 0

    public init() {}

    /// The wait before the next attempt; each call moves one step along the schedule.
    public mutating func nextDelay() -> Duration {
        let delay = Self.delay(forAttempt: attempt)
        // The counter stops once the schedule is exhausted, so it can never overflow.
        if attempt < Self.schedule.count { attempt += 1 }
        return delay
    }

    /// Starts the schedule again, after a connection heard from the desk.
    public mutating func reset() {
        attempt = 0
    }

    /// The wait before attempt `attempt`, counted from 0 for the first retry after a loss. A negative
    /// attempt reads as the first, and any attempt past the schedule waits ``maximumDelay``.
    public static func delay(forAttempt attempt: Int) -> Duration {
        let index = max(attempt, 0)
        return index < schedule.count ? schedule[index] : maximumDelay
    }

    /// Whether a connection that failed or closed is opened again: only while the session still runs
    /// and nobody stopped it (`ReconnectPolicy.kt:44-50`).
    public static func shouldReconnect(sessionRunning: Bool, stopped: Bool) -> Bool {
        sessionRunning && !stopped
    }
}
