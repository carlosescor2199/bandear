import Testing

@testable import BandEarPlayback

/// A launch, told apart by identity as Android tells its threads apart with `===`. Android's test
/// launches the strings "first" and "second"; a Swift string has no identity, so each is a named
/// object here.
private final class Launch: CustomStringConvertible {
    let description: String

    init(_ name: String) { description = name }
}

private struct ListenerFailed: Error {}

/// The Android client's own tests for which receive loop owns the session flag, ported case for
/// case from `apps/android/app/src/test/java/com/bandear/android/ReceiverLaunchesTest.kt`.
@Suite("Receiver launches: Android parity")
struct ReceiverLaunchesParityTests {
    private let running = RunningFlag(false)
    private let launches: ReceiverLaunches<Launch>
    private let first = Launch("first")
    private let second = Launch("second")

    init() {
        launches = ReceiverLaunches(running: running)
    }

    /// What a caller does to start a session: raise the flag, then launch.
    private func startSession(_ receiver: Launch) {
        running.value = true
        launches.launched(receiver)
    }

    // ReceiverLaunchesTest.kt:23
    @Test func aReceiverThatEndsOnItsOwnEndsTheSession() {
        var reports = 0
        startSession(first)

        launches.finished(first) { reports += 1 }

        #expect(!running.value)
        #expect(reports == 1)
        #expect(launches.current == nil)
    }

    // ReceiverLaunchesTest.kt:34
    @Test func aFinishingReceiverCannotEndANewerSession() {
        var reports = 0
        startSession(first)
        running.value = false  // Stop.
        startSession(second)  // A new session before the first loop has finished.

        launches.finished(first) { reports += 1 }

        #expect(running.value, "the newer session keeps running")
        #expect(reports == 1, "the older loop still reports its own stop")
        #expect(launches.current === second)
    }

    // ReceiverLaunchesTest.kt:47
    @Test func theReceiverIsReleasedOnlyAfterTheFlagIsDownAndItsStopIsReported() {
        var reports = 0
        startSession(first)

        launches.finished(first) {
            // While the stop is being reported the flag is already down and the receiver is still
            // current, so no caller can launch into a session the old loop is about to end.
            #expect(!running.value)
            #expect(launches.current === first)
            reports += 1
        }

        #expect(reports == 1)
        #expect(launches.current == nil)
    }

    // ReceiverLaunchesTest.kt:63
    @Test func theNextSessionStartedAfterTheReleaseKeepsRunning() {
        var reports = 0
        startSession(first)
        launches.finished(first) { reports += 1 }

        startSession(second)

        #expect(running.value)
        #expect(launches.current === second)
    }

    // ReceiverLaunchesTest.kt:74
    @Test func aReportThatFailsStillReleasesTheReceiver() {
        startSession(first)

        #expect(throws: ListenerFailed.self) {
            try launches.finished(first) { throw ListenerFailed() }
        }

        #expect(!running.value)
        #expect(launches.current == nil)
    }
}
