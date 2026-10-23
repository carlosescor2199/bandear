import BandEarDiscovery
import Testing

@testable import BandEarScreens

/// What Android leaves untested on the Settings screen: its copy, the server card's dot and the
/// rule that hides a refusal while a session runs, all from `SettingsScreen.kt`.
@Suite("Settings words")
struct SettingsWordsTests {
    private let desk = DeskEndpoint.manual(host: "192.168.1.34", controlPort: 50_001)

    // `SettingsScreen.kt:137`.
    @Test func theServerCardsDotFollowsThePillOnlyWhileASessionRuns() {
        let running = ConnectionStatus(connection: .reconnecting, sessionRunning: true, desk: desk)
        let ended = ConnectionStatus(connection: .connected, sessionRunning: false)

        #expect(SettingsWords.serverCardTone(running) == .amber)
        #expect(SettingsWords.serverCardTone(ended) == .neutral)
    }

    // `SettingsText.kt:69`: no server is the same as no session.
    @Test func aRunningSessionWithNoDeskPointsToTheForm() {
        let status = ConnectionStatus(connection: .connected, sessionRunning: true)
        #expect(
            SettingsWords.serverCardDetail(status) == "Pick a server, or enter its address below.")
    }

    // `SettingsScreen.kt:220`.
    @Test func theManualFormShowsARefusalOnlyWithoutASession() {
        let problem = "Enter a valid UDP port between 1 and 65535"

        #expect(
            SettingsWords.manualProblemBanner(problem, sessionRunning: false)
                == ConnectionBanner(
                    tone: .red, title: "Could not connect", detail: problem, busy: false))
        #expect(SettingsWords.manualProblemBanner(problem, sessionRunning: true) == nil)
        #expect(SettingsWords.manualProblemBanner(nil, sessionRunning: false) == nil)
    }

    // Every fixed string of `SettingsScreen.kt` outside the diagnostics card, verbatim.
    @Test func theSettingsScreenUsesAndroidsWords() {
        #expect(SettingsWords.title == "Settings")
        #expect(SettingsWords.backToServers == "Servers")
        #expect(SettingsWords.nameCaption == "YOUR NAME")
        #expect(SettingsWords.nameLabel == "Your name")
        #expect(SettingsWords.disconnect == "Disconnect")
        #expect(SettingsWords.manualConnectionTitle == "MANUAL CONNECTION")
        #expect(SettingsWords.serverAddressLabel == "Server address")
        #expect(SettingsWords.serverAddressPlaceholder == "192.168.1.34")
        #expect(SettingsWords.audioPortLabel == "Audio port")
        #expect(SettingsWords.controlPortLabel == "Control port")
        #expect(SettingsWords.connectToThisAddress == "Connect to this address")
        #expect(SettingsWords.disconnectFirst == "Disconnect first to connect to another server.")
    }
}
