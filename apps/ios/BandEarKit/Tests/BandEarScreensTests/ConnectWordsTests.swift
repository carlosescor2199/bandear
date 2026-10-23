import BandEarDiscovery
import BandEarSession
import Testing

@testable import BandEarScreens

/// What only the iOS port has to answer on the Connect screen: which words each of this client's
/// refusals and discovery states gets, and the screen's copy, which Android does not test.
@Suite("Connect words")
struct ConnectWordsTests {
    private struct UnexpectedError: Error, CustomStringConvertible {
        var description: String { "the socket went away" }
    }

    // The words of `MonitorViewModel.kt:243-247`.
    @Test func theFormsRefusalsUseAndroidsWords() {
        #expect(
            ConnectWords.problem(for: ConnectionFormError.missingHost)
                == "Enter the server IP address or host name")
        #expect(
            ConnectWords.problem(for: ConnectionFormError.invalidAudioPort)
                == "Enter a valid UDP port between 1 and 65535")
        #expect(
            ConnectWords.problem(for: ConnectionFormError.invalidControlPort)
                == "Enter a valid WebSocket control port between 1 and 65535")
    }

    // The words of `MonitorViewModel.kt:242`.
    @Test func aReceiverStillStoppingUsesAndroidsWords() {
        #expect(
            ConnectWords.problem(for: MonitorAudioError.previousReceiverStopping)
                == "The previous receiver is still stopping")
    }

    @Test func anyOtherErrorDescribesItself() {
        #expect(ConnectWords.problem(for: UnexpectedError()) == "the socket went away")
    }

    @Test func deniedLocalNetworkSaysHowToAllowIt() {
        #expect(
            ConnectWords.discoveryProblem(.localNetworkDenied)
                == "BandEar needs Local Network access to find the desk. Allow it in Settings, Privacy & Security, Local Network."
        )
    }

    @Test func aBrowserThatCannotSearchSaysWhy() {
        #expect(
            ConnectWords.discoveryProblem(.unavailable(reason: "-65563: ServiceNotRunning"))
                == "Could not search for servers: -65563: ServiceNotRunning")
    }

    @Test func searchingIsSaidWhileDiscoveryCanFindADesk() {
        #expect(ConnectWords.discoveryProblem(.idle) == nil)
        #expect(ConnectWords.discoveryProblem(.browsing) == nil)
    }

    @Test func aServerCardNamesItsDesk() {
        let desk = DiscoveredDesk(
            name: "BandEar on STUDIO",
            endpoint: .bonjour(name: "BandEar on STUDIO", type: "_bandear._tcp", domain: "local."),
            sampleRate: 48_000)
        #expect(ConnectWords.serverCardDetail(desk) == "BandEar on STUDIO")
    }

    // `ConnectScreen.kt:150-156`.
    @Test func aRefusedConnectShowsItsWordsUnderCouldNotConnect() {
        #expect(
            ConnectionBanner.couldNotConnect("Enter the server IP address or host name")
                == ConnectionBanner(
                    tone: .red, title: "Could not connect",
                    detail: "Enter the server IP address or host name", busy: false))
        #expect(ConnectionBanner.couldNotConnect(nil) == nil)
    }

    // Every string of `ConnectScreen.kt`, verbatim.
    @Test func theConnectScreenUsesAndroidsWords() {
        #expect(ConnectWords.brand == "BandEar")
        #expect(ConnectWords.brandTag == "MONITOR")
        #expect(ConnectWords.nameCaption == "YOUR NAME")
        #expect(ConnectWords.nameLabel == "Your name")
        #expect(ConnectWords.nameHint == "The engineer sees this name on the desktop.")
        #expect(ConnectWords.serversTitle == "SERVERS ON THIS NETWORK")
        #expect(ConnectWords.searching == "Searching")
        #expect(
            ConnectWords.serversHint
                == "Servers appear here on their own. Tap one to join; the phone stays connected and reconnects on its own if the Wi-Fi drops."
        )
        #expect(ConnectWords.serverCardTitle == "BandEar server")
        #expect(ConnectWords.connect == "Connect")
        #expect(ConnectWords.noServersTitle == "No servers found yet")
        #expect(
            ConnectWords.noServersDetail
                == "Check that this phone is on the same Wi-Fi as the BandEar desktop.")
        #expect(ConnectWords.manualAddress == "Enter the server address manually")
        #expect(ConnectionBanner.couldNotConnectTitle == "Could not connect")
    }
}
