import Foundation

/// A control message the phone sends the desk. One `type` per case, named by the payload itself so
/// the tag lives in exactly one place.
///
/// Field names are the desk's, which spells every one of them in snake_case through
/// `#[serde(rename = "type")]` with no `rename_all` (`apps/server/src/protocol.rs`). Each payload
/// writes explicit `CodingKeys` rather than leaning on a key-encoding strategy, because a strategy
/// is a guess that holds right up until one field disagrees.
public enum ClientMessage: Equatable, Sendable {
    case register(Registration)
    case mix(MixCommand)
    case stats(LinkStats)

    public var type: String {
        switch self {
        case .register: Registration.messageType
        case .mix: MixCommand.messageType
        case .stats: LinkStats.messageType
        }
    }

    public func encodedJSON() throws -> Data {
        let encoder = JSONEncoder()
        switch self {
        case .register(let registration): return try encoder.encode(registration)
        case .mix(let mix): return try encoder.encode(mix)
        case .stats(let stats): return try encoder.encode(stats)
        }
    }
}

/// A payload that knows the `type` it goes out under.
protocol ControlPayload: Encodable {
    static var messageType: String { get }
}

// MARK: - Capabilities

/// What this client tells the desk it understands. The desk keeps any name it is sent and gates
/// nothing on it (`apps/server/src/protocol.rs:127-146`), but we only ever announce what we have
/// actually implemented, so a closed set is the honest shape.
public enum ClientCapability: String, Equatable, Sendable, CaseIterable {
    /// `stats` carry `jitter_ms`, `cushion_ms` and `backlog_ms`.
    case statsV2 = "stats.v2"
    /// The desk may set the playback buffer with a `playback` message.
    case playbackBuffer = "playback.buffer"
    /// `stats` carry `recovered`: packets played after a delayed copy filled their gap.
    case redundancyRecovered = "redundancy.recovered"
    /// Packets are held back for the blocks a `redundancy` message names.
    case redundancyHold = "redundancy.hold"
}

// MARK: - register

/// The `register` message: who is listening, and on which UDP port.
///
/// There is deliberately no address field. The desk reads the address off the socket, so a client
/// cannot claim to be somebody else's (`apps/server/src/protocol.rs:107-111`).
///
/// The initializer normalises exactly as the desk does, so what the phone shows is what the desk
/// stores: the name is trimmed and capped, and a client id the desk would discard is dropped here
/// instead of being sent and silently ignored. A zero UDP port is refused outright, because the
/// desk refuses the whole registration for it.
public struct Registration: Equatable, Sendable, ControlPayload {
    static let messageType = "register"

    /// `MAX_MUSICIAN_NAME_LENGTH` (`apps/server/src/protocol.rs:97`).
    public static let maximumNameLength = 64
    /// `MAX_CLIENT_ID_LENGTH` (`apps/server/src/protocol.rs:100`).
    public static let maximumClientIDLength = 64

    public let name: String
    public let udpPort: UInt16
    public let clientID: String?
    public let capabilities: [ClientCapability]

    public init(
        name: String, udpPort: UInt16, clientID: String? = nil,
        capabilities: [ClientCapability] = []
    ) throws {
        guard udpPort != 0 else { throw ClientMessageError.udpPortMustNotBeZero }

        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { throw ClientMessageError.nameIsBlank }

        self.name = Self.capped(trimmed)
        self.udpPort = udpPort
        self.clientID = clientID.flatMap(Self.keptClientID)
        self.capabilities = capabilities.deduplicated()
    }

    /// Cuts `name` to ``maximumNameLength`` **Unicode scalars**, which is what the desk counts with
    /// Rust's `chars().take(…)`. Counting Swift `Character`s would keep more scalars than the desk
    /// stores, and the phone would then display a name the desk does not have.
    private static func capped(_ name: String) -> String {
        let scalars = name.unicodeScalars
        guard scalars.count > maximumNameLength else { return name }
        return String(String.UnicodeScalarView(scalars.prefix(maximumNameLength)))
    }

    /// `raw` when the desk would keep it: 1 to ``maximumClientIDLength`` ASCII letters, digits and
    /// hyphens (`valid_client_id`, `apps/server/src/protocol.rs:181-189`). Otherwise `nil`, which
    /// costs only the row-following a stable id buys.
    private static func keptClientID(_ raw: String) -> String? {
        let isKept =
            (1...maximumClientIDLength).contains(raw.count)
            && raw.utf8.allSatisfy { byte in
                byte == UInt8(ascii: "-")
                    || (UInt8(ascii: "0")...UInt8(ascii: "9")).contains(byte)
                    || (UInt8(ascii: "A")...UInt8(ascii: "Z")).contains(byte)
                    || (UInt8(ascii: "a")...UInt8(ascii: "z")).contains(byte)
            }
        return isKept ? raw : nil
    }

    private enum CodingKeys: String, CodingKey {
        case type
        case name
        case udpPort = "udp_port"
        case clientID = "client_id"
        case capabilities
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(Self.messageType, forKey: .type)
        try container.encode(name, forKey: .name)
        try container.encode(udpPort, forKey: .udpPort)
        try container.encodeIfPresent(clientID, forKey: .clientID)
        if !capabilities.isEmpty {
            try container.encode(capabilities.map(\.rawValue), forKey: .capabilities)
        }
    }
}

// MARK: - mix

/// The `mix` message: the master values, always, and the per-channel and group lists.
///
/// A list left `nil` is left out of the JSON rather than sent as null: the desk reads an absent
/// list as "unchanged" (`#[serde(default)] Option<Vec<…>>`, `apps/server/src/protocol.rs:21-37`).
/// This client nonetheless sends every list on every `mix`, the whole mix as the phone plays it, as
/// the Android client does (`ControlChannel.kt:470-504`).
///
/// The Swift names say what the values are; `CodingKeys` maps them onto the keys the desk reads,
/// where the channel gains are called `channels`.
public struct MixCommand: Equatable, Sendable, ControlPayload {
    static let messageType = "mix"

    /// The number this mix goes out under, so its ack can be told from the acks of older mixes
    /// still in flight. `nil` sends none, which the desk reads leniently.
    public var sequence: UInt64?
    public var volumePercent: Int
    public var maxLevelPercent: Int
    public var muted: Bool
    /// Per-channel level, 0 to 100. Goes out as `channels`.
    public var channelGains: [Int]?
    /// Per-channel pan, 0 left, 50 centre, 100 right.
    public var pans: [Int]?
    public var channelMutes: [Bool]?
    public var channelSolos: [Bool]?
    public var groupLevels: [Int]?

    public init(
        sequence: UInt64? = nil, volumePercent: Int, maxLevelPercent: Int, muted: Bool,
        channelGains: [Int]? = nil, pans: [Int]? = nil, channelMutes: [Bool]? = nil,
        channelSolos: [Bool]? = nil, groupLevels: [Int]? = nil
    ) {
        self.sequence = sequence
        self.volumePercent = volumePercent
        self.maxLevelPercent = maxLevelPercent
        self.muted = muted
        self.channelGains = channelGains
        self.pans = pans
        self.channelMutes = channelMutes
        self.channelSolos = channelSolos
        self.groupLevels = groupLevels
    }

    private enum CodingKeys: String, CodingKey {
        case type
        case seq
        case volumePercent = "volume_percent"
        case maxLevelPercent = "max_level_percent"
        case muted
        case channels
        case pans
        case mutes
        case solos
        case groupLevels = "group_levels"
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(Self.messageType, forKey: .type)
        try container.encodeIfPresent(sequence, forKey: .seq)
        try container.encode(volumePercent, forKey: .volumePercent)
        try container.encode(maxLevelPercent, forKey: .maxLevelPercent)
        try container.encode(muted, forKey: .muted)
        try container.encodeIfPresent(channelGains, forKey: .channels)
        try container.encodeIfPresent(pans, forKey: .pans)
        try container.encodeIfPresent(channelMutes, forKey: .mutes)
        try container.encodeIfPresent(channelSolos, forKey: .solos)
        try container.encodeIfPresent(groupLevels, forKey: .groupLevels)
    }
}

// MARK: - stats

/// The `stats` message: what the link did over the last interval.
///
/// The counts are deltas for that interval, never running totals, so a lost report never skews the
/// next one (`apps/server/src/protocol.rs:237-241`).
///
/// The optional measurements are sanitised on the way in rather than on the way out. The desk
/// already drops anything outside its range, but `JSONEncoder` *throws* on a non-finite double,
/// which would lose a whole report mid-send over a measurement that was going to be discarded
/// anyway. `recovered` is held to `packetsReceived` because the desk holds it there too, so the
/// phone's own diagnostics agree with the engineer's screen.
public struct LinkStats: Equatable, Sendable, ControlPayload {
    static let messageType = "stats"

    /// `MAX_STATS_INTERVAL_MS` (`apps/server/src/protocol.rs:225`).
    public static let maximumIntervalMilliseconds: UInt32 = 60_000
    /// `MAX_STATS_MILLISECONDS` (`apps/server/src/protocol.rs:277`).
    public static let maximumReportedMilliseconds = 60_000.0

    public let intervalMilliseconds: UInt32
    public let packetsReceived: UInt64
    public let packetsLost: UInt64
    public let underruns: UInt64
    public let receiveTimeouts: UInt64
    public let jitterMilliseconds: Double?
    public let cushionMilliseconds: Double?
    public let backlogMilliseconds: Double?
    public let recovered: UInt64?

    public init(
        intervalMilliseconds: UInt32, packetsReceived: UInt64, packetsLost: UInt64,
        underruns: UInt64, receiveTimeouts: UInt64, jitterMilliseconds: Double? = nil,
        cushionMilliseconds: Double? = nil, backlogMilliseconds: Double? = nil,
        recovered: UInt64? = nil
    ) throws {
        guard (1...Self.maximumIntervalMilliseconds).contains(intervalMilliseconds) else {
            throw ClientMessageError.statsIntervalOutOfRange(intervalMilliseconds)
        }

        self.intervalMilliseconds = intervalMilliseconds
        self.packetsReceived = packetsReceived
        self.packetsLost = packetsLost
        self.underruns = underruns
        self.receiveTimeouts = receiveTimeouts
        self.jitterMilliseconds = Self.reportable(jitterMilliseconds)
        self.cushionMilliseconds = Self.reportable(cushionMilliseconds)
        self.backlogMilliseconds = Self.reportable(backlogMilliseconds)
        self.recovered = recovered.map { min($0, packetsReceived) }
    }

    /// A measurement worth sending: finite, and inside the range the desk keeps. Anything else
    /// becomes absent, which is what the desk would have made of it. What is kept is rounded to a
    /// hundredth of a millisecond, after the range check, as Android does (`ControlChannel.kt:586`).
    private static func reportable(_ milliseconds: Double?) -> Double? {
        milliseconds.flatMap { value in
            value.isFinite && (0...maximumReportedMilliseconds).contains(value)
                ? (value * 100).rounded() / 100 : nil
        }
    }

    private enum CodingKeys: String, CodingKey {
        case type
        case intervalMilliseconds = "interval_ms"
        case packetsReceived = "packets_received"
        case packetsLost = "packets_lost"
        case underruns
        case receiveTimeouts = "receive_timeouts"
        case jitterMilliseconds = "jitter_ms"
        case cushionMilliseconds = "cushion_ms"
        case backlogMilliseconds = "backlog_ms"
        case recovered
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.container(keyedBy: CodingKeys.self)
        try container.encode(Self.messageType, forKey: .type)
        try container.encode(intervalMilliseconds, forKey: .intervalMilliseconds)
        try container.encode(packetsReceived, forKey: .packetsReceived)
        try container.encode(packetsLost, forKey: .packetsLost)
        try container.encode(underruns, forKey: .underruns)
        try container.encode(receiveTimeouts, forKey: .receiveTimeouts)
        try container.encodeIfPresent(jitterMilliseconds, forKey: .jitterMilliseconds)
        try container.encodeIfPresent(cushionMilliseconds, forKey: .cushionMilliseconds)
        try container.encodeIfPresent(backlogMilliseconds, forKey: .backlogMilliseconds)
        try container.encodeIfPresent(recovered, forKey: .recovered)
    }
}

// MARK: - Errors

/// Why a message could not be built. Each case is something the desk would refuse, caught here so
/// it never costs a round trip.
public enum ClientMessageError: Error, Equatable, Sendable {
    /// The desk refuses a registration without a port to send audio to.
    case udpPortMustNotBeZero
    /// A name of only whitespace, which the desk stores as no name at all.
    case nameIsBlank
    /// Outside 1 to ``LinkStats/maximumIntervalMilliseconds``; the desk cannot normalise rates by it.
    case statsIntervalOutOfRange(UInt32)
}

extension Array where Element: Equatable {
    /// The first occurrence of each element, in the order they arrived.
    fileprivate func deduplicated() -> [Element] {
        reduce(into: []) { kept, element in
            if !kept.contains(element) { kept.append(element) }
        }
    }
}
