import Foundation

// MARK: - Channels and groups

/// What a source channel carries. The desk names eleven
/// (`apps/server/src/protocol.rs:420-424`) and states the client's duty plainly: one that meets a
/// kind it does not know shows it as ``other``. So decoding never fails on an unknown kind.
public enum ChannelKind: String, Equatable, Sendable, CaseIterable {
    case voice, guitar, bass, keys, drums, percussion, horns, strings, playback, click, other
}

/// One source channel in the desk's `config`, in channel order: entry 0 is CH 1.
public struct SourceChannel: Equatable, Sendable {
    /// What the engineer called the input, at most 32 characters. Empty when it has no name, and
    /// the client then labels it by its number.
    public let name: String
    public let kind: ChannelKind
    /// The engineer hid the input: it is in nobody's mix and the client does not show it.
    public let isHidden: Bool
    /// This channel and the next are one stereo pair. Only the pair's first channel carries it.
    public let isLinkedToNext: Bool

    public init(name: String, kind: ChannelKind, isHidden: Bool, isLinkedToNext: Bool) {
        self.name = name
        self.kind = kind
        self.isHidden = isHidden
        self.isLinkedToNext = isLinkedToNext
    }
}

/// One group in the desk's `config`: a stable id, a name, and 0-based source channel indices.
public struct ChannelGroup: Equatable, Sendable {
    /// The desk's stable id for the group, or nil from a desk that predates group ids. Groups are
    /// matched by position everywhere, as on Android.
    public let id: String?
    public let name: String
    public let channels: [Int]

    public init(id: String?, name: String, channels: [Int]) {
        self.id = id
        self.name = name
        self.channels = channels
    }
}

/// The `config` message: what the desk is capturing, sent as soon as a socket opens and again
/// whenever the device or the channels change (`apps/server/src/control.rs:672,678-690`).
public struct DeskConfiguration: Equatable, Sendable {
    /// `MAX_MIX_CHANNELS` (`apps/server/src/mix.rs:9`).
    public static let maximumMixChannels = 32
    /// `MAX_GROUPS` (`apps/server/src/mix.rs:10`).
    public static let maximumGroups = 16

    public let sourceChannels: UInt8
    /// The rate the desk named, or nil when the `config` carried none; every audio datagram
    /// carries the rate that matters.
    public let sampleRate: UInt32?
    public let groups: [ChannelGroup]
    /// Exactly one per source channel, or nil when the desk sent no details (an older desk), which
    /// a screen shows differently from details that name nothing.
    public let channels: [SourceChannel]?

    public init(
        sourceChannels: UInt8, sampleRate: UInt32?, groups: [ChannelGroup],
        channels: [SourceChannel]?
    ) {
        self.sourceChannels = sourceChannels
        self.sampleRate = sampleRate
        self.groups = groups
        self.channels = channels
    }
}

// MARK: - Mix reports

/// A whole mix as the desk reports it. `mix_ack` answers the phone's own `mix` and carries that
/// mix's number; `mix_state` is pushed when the mix changed from elsewhere and never carries one
/// (`apps/server/src/protocol.rs:382-396`).
public struct MixReport: Equatable, Sendable {
    /// What `engineer_limit_percent` holds when the engineer set no limit.
    public static let noEngineerLimitPercent = 100

    public let sequence: UInt64?
    public let volumePercent: Int
    public let maxLevelPercent: Int
    public let muted: Bool
    public let channelGains: [Int]
    public let pans: [Int]
    public let channelMutes: [Bool]
    public let channelSolos: [Bool]
    public let groupLevels: [Int]
    /// Caps what the musician hears on top of their own `maxLevelPercent`; the desk applies the
    /// lower of the two, so a screen should show both and say the lower one wins.
    public let engineerLimitPercent: Int
    /// One entry per channel, index-aligned with ``channelGains``. A locked channel keeps its
    /// level, pan, mute and solo whatever the phone sends, and its controls should read disabled.
    public let locked: [Bool]
    /// The musician whose mix this one mirrors, or `nil` when it follows nobody. While it is set,
    /// the channel and group controls should read disabled.
    public let following: String?

    public init(
        sequence: UInt64?, volumePercent: Int, maxLevelPercent: Int, muted: Bool,
        channelGains: [Int], pans: [Int], channelMutes: [Bool], channelSolos: [Bool],
        groupLevels: [Int], engineerLimitPercent: Int, locked: [Bool], following: String?
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
        self.engineerLimitPercent = engineerLimitPercent
        self.locked = locked
        self.following = following
    }
}

// MARK: - Playback and errors

/// The cushion the desk set for this musician (`apps/server/src/protocol.rs:577-600`).
public enum PlaybackBufferSetting: Equatable, Sendable {
    /// The phone adapts the cushion on its own.
    case auto
    /// The phone keeps this cushion and stops adapting it.
    case fixed(milliseconds: UInt16)

    /// `MIN_FIXED_BUFFER_MS` (`apps/server/src/playback.rs:17`).
    public static let minimumFixedMilliseconds: UInt16 = 5
    /// `MAX_FIXED_BUFFER_MS` (`apps/server/src/playback.rs:21`).
    public static let maximumFixedMilliseconds: UInt16 = 500
}

/// An `error` reply. It carries the number of the `mix` it refused, when that mix had one.
public struct DeskError: Equatable, Sendable {
    public let message: String
    public let sequence: UInt64?

    public init(message: String, sequence: UInt64?) {
        self.message = message
        self.sequence = sequence
    }
}

// MARK: - Routing

/// A control message from the desk.
///
/// Unknown types are kept, never thrown: the desk's own contract repeats, message after message,
/// that "a phone ignores any unknown type". ``unrecognized(type:)`` makes that ignoring visible, so
/// a diagnostics screen can name what arrived instead of counting it as nothing.
///
/// Fields added after a message shipped are read with the documented fallback rather than required,
/// so an older desk still drives this client.
public enum ServerMessage: Equatable, Sendable {
    case configuration(DeskConfiguration)
    case mixAck(MixReport)
    /// A `mix_ack` numbered `sequence` whose mix could not be read. It still answers that number,
    /// as Android's does (`ControlChannel.kt:172-176`), so the phone's mix it answers is not left
    /// waiting for an answer that already came; there is just nothing to show.
    case unreadableMixAck(sequence: UInt64)
    case mixState(MixReport)
    case playbackBuffer(PlaybackBufferSetting)
    case redundancyHold(blocks: UInt8)
    case error(DeskError)
    case unrecognized(type: String)

    /// `MAX_REDUNDANCY_DELAY_BLOCKS` (`apps/server/src/redundancy.rs:42`).
    public static let maximumRedundancyHoldBlocks: UInt8 = 4

    public init(decodingJSON data: Data) throws {
        let decoder = JSONDecoder()
        let type = try Self.routingType(in: data, using: decoder)

        self = try Self.translatingMissingFields(in: type) {
            switch type {
            case "config":
                .configuration(try decoder.decode(WireConfiguration.self, from: data).value)
            case "mix_ack":
                try Self.mixAck(decoding: data, using: decoder)
            case "mix_state":
                // The desk never numbers a push; a number on one answers nothing this phone sent.
                .mixState(try decoder.decode(WireMixReport.self, from: data).value(sequence: nil))
            case "playback":
                .playbackBuffer(try decoder.decode(WirePlayback.self, from: data).value)
            case "redundancy":
                .redundancyHold(blocks: try decoder.decode(WireRedundancy.self, from: data).value)
            case "error":
                .error(try decoder.decode(WireError.self, from: data).value)
            default:
                .unrecognized(type: type)
            }
        }
    }
}

// MARK: - Decoding plumbing

private struct WireEnvelope: Decodable {
    let type: String
}

extension ServerMessage {
    /// The `type` to route on. Anything that is not a JSON object is ``ServerMessageError/notJSON``;
    /// an object without a `type` has nothing to route on.
    fileprivate static func routingType(in data: Data, using decoder: JSONDecoder) throws -> String {
        do {
            return try decoder.decode(WireEnvelope.self, from: data).type
        } catch DecodingError.keyNotFound {
            throw ServerMessageError.missingType
        } catch DecodingError.valueNotFound {
            throw ServerMessageError.missingType
        } catch {
            throw ServerMessageError.notJSON
        }
    }

    /// A `mix_ack`, read as Android reads one (`ControlChannel.kt:169-177`). A numbered ack whose mix
    /// cannot be read still answers its number, as ``unreadableMixAck(sequence:)``.
    ///
    /// An ack with no number whose mix cannot be read is refused like any unreadable report. Android
    /// never reads the mix of an unnumbered ack and only counts it as acknowledged; this client has
    /// no message for "acknowledged, nothing to read", and the desk numbers the answer to every
    /// numbered `mix` (`apps/server/src/control.rs:2530-2570`), so that difference is named here
    /// rather than modelled.
    fileprivate static func mixAck(decoding data: Data, using decoder: JSONDecoder) throws
        -> ServerMessage
    {
        let sequence = try decoder.decode(WireSequence.self, from: data).value
        do {
            return .mixAck(try decoder.decode(WireMixReport.self, from: data).value(sequence: sequence))
        } catch {
            guard let sequence else { throw error }
            return .unreadableMixAck(sequence: sequence)
        }
    }

    /// Runs `build` and turns a missing required key into
    /// ``ServerMessageError/missingField(_:inMessageType:)``, so one translation covers every
    /// message instead of each one catching for itself.
    fileprivate static func translatingMissingFields(
        in type: String, _ build: () throws -> ServerMessage
    ) throws -> ServerMessage {
        do {
            return try build()
        } catch DecodingError.keyNotFound(let key, _) {
            throw ServerMessageError.missingField(key.stringValue, inMessageType: type)
        } catch let error as DecodingError {
            if case .valueNotFound(_, let context) = error, let key = context.codingPath.last {
                throw ServerMessageError.missingField(key.stringValue, inMessageType: type)
            }
            throw ServerMessageError.notJSON
        }
    }
}

/// Decodes an element when it can and yields nil when it cannot, so one unreadable entry never
/// costs the whole list it sits in.
private struct Lenient<Value: Decodable>: Decodable {
    let value: Value?

    init(from decoder: any Decoder) throws {
        value = try? Value(from: decoder)
    }
}

/// A `config`, read as leniently as the Android client reads it (`ControlChannel.kt:158-167`,
/// `parseGroups` at `:642-658`, `parseChannelDetails` at `:669-680`). The desk added fields over
/// time (the channel details after the groups, a group's `id` after its name and channels), so a
/// phone that insisted on every field would lose the whole configuration to an older desk.
///
/// - `source_channels` is the one field it cannot do without.
/// - `sample_rate` is read when present; Android never reads it, and every audio datagram carries
///   the rate that matters.
/// - `groups`: anything but a list is none; an entry that is not an object or has no name is
///   skipped; `id` is optional; a member channel that does not decode as a whole number, or is
///   negative, is dropped. Android's `optString` and `optInt` coerce where this drops (a name of
///   `7` becomes "7", a channel of `2.5` or "3" becomes 2 or 3); the desk only ever sends names
///   and whole numbers, so this is the one place the two read garbage differently.
/// - `channels`: anything but a list is no details at all (an older desk), which is not the same as
///   empty details; a list becomes exactly `source_channels` entries, an entry that is not an
///   object or a field of the wrong type reading as the default, an unknown kind as `other`.
private struct WireConfiguration: Decodable {
    let value: DeskConfiguration

    private enum CodingKeys: String, CodingKey {
        case sourceChannels = "source_channels"
        case sampleRate = "sample_rate"
        case groups
        case channels
    }

    /// A channel the engineer never touched: no name, kind `other`, shown, not linked.
    private static let untouched = SourceChannel(
        name: "", kind: .other, isHidden: false, isLinkedToNext: false)

    private struct WireGroup: Decodable {
        let group: ChannelGroup

        private enum CodingKeys: String, CodingKey {
            case id
            case name
            case channels
        }

        init(from decoder: any Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            guard let name = try? container.decodeIfPresent(String.self, forKey: .name), !name.isEmpty
            else {
                throw DecodingError.dataCorrupted(
                    .init(codingPath: decoder.codingPath, debugDescription: "a group with no name"))
            }
            let members = try? container.decodeIfPresent([Lenient<Int>].self, forKey: .channels)
            group = ChannelGroup(
                id: try? container.decodeIfPresent(String.self, forKey: .id),
                name: name,
                channels: (members ?? []).compactMap(\.value).filter { $0 >= 0 })
        }
    }

    private struct WireChannel: Decodable {
        let channel: SourceChannel

        private enum CodingKeys: String, CodingKey {
            case name
            case kind
            case hidden
            case link
        }

        init(from decoder: any Decoder) throws {
            let container = try decoder.container(keyedBy: CodingKeys.self)
            let kind = try? container.decodeIfPresent(String.self, forKey: .kind)
            channel = SourceChannel(
                name: (try? container.decodeIfPresent(String.self, forKey: .name)) ?? "",
                // An unknown kind is shown as `other`, which is the desk's own contract.
                kind: kind.flatMap(ChannelKind.init(rawValue:)) ?? .other,
                isHidden: (try? container.decodeIfPresent(Bool.self, forKey: .hidden)) == true,
                isLinkedToNext: (try? container.decodeIfPresent(Bool.self, forKey: .link)) == true)
        }
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let count = Int(try container.decode(UInt8.self, forKey: .sourceChannels))
        let groups = try? container.decodeIfPresent([Lenient<WireGroup>].self, forKey: .groups)
        let details = try? container.decodeIfPresent([Lenient<WireChannel>].self, forKey: .channels)
        value = DeskConfiguration(
            sourceChannels: UInt8(count),
            sampleRate: try? container.decodeIfPresent(UInt32.self, forKey: .sampleRate),
            groups: (groups ?? []).compactMap { $0.value?.group },
            channels: details.map { entries in
                (0..<count).map { index in
                    entries.indices.contains(index)
                        ? entries[index].value?.channel ?? Self.untouched : Self.untouched
                }
            })
    }
}

/// The number of the phone's mix an answer names (`seq`), read as Android's `answeredSequence`
/// reads it (`ControlChannel.kt:631`): a number that is not negative names one, and anything else
/// names none without costing the message. Android cuts a fraction off (`1.5` names 1) where this
/// names none; the desk only ever echoes the whole number the phone sent.
private struct WireSequence: Decodable {
    let value: UInt64?

    private enum CodingKeys: String, CodingKey { case seq }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        value = try? container.decodeIfPresent(UInt64.self, forKey: .seq)
    }
}

/// A whole mix report (`mix_ack`, `mix_state`), read as Android's `parseMixReport` and
/// `parseEngineerControls` read one (`ControlChannel.kt:601-628`).
///
/// - The mix is required whole: the master volume, the level cap, the mute and all five lists. The
///   desk always sends every field, so a report missing one, or with an entry of the wrong type, is
///   unreadable: a push is refused and never applied, and a numbered ack answers its number with
///   nothing to show. Android's `getInt` and `getBoolean` coerce where this refuses (a level of
///   `2.5` or `"3"`, a mute of `"true"`); the desk never sends either.
/// - The engineer's controls came later and are read leniently, so a desk that predates them still
///   drives this client. `engineer_limit_percent` is any number, held to 0-100 and cut to its whole
///   part, and anything else is no limit; `locked` locks the entries that are `true` and no other,
///   and anything but a list locks nothing; `following` is a string, an empty one still a leader,
///   and anything else is nobody.
private struct WireMixReport: Decodable {
    private let volumePercent: Int
    private let maxLevelPercent: Int
    private let muted: Bool
    private let channelGains: [Int]
    private let pans: [Int]
    private let channelMutes: [Bool]
    private let channelSolos: [Bool]
    private let groupLevels: [Int]
    private let engineerLimitPercent: Int
    private let locked: [Bool]?
    private let following: String?

    private enum CodingKeys: String, CodingKey {
        case volumePercent = "volume_percent"
        case maxLevelPercent = "max_level_percent"
        case muted
        case channels
        case pans
        case mutes
        case solos
        case groupLevels = "group_levels"
        case engineerLimitPercent = "engineer_limit_percent"
        case locked
        case following
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        volumePercent = try container.decode(Int.self, forKey: .volumePercent)
        maxLevelPercent = try container.decode(Int.self, forKey: .maxLevelPercent)
        muted = try container.decode(Bool.self, forKey: .muted)
        channelGains = try container.decode([Int].self, forKey: .channels)
        pans = try container.decode([Int].self, forKey: .pans)
        channelMutes = try container.decode([Bool].self, forKey: .mutes)
        channelSolos = try container.decode([Bool].self, forKey: .solos)
        groupLevels = try container.decode([Int].self, forKey: .groupLevels)

        // Read as a `Double` because Kotlin's `Number.toInt()` takes any number, a fraction too.
        // Held to the range before it becomes an `Int`, so a huge number cannot trap the
        // conversion (JSON has no NaN).
        let limit = try? container.decodeIfPresent(Double.self, forKey: .engineerLimitPercent)
        engineerLimitPercent =
            limit.map { Int($0.clamped(to: 0...100)) } ?? MixReport.noEngineerLimitPercent
        let locked = try? container.decodeIfPresent([Lenient<Bool>].self, forKey: .locked)
        self.locked = locked.map { entries in entries.map { $0.value == true } }
        following = try? container.decodeIfPresent(String.self, forKey: .following)
    }

    /// The report numbered `sequence`: the number of the phone's mix a `mix_ack` answers, or nil.
    func value(sequence: UInt64?) -> MixReport {
        MixReport(
            sequence: sequence,
            volumePercent: volumePercent,
            maxLevelPercent: maxLevelPercent,
            muted: muted,
            channelGains: channelGains,
            pans: pans,
            channelMutes: channelMutes,
            channelSolos: channelSolos,
            groupLevels: groupLevels,
            engineerLimitPercent: engineerLimitPercent,
            // A desk that predates the locks sends none: nothing locked.
            locked: locked ?? [Bool](repeating: false, count: channelGains.count),
            following: following)
    }
}

private struct WirePlayback: Decodable {
    let value: PlaybackBufferSetting

    private enum CodingKeys: String, CodingKey { case buffer }

    private struct WireBuffer: Decodable {
        let mode: String
        let ms: Int?
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let buffer = try container.decode(WireBuffer.self, forKey: .buffer)

        // A pinned cushion with no length, or a mode this build does not know, cannot be honoured;
        // adapting on our own is the safe reading, and dropping the message would guess too.
        guard buffer.mode == "fixed", let milliseconds = buffer.ms else {
            value = .auto
            return
        }
        let lowest = Int(PlaybackBufferSetting.minimumFixedMilliseconds)
        let highest = Int(PlaybackBufferSetting.maximumFixedMilliseconds)
        value = .fixed(milliseconds: UInt16(milliseconds.clamped(to: lowest...highest)))
    }
}

private struct WireRedundancy: Decodable {
    let value: UInt8

    private enum CodingKeys: String, CodingKey { case delayBlocks = "delay_blocks" }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let blocks = try container.decode(Int.self, forKey: .delayBlocks)
        value = UInt8(blocks.clamped(to: 0...Int(ServerMessage.maximumRedundancyHoldBlocks)))
    }
}

private struct WireError: Decodable {
    let value: DeskError

    private enum CodingKeys: String, CodingKey {
        case message
        case seq
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        value = DeskError(
            message: try container.decode(String.self, forKey: .message),
            sequence: try container.decodeIfPresent(UInt64.self, forKey: .seq))
    }
}

extension Comparable {
    fileprivate func clamped(to range: ClosedRange<Self>) -> Self {
        min(max(self, range.lowerBound), range.upperBound)
    }
}

// MARK: - Errors

/// Why a message from the desk could not be read. An unknown `type` is not here: it is
/// ``ServerMessage/unrecognized(type:)``, which is a message this build ignores, not a failure.
public enum ServerMessageError: Error, Equatable, Sendable {
    /// Not a JSON object this client can route on.
    case notJSON
    /// A JSON object with no `type`, so there is nothing to route it by.
    case missingType
    /// A known message type missing a field it cannot be read without.
    case missingField(String, inMessageType: String)
}
