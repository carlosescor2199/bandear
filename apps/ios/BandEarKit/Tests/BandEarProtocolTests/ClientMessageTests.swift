import Foundation
import Testing

@testable import BandEarProtocol

/// Compares what we put on the wire as a parsed object, not as a string: JSON key order carries no
/// meaning and pinning it would make the tests fail for a reason the server does not care about.
private func wireObject(_ message: ClientMessage) throws -> NSDictionary {
    let data = try message.encodedJSON()
    let object = try JSONSerialization.jsonObject(with: data)
    return try #require(object as? NSDictionary)
}

@Suite("Client control messages")
struct ClientMessageTests {

    // MARK: register

    @Test("names its own type")
    func messageTypes() throws {
        let registration = try Registration(name: "Ana", udpPort: 50_000)
        let mix = MixCommand(volumePercent: 80, maxLevelPercent: 100, muted: false)
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 0, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0)

        #expect(ClientMessage.register(registration).type == "register")
        #expect(ClientMessage.mix(mix).type == "mix")
        #expect(ClientMessage.stats(stats).type == "stats")
    }

    @Test("sends the shape an older phone sends when it has no id and no capabilities")
    func registerMinimal() throws {
        let message = ClientMessage.register(try Registration(name: "Ana", udpPort: 50_000))

        #expect(
            try wireObject(message) == [
                "type": "register", "name": "Ana", "udp_port": 50_000,
            ] as NSDictionary)
    }

    @Test("sends the id and the capabilities it has, in the order given")
    func registerFull() throws {
        let registration = try Registration(
            name: "Ana", udpPort: 50_000, clientID: "3F2504E0-4F89-11D3-9A0C-0305E82C3301",
            capabilities: [.statsV2, .playbackBuffer, .redundancyHold])
        let message = ClientMessage.register(registration)

        #expect(
            try wireObject(message) == [
                "type": "register", "name": "Ana", "udp_port": 50_000,
                "client_id": "3F2504E0-4F89-11D3-9A0C-0305E82C3301",
                "capabilities": ["stats.v2", "playback.buffer", "redundancy.hold"],
            ] as NSDictionary)
    }

    @Test("trims the name the musician typed")
    func trimsName() throws {
        #expect(try Registration(name: "  Ana  ", udpPort: 50_000).name == "Ana")
    }

    /// Android's `RegistrationMessageTest.trimsTheNameButKeepsAnEmptyOne`: a name with nothing in it
    /// still registers, with an empty name, which the desk stores as no name
    /// (`registered_name`, `apps/server/src/protocol.rs:330-336`) and still sends audio to.
    @Test("keeps a name with nothing in it, sent empty", arguments: ["", "   ", "\t\n"])
    func keepsAnEmptyName(name: String) throws {
        let registration = try Registration(name: name, udpPort: 50_001)

        #expect(registration.name == "")
        #expect(
            try wireObject(.register(registration)) == [
                "type": "register", "name": "", "udp_port": 50_001,
            ] as NSDictionary)
    }

    @Test("caps the name at the sixty-four the desk keeps")
    func truncatesName() throws {
        let registration = try Registration(
            name: String(repeating: "a", count: 100), udpPort: 50_000)

        #expect(Registration.maximumNameLength == 64)
        #expect(registration.name.count == 64)
    }

    /// The desk truncates with Rust's `chars().take(64)`, which counts Unicode scalars, so this
    /// does too. Counting Swift `Character`s instead would keep more scalars than the desk stores
    /// and the phone would show a name the desk does not have.
    @Test("counts the name in scalars, the way the desk counts it")
    func truncatesNameByScalar() throws {
        let family = "👨‍👩‍👧"  // three scalars joined by two zero-width joiners: 5 scalars, 1 Character
        let name = String(repeating: family, count: 20)

        let registration = try Registration(name: name, udpPort: 50_000)

        #expect(name.unicodeScalars.count == 100)
        #expect(registration.name.unicodeScalars.count == 64)
    }

    @Test("refuses a UDP port of zero, which the desk rejects")
    func refusesZeroPort() {
        #expect(throws: ClientMessageError.udpPortMustNotBeZero) {
            try Registration(name: "Ana", udpPort: 0)
        }
    }

    @Test(
        "drops a client id the desk would not keep",
        arguments: ["has space", "has_underscore", "has.dot", "", "aa\u{00E9}"])
    func dropsInvalidClientID(clientID: String) throws {
        let registration = try Registration(name: "Ana", udpPort: 50_000, clientID: clientID)

        #expect(registration.clientID == nil)
    }

    @Test("drops a client id longer than the desk keeps")
    func dropsOverlongClientID() throws {
        let registration = try Registration(
            name: "Ana", udpPort: 50_000, clientID: String(repeating: "a", count: 65))

        #expect(Registration.maximumClientIDLength == 64)
        #expect(registration.clientID == nil)
    }

    @Test("keeps letters, digits and hyphens in a client id", arguments: ["a", "A1-b2", "3F25-04E0"])
    func keepsValidClientID(clientID: String) throws {
        #expect(try Registration(name: "Ana", udpPort: 50_000, clientID: clientID).clientID == clientID)
    }

    @Test("announces each capability once, keeping the order given")
    func dedupesCapabilities() throws {
        let registration = try Registration(
            name: "Ana", udpPort: 50_000,
            capabilities: [.redundancyHold, .statsV2, .redundancyHold, .statsV2])

        #expect(registration.capabilities == [.redundancyHold, .statsV2])
    }

    // MARK: mix

    @Test("sends only the master values when no list changed")
    func mixMasterOnly() throws {
        let mix = MixCommand(volumePercent: 80, maxLevelPercent: 95, muted: true)

        #expect(
            try wireObject(.mix(mix)) == [
                "type": "mix", "volume_percent": 80, "max_level_percent": 95, "muted": true,
            ] as NSDictionary)
    }

    @Test("numbers a mix when it was given a number")
    func mixWithSequence() throws {
        let mix = MixCommand(
            sequence: 7, volumePercent: 80, maxLevelPercent: 100, muted: false)

        #expect(try wireObject(.mix(mix))["seq"] as? UInt64 == 7)
    }

    @Test("maps the Swift names onto the keys the desk reads")
    func mixWireKeys() throws {
        let mix = MixCommand(
            volumePercent: 80, maxLevelPercent: 100, muted: false, channelGains: [10, 20],
            pans: [0, 50], channelMutes: [true, false], channelSolos: [false, true],
            groupLevels: [30])

        #expect(
            try wireObject(.mix(mix)) == [
                "type": "mix", "volume_percent": 80, "max_level_percent": 100, "muted": false,
                "channels": [10, 20], "pans": [0, 50], "mutes": [true, false],
                "solos": [false, true], "group_levels": [30],
            ] as NSDictionary)
    }

    @Test("leaves out a list it was not given rather than sending null")
    func mixOmitsAbsentLists() throws {
        let mix = MixCommand(
            volumePercent: 80, maxLevelPercent: 100, muted: false, channelGains: [10])
        let object = try wireObject(.mix(mix))

        #expect(object["channels"] as? [Int] == [10])
        #expect(object.allKeys.count == 5)
        for absent in ["pans", "mutes", "solos", "group_levels", "seq"] {
            #expect(object[absent] == nil)
        }
    }

    // MARK: stats

    @Test("sends the four counts an older phone sends")
    func statsRequiredOnly() throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 480, packetsLost: 2, underruns: 1,
            receiveTimeouts: 3)

        #expect(
            try wireObject(.stats(stats)) == [
                "type": "stats", "interval_ms": 1_000, "packets_received": 480,
                "packets_lost": 2, "underruns": 1, "receive_timeouts": 3,
            ] as NSDictionary)
    }

    @Test("sends the extras a stats.v2 phone measures")
    func statsWithExtras() throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 480, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0, jitterMilliseconds: 1.25, cushionMilliseconds: 40,
            backlogMilliseconds: 52.5, recovered: 2)
        let object = try wireObject(.stats(stats))

        #expect(object["jitter_ms"] as? Double == 1.25)
        #expect(object["cushion_ms"] as? Double == 40)
        #expect(object["backlog_ms"] as? Double == 52.5)
        #expect(object["recovered"] as? UInt64 == 2)
    }

    /// Android rounds each extra to a hundredth, after checking its range on the measured value
    /// (`putMilliseconds`, `ControlChannel.kt:586`).
    @Test(
        "rounds a measurement to a hundredth of a millisecond",
        arguments: [(1.2345, 1.23), (0.006, 0.01), (37.5, 37.5), (59_999.996, 60_000.0)])
    func roundsMeasurement(measured: Double, sent: Double) throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 0, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0, jitterMilliseconds: measured, cushionMilliseconds: measured,
            backlogMilliseconds: measured)
        let object = try wireObject(.stats(stats))

        for key in ["jitter_ms", "cushion_ms", "backlog_ms"] {
            #expect(object[key] as? Double == sent, "\(key)")
        }
    }

    @Test("refuses an interval the desk rejects", arguments: [0, 60_001, 100_000] as [UInt32])
    func refusesIntervalOutOfRange(interval: UInt32) {
        #expect(throws: ClientMessageError.statsIntervalOutOfRange(interval)) {
            try LinkStats(
                intervalMilliseconds: interval, packetsReceived: 0, packetsLost: 0, underruns: 0,
                receiveTimeouts: 0)
        }
    }

    @Test("accepts the ends of the interval the desk allows", arguments: [1, 60_000] as [UInt32])
    func acceptsIntervalBounds(interval: UInt32) throws {
        let stats = try LinkStats(
            intervalMilliseconds: interval, packetsReceived: 0, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0)

        #expect(stats.intervalMilliseconds == interval)
        #expect(LinkStats.maximumIntervalMilliseconds == 60_000)
    }

    /// `JSONEncoder` throws on a non-finite double, which would lose the whole report mid-send for
    /// a measurement the desk would have dropped anyway.
    @Test(
        "drops a measurement that is not a finite number",
        arguments: [Double.nan, .infinity, -.infinity])
    func dropsNonFiniteMeasurement(value: Double) throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 0, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0, jitterMilliseconds: value, cushionMilliseconds: value,
            backlogMilliseconds: value)

        #expect(stats.jitterMilliseconds == nil)
        #expect(try wireObject(.stats(stats))["jitter_ms"] == nil)
    }

    @Test(
        "drops a measurement outside the range the desk keeps, before rounding it",
        arguments: [-0.5, -0.004, 60_000.004, 60_000.5])
    func dropsMeasurementOutOfRange(value: Double) throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 0, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0, cushionMilliseconds: value)

        #expect(LinkStats.maximumReportedMilliseconds == 60_000)
        #expect(stats.cushionMilliseconds == nil)
    }

    @Test("holds recovered to the packets that were received, as the desk does")
    func clampsRecovered() throws {
        let stats = try LinkStats(
            intervalMilliseconds: 1_000, packetsReceived: 10, packetsLost: 0, underruns: 0,
            receiveTimeouts: 0, recovered: 25)

        #expect(stats.recovered == 10)
    }
}
