import Testing

@testable import BandEarDiscovery

private let foundDesk = DiscoveredDesk(
    name: "BandEar on STUDIO-PC",
    endpoint: .bonjour(name: "BandEar on STUDIO-PC", type: "_bandear._tcp", domain: "local."),
    sampleRate: 48_000)

@Suite("Connection form")
struct ConnectionFormTests {

    @Test("starts on the ports the desk and the Android client default to")
    func defaults() {
        let form = ConnectionForm()

        #expect(ConnectionForm.defaultControlPort == 50_001)
        #expect(ConnectionForm.defaultAudioPort == 50_000)
        #expect(form == ConnectionForm(host: "", controlPort: "50001", audioPort: "50000"))
    }

    @Test("checks the host first, then this phone's port, then the desk's")
    func checkOrder() {
        #expect(ConnectionForm(host: "", controlPort: "x", audioPort: "x").manualRequest() == .failure(.missingHost))
        #expect(
            ConnectionForm(host: "10.0.0.5", controlPort: "x", audioPort: "x").manualRequest()
                == .failure(.invalidAudioPort))
    }

    @Test(
        "refuses a port outside 1 through 65535 or not a number",
        arguments: ["0", "65536", "-1", "", "   ", "abc", "5 0001", "50001.0"])
    func refusesBadPort(port: String) {
        #expect(
            ConnectionForm(host: "10.0.0.5", controlPort: port, audioPort: "50000").manualRequest()
                == .failure(.invalidControlPort))
        #expect(
            ConnectionForm(host: "10.0.0.5", controlPort: "50001", audioPort: port).manualRequest()
                == .failure(.invalidAudioPort))
    }

    @Test("accepts the ends of the port range, typed with blanks around them")
    func acceptsPortBounds() {
        #expect(
            ConnectionForm(host: "h", controlPort: " 1 ", audioPort: "65535\n").manualRequest()
                == .success(ConnectionRequest(endpoint: .manual(host: "h", controlPort: 1), audioPort: 65_535)))
    }

    /// For a desk found over Bonjour the desk's address and control port come from the network,
    /// so the form's host and control port play no part; only this phone's port is typed.
    @Test("asks only for this phone's port when connecting to a found desk")
    func foundDeskIgnoresHostAndControlPort() {
        let form = ConnectionForm(host: "", controlPort: "not a port", audioPort: "50000")

        #expect(
            form.request(for: foundDesk)
                == .success(ConnectionRequest(endpoint: foundDesk.endpoint, audioPort: 50_000)))
    }

    @Test("refuses a found desk while this phone's port is not a port")
    func foundDeskNeedsAudioPort() {
        #expect(
            ConnectionForm(audioPort: "0").request(for: foundDesk) == .failure(.invalidAudioPort))
    }
}
