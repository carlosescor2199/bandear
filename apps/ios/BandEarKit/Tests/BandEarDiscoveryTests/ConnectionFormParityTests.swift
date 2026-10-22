import Testing

@testable import BandEarDiscovery

/// The Android client's checks on the connection form, with the inputs of its own tests in
/// `apps/android/app/src/test/java/com/bandear/android/ui/state/MonitorViewModelTest.kt`
/// (`aManualAddressIsCheckedBeforeAnythingStarts`, `aFoundServerUsesTheAudioPortFromSettings`).
///
/// Android turns each refusal into a message on the same call. Here the check returns the case and
/// the screen (T8) owns the words, which must stay Android's:
/// - `missingHost`: "Enter the server IP address or host name"
/// - `invalidAudioPort`: "Enter a valid UDP port between 1 and 65535"
/// - `invalidControlPort`: "Enter a valid WebSocket control port between 1 and 65535"
@Suite("Connection form: Android parity")
struct ConnectionFormParityTests {

    @Test func aManualAddressIsCheckedBeforeAnythingStarts() {
        #expect(
            ConnectionForm(host: "  ", controlPort: "50001", audioPort: "50000").manualRequest()
                == .failure(.missingHost))
        #expect(
            ConnectionForm(host: "10.0.0.5", controlPort: "50001", audioPort: "0").manualRequest()
                == .failure(.invalidAudioPort))
        #expect(
            ConnectionForm(host: "10.0.0.5", controlPort: "x", audioPort: "50000").manualRequest()
                == .failure(.invalidControlPort))

        #expect(
            ConnectionForm(host: " 10.0.0.5 ", controlPort: "50101", audioPort: "50100").manualRequest()
                == .success(
                    ConnectionRequest(
                        endpoint: .manual(host: "10.0.0.5", controlPort: 50_101), audioPort: 50_100)))
    }

    @Test func aFoundServerUsesTheAudioPortFromSettings() {
        let form = ConnectionForm(host: "", controlPort: "50001", audioPort: "50200")
        let desk = DiscoveredDesk(
            name: "BandEar on STUDIO-PC",
            endpoint: .bonjour(name: "BandEar on STUDIO-PC", type: "_bandear._tcp", domain: "local."),
            sampleRate: 48_000)

        #expect(form.request(for: desk).map(\.audioPort) == .success(50_200))
    }
}
