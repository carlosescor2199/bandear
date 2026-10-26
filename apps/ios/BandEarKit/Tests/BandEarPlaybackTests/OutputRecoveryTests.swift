import Testing

@testable import BandEarPlayback

/// A recovery that has been through `events`, their restarts neither tried nor reported.
private func recovery(after events: [OutputRecovery.Event]) -> OutputRecovery {
    var recovery = OutputRecovery()
    for event in events { _ = recovery.handle(event) }
    return recovery
}

/// When the app's audio output starts again after the system stopped it, and whether it plays: one
/// test per rule of T7g (`odd/tasks/ios-client.md`), each through the events the app forwards and
/// the restart results it reports.
@Suite("Output recovery")
struct OutputRecoveryTests {

    @Test("plays from the start, because the engine was just started")
    func playsFromTheStart() {
        let recovery = OutputRecovery()

        #expect(recovery.isPlaying)
        #expect(!recovery.isInterrupted)
    }

    @Test("stops for an interruption and restarts nothing")
    func anInterruptionStops() {
        var recovery = OutputRecovery()

        let restarts = recovery.handle(.interruptionBegan)

        #expect(!restarts)
        #expect(!recovery.isPlaying)
        #expect(recovery.isInterrupted)
    }

    @Test("restarts when an interruption ends asking to resume")
    func anInterruptionEndingAskingToResumeRestarts() {
        var recovery = recovery(after: [.interruptionBegan])

        let restarts = recovery.handle(.interruptionEnded(shouldResume: true))

        #expect(restarts)
        #expect(!recovery.isInterrupted)
    }

    /// Another app took the audio, so the output waits for the musician to come back to this one.
    @Test("stays stopped when an interruption ends without asking to resume")
    func anInterruptionEndingWithoutAskingToResumeStaysStopped() {
        var recovery = recovery(after: [.interruptionBegan])

        let restarts = recovery.handle(.interruptionEnded(shouldResume: false))

        #expect(!restarts)
        #expect(!recovery.isInterrupted)
        #expect(!recovery.isPlaying)
    }

    /// The system does not promise a beginning for every end. Another app holds the audio either
    /// way, so the output is taken as stopped, and the app becoming active starts it again.
    @Test("says stopped for an interruption ending without asking to resume that it never saw begin")
    func anUnseenInterruptionEndingWithoutAskingToResumeStops() {
        var recovery = OutputRecovery()

        let restartsAtTheEnd = recovery.handle(.interruptionEnded(shouldResume: false))

        #expect(!restartsAtTheEnd)
        #expect(!recovery.isPlaying)
        let restartsWhenActive = recovery.handle(.becameActive)
        #expect(restartsWhenActive)
    }

    /// The engine stops itself when the hardware's format changes, for example when headphones are
    /// plugged in.
    @Test("stops and restarts for a configuration change")
    func aConfigurationChangeRestarts() {
        var recovery = OutputRecovery()

        let restarts = recovery.handle(.configurationChanged)

        #expect(restarts)
        #expect(!recovery.isPlaying)
    }

    @Test("restarts a stopped output when the app becomes active")
    func becomingActiveRestartsAStoppedOutput() {
        var recovery = recovery(
            after: [.interruptionBegan, .interruptionEnded(shouldResume: false)])

        let restarts = recovery.handle(.becameActive)

        #expect(restarts)
    }

    @Test("leaves a playing output alone when the app becomes active")
    func becomingActiveLeavesAPlayingOutputAlone() {
        var recovery = OutputRecovery()

        let restarts = recovery.handle(.becameActive)

        #expect(!restarts)
        #expect(recovery.isPlaying)
    }

    /// Apple promises no end for every interruption: another app that keeps its session active
    /// never sends one. The musician coming back to the app is the end then, as the user decided
    /// on 2026-10-23 ("Volver a la app la reanuda"); a system that still holds the audio, such as a
    /// call in progress, refuses the restart, which ``restartFailed()`` then reports.
    @Test("ends an interruption whose end never came when the app becomes active")
    func becomingActiveEndsAnInterruption() {
        var recovery = recovery(after: [.interruptionBegan])

        let restarts = recovery.handle(.becameActive)

        #expect(restarts)
        #expect(!recovery.isInterrupted)
        #expect(!recovery.isPlaying)
    }

    @Test(
        "restarts nothing while interrupted",
        arguments: [OutputRecovery.Event.configurationChanged, .interruptionBegan])
    func nothingRestartsWhileInterrupted(event: OutputRecovery.Event) {
        var recovery = recovery(after: [.interruptionBegan])

        let restarts = recovery.handle(event)

        #expect(!restarts)
        #expect(!recovery.isPlaying)
        #expect(recovery.isInterrupted)
    }

    @Test("plays once a restart succeeded")
    func aRestartThatSucceededPlays() {
        var recovery = recovery(after: [.configurationChanged])

        recovery.restartSucceeded()

        #expect(recovery.isPlaying)
    }

    /// Started from a playing output, as for an interruption whose beginning the app never saw, so
    /// the failure itself is what says the output stopped.
    @Test("says the output stopped when a restart failed")
    func aRestartThatFailedStops() {
        var recovery = OutputRecovery()
        let restarts = recovery.handle(.interruptionEnded(shouldResume: true))
        #expect(restarts)

        recovery.restartFailed()

        #expect(!recovery.isPlaying)
        #expect(!recovery.isInterrupted)
    }

    @Test(
        "tries a failed restart again at the next event",
        arguments: [
            OutputRecovery.Event.becameActive, .configurationChanged,
            .interruptionEnded(shouldResume: true),
        ])
    func aFailedRestartIsRetriedByTheNextEvent(next: OutputRecovery.Event) {
        var recovery = recovery(after: [.interruptionBegan, .interruptionEnded(shouldResume: true)])
        recovery.restartFailed()

        let restarts = recovery.handle(next)

        #expect(restarts)
    }
}
