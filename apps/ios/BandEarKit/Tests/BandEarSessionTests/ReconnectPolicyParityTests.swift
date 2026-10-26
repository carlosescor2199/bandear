import Testing

@testable import BandEarSession

/// The Android client's `ReconnectPolicyTest` (`apps/android/app/src/test/java/com/bandear/android/
/// ReconnectPolicyTest.kt`), with its milliseconds as `Duration`s.
///
/// The same file's `ConnectionGenerationsTest` is not ported as such: Android needs that class to
/// tell sockets apart across threads, while ``ControlSession`` numbers its connections on the main
/// actor and keeps the current one's number. What those tests pin is checked through the session:
/// an older connection's late events (`ControlSessionReconnectParityTests`), a connection ending
/// once (`ControlSessionReconnectTests`), a stop making every connection stale
/// (`stopClosesASocketThatIsStillOpening`).
@Suite("Reconnect policy: Android parity")
struct ReconnectPolicyParityTests {

    @Test func firstAttemptsBackOffFromHalfASecond() {
        var policy = ReconnectPolicy()

        #expect(
            (0..<4).map { _ in policy.nextDelay() }
                == [.milliseconds(500), .seconds(1), .seconds(2), .seconds(4)])
    }

    @Test func laterAttemptsWaitTheCapForever() {
        var policy = ReconnectPolicy()
        for _ in 0..<4 { _ = policy.nextDelay() }

        #expect(ReconnectPolicy.maximumDelay == .seconds(5))
        let later = (0..<1_000).map { _ in policy.nextDelay() }
        #expect(later.allSatisfy { $0 == ReconnectPolicy.maximumDelay })
    }

    @Test func aSuccessfulOpenStartsTheScheduleAgain() {
        var policy = ReconnectPolicy()
        for _ in 0..<6 { _ = policy.nextDelay() }

        policy.reset()

        #expect(policy.nextDelay() == .milliseconds(500))
        #expect(policy.nextDelay() == .seconds(1))
    }

    @Test func delayForAttemptFollowsTheScheduleAndIsSafeAtTheEdges() {
        #expect(ReconnectPolicy.delay(forAttempt: 0) == .milliseconds(500))
        #expect(ReconnectPolicy.delay(forAttempt: 2) == .seconds(2))
        #expect(ReconnectPolicy.delay(forAttempt: 3) == .seconds(4))
        #expect(ReconnectPolicy.delay(forAttempt: 4) == .seconds(5))
        #expect(ReconnectPolicy.delay(forAttempt: Int.max) == .seconds(5))
        #expect(ReconnectPolicy.delay(forAttempt: -1) == .milliseconds(500))
    }

    @Test func reconnectsOnlyWhileTheSessionRunsAndWasNotStopped() {
        #expect(ReconnectPolicy.shouldReconnect(sessionRunning: true, stopped: false))
        #expect(!ReconnectPolicy.shouldReconnect(sessionRunning: true, stopped: true))
        #expect(!ReconnectPolicy.shouldReconnect(sessionRunning: false, stopped: false))
        #expect(!ReconnectPolicy.shouldReconnect(sessionRunning: false, stopped: true))
    }
}
