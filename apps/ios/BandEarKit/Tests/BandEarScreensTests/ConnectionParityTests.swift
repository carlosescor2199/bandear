import BandEarDiscovery
import BandEarSession
import Testing

@testable import BandEarScreens

/// The Android client's own tests for the status pill, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/components/ConnectionPillTest.kt`.
///
/// Android's `ControlState.UNAVAILABLE` (a channel that gave up) and `CLOSED` (a stopped one) have
/// no counterpart in ``ControlSession/ConnectionState``: the session never gives up, and a stopped
/// one is idle. Their assertions are left out; the idle one is kept.
@Suite("Connection pill: Android parity")
struct ConnectionPillParityTests {

    // ConnectionPillTest.kt:9
    @Test func liveStatesMapToTheirOwnPill() {
        #expect(ConnectionPill(.connecting) == .connecting)
        #expect(ConnectionPill(.connected) == .connected)
        #expect(ConnectionPill(.reconnecting) == .reconnecting)
    }

    // ConnectionPillTest.kt:16, without UNAVAILABLE and CLOSED.
    @Test func idleStoppedAndGivenUpChannelsReadAsNotConnected() {
        #expect(ConnectionPill(.idle) == .notConnected)
    }

    // ConnectionPillTest.kt:23
    @Test func everyPillSaysItsStateInWords() {
        #expect(ConnectionPill.connected.words == "Connected")
        #expect(ConnectionPill.reconnecting.words == "Reconnecting")
        #expect(ConnectionPill.connecting.words == "Connecting")
        #expect(ConnectionPill.notConnected.words == "Not connected")
    }

    // ConnectionPillTest.kt:31
    @Test func everyPillHasItsOwnColour() {
        #expect(ConnectionPill.connected.tone == .green)
        #expect(ConnectionPill.reconnecting.tone == .amber)
        #expect(ConnectionPill.connecting.tone == .blue)
        #expect(ConnectionPill.notConnected.tone == .neutral)
    }

    // ConnectionPillTest.kt:39
    @Test func noTwoPillsShareWordsOrColour() {
        let kinds = ConnectionPill.allCases
        #expect(Set(kinds.map(\.words)).count == kinds.count)
        #expect(Set(kinds.map(\.tone)).count == kinds.count)
    }
}

/// The Android client's own tests for the control status detail, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ControlStatusTest.kt`.
///
/// Not ported, for want of a state to say them for (see ``ConnectionWords/statusDetail(_:mixHeldByDesk:)``):
/// `lostChannelAfterAcknowledgementKeepsServerOwnership` (`ControlStatusTest.kt:23`) and
/// `lostChannelWithoutAcknowledgementFallsBackToLocalProcessing` (`ControlStatusTest.kt:31`), which
/// call it with `UNAVAILABLE` and `CLOSED`.
@Suite("Control status: Android parity")
struct ControlStatusParityTests {
    private func label(_ state: ControlSession.ConnectionState, acknowledged: Bool) -> String {
        ConnectionWords.statusDetail(state, mixHeldByDesk: acknowledged)
    }

    // ControlStatusTest.kt:7
    @Test func connectedAfterAcknowledgementReportsServerOwnership() {
        #expect(label(.connected, acknowledged: true) == "connected - mix applied on the server")
    }

    // ControlStatusTest.kt:15
    @Test func connectedWithoutAcknowledgementWaitsForTheServer() {
        #expect(
            label(.connected, acknowledged: false)
                == "connected - waiting for the server to take over")
    }

    // ControlStatusTest.kt:39
    @Test func reconnectingAfterAcknowledgementKeepsServerOwnership() {
        #expect(label(.reconnecting, acknowledged: true) == "reconnecting - mix held at the server")
    }

    // ControlStatusTest.kt:47
    @Test func reconnectingWithoutAcknowledgementMixesOnThisDevice() {
        #expect(
            label(.reconnecting, acknowledged: false) == "reconnecting - mixing on this device")
    }

    // ControlStatusTest.kt:55
    @Test func idleAndConnectingNeverClaimOwnership() {
        #expect(label(.idle, acknowledged: false) == "not connected")
        #expect(label(.idle, acknowledged: true) == "not connected")
        #expect(label(.connecting, acknowledged: false) == "connecting...")
    }
}

/// The Android client's own tests for the connection banner, ported case for case from
/// `apps/android/app/src/test/java/com/bandear/android/ui/screens/MixScreenTextTest.kt`; only the
/// `connectionBanner` cases, since the rest of that file is about the Mix screen's other words.
///
/// Android builds the banner from a pill. Here it is derived from the connection's state, and the
/// state behind each of Android's pills is the one ``ConnectionPill/init(_:)`` maps to it; for
/// `NOT_CONNECTED` while a session runs that is ``ControlSession/ConnectionState/idle``.
@Suite("Connection banner: Android parity")
struct ConnectionBannerParityTests {
    private let server = DeskEndpoint.manual(host: "192.168.1.34", controlPort: 50_001)

    private func running(_ connection: ControlSession.ConnectionState, held: Bool)
        -> ConnectionStatus
    {
        ConnectionStatus(
            connection: connection, sessionRunning: true, desk: server, mixHeldByDesk: held)
    }

    // MixScreenTextTest.kt:141
    @Test func noBannerWhileConnectedOrConnecting() {
        #expect(running(.connected, held: true).banner == nil)
        #expect(running(.connecting, held: false).banner == nil)
    }

    // MixScreenTextTest.kt:147
    @Test func noBannerWithoutASession() {
        #expect(ConnectionStatus().banner == nil)
    }

    // MixScreenTextTest.kt:152
    @Test func reconnectingSaysTheServerHoldsTheMixWhenItDoes() {
        #expect(
            running(.reconnecting, held: true).banner
                == ConnectionBanner(
                    tone: .amber,
                    title: "Reconnecting to 192.168.1.34:50001",
                    detail: "Your mix is held at the server. It retries on its own.",
                    busy: true))
    }

    // MixScreenTextTest.kt:166
    @Test func reconnectingSaysThePhoneMixesWhenTheServerNeverTookOver() {
        #expect(
            running(.reconnecting, held: false).banner?.detail
                == "This phone keeps mixing until it is back. It retries on its own.")
    }

    // MixScreenTextTest.kt:167
    @Test func aControlChannelThatGaveUpAsksForANewConnect() {
        #expect(
            running(.idle, held: true).banner
                == ConnectionBanner(
                    tone: .red,
                    title: "Lost the server",
                    detail:
                        "Your mix stays at the server's last setting. Disconnect in Settings, then connect again.",
                    busy: false))
        #expect(
            running(.idle, held: false).banner?.detail
                == "This phone keeps mixing. Disconnect in Settings, then connect again.")
    }
}

/// The Android reducer's tests for the pill, the status detail and who holds the mix, ported from
/// `apps/android/app/src/test/java/com/bandear/android/ui/state/MonitorReducerTest.kt`.
///
/// Android reduces a `ControlChanged` or `MixAcknowledged` event into its connection state. Here the
/// session already holds the result (``ControlSession/connection``, ``ControlSession/mixHeldByDesk``),
/// so each case builds the state those events leave and checks what is derived from it.
@Suite("Connection status: Android reducer parity")
struct ConnectionReducerParityTests {
    private let server = DeskEndpoint.manual(host: "192.168.1.20", controlPort: 50_001)

    private func running(_ connection: ControlSession.ConnectionState, held: Bool = false)
        -> ConnectionStatus
    {
        ConnectionStatus(
            connection: connection, sessionRunning: true, desk: server, mixHeldByDesk: held)
    }

    // MonitorReducerTest.kt:122, without UNAVAILABLE and CLOSED.
    @Test func theControlStateMapsToThePill() {
        #expect(running(.connected).pill == .connected)
        #expect(running(.connecting).pill == .connecting)
        #expect(running(.idle).pill == .notConnected)
    }

    // MonitorReducerTest.kt:133
    @Test func reconnectingMapsToTheReconnectingPillAndSaysWhoHoldsTheMix() {
        let held = running(.reconnecting, held: true)
        let local = running(.reconnecting, held: false)

        #expect(held.pill == .reconnecting)
        #expect(held.reconnecting)
        #expect(held.mixHeldByDesk)
        #expect(held.statusDetail == "reconnecting - mix held at the server")
        #expect(local.pill == .reconnecting)
        #expect(!local.mixHeldByDesk)
        #expect(local.statusDetail == "reconnecting - mixing on this device")
    }

    // MonitorReducerTest.kt:151
    @Test func anAcknowledgedMixIsHeldByTheServer() {
        let state = running(.connected, held: true)

        #expect(state.pill == .connected)
        #expect(state.mixHeldByDesk)
        #expect(!state.reconnecting)
        #expect(state.statusDetail == "connected - mix applied on the server")
    }
}
