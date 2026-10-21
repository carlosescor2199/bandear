/// Reads little-endian fields out of a byte collection in order. Works off offsets from
/// `startIndex` rather than absolute integers, so a `Data` slice that does not start at zero reads
/// the same as an array.
private struct ByteReader<Bytes: RandomAccessCollection> where Bytes.Element == UInt8 {
    private let bytes: Bytes
    private var offset: Int

    init(_ bytes: Bytes, startingAt offset: Int = 0) {
        self.bytes = bytes
        self.offset = offset
    }

    mutating func nextByte() -> UInt8 {
        defer { offset += 1 }
        return bytes[bytes.index(bytes.startIndex, offsetBy: offset)]
    }

    mutating func nextBytes(_ count: Int) -> [UInt8] {
        let start = bytes.index(bytes.startIndex, offsetBy: offset)
        let end = bytes.index(start, offsetBy: count)
        defer { offset += count }
        return Array(bytes[start..<end])
    }

    mutating func nextLittleEndian<Value>(_: Value.Type) -> Value
    where Value: FixedWidthInteger & UnsignedInteger {
        var value: Value = 0
        for byteIndex in 0..<(Value.bitWidth / 8) {
            value |= Value(nextByte()) << (byteIndex * 8)
        }
        return value
    }
}

/// The twenty bytes every audio datagram opens with: `PMON`, a version, the channel count, the
/// sample rate, the stream's block counter and how many samples follow.
///
/// Layout and field order are the server's, at `apps/server/src/protocol.rs:476-498`. There is no
/// timestamp: `sequence` is the engine's block counter and the only ordering the client gets.
public struct AudioDatagramHeader: Equatable, Sendable {
    /// `4 + 1 + 1 + 4 + 8 + 2`, matching `HEADER_SIZE` at `apps/server/src/protocol.rs:9`.
    public static let byteCount = 20

    static let magic: [UInt8] = Array("PMON".utf8)
    static let supportedVersion: UInt8 = 1

    public let channels: UInt8
    public let sampleRate: UInt32
    public let sequence: UInt64
    public let sampleCount: Int

    /// Samples per channel. Safe to divide: a header with no channels never decodes.
    public var frameCount: Int { sampleCount / Int(channels) }

    /// How many payload bytes `sampleCount` PCM16 samples take.
    public var payloadByteCount: Int { sampleCount * 2 }

    public init(decoding bytes: some RandomAccessCollection<UInt8>) throws {
        guard bytes.count >= Self.byteCount else {
            throw AudioDatagramDecodingError.headerTooShort(byteCount: bytes.count)
        }

        var reader = ByteReader(bytes)

        let magic = reader.nextBytes(4)
        guard magic == Self.magic else {
            throw AudioDatagramDecodingError.unknownMagic(magic)
        }

        let version = reader.nextByte()
        guard version == Self.supportedVersion else {
            throw AudioDatagramDecodingError.unsupportedVersion(version)
        }

        let channels = reader.nextByte()
        guard channels > 0 else {
            throw AudioDatagramDecodingError.zeroChannels
        }

        let sampleRate = reader.nextLittleEndian(UInt32.self)
        let sequence = reader.nextLittleEndian(UInt64.self)
        let sampleCount = Int(reader.nextLittleEndian(UInt16.self))

        guard sampleCount % Int(channels) == 0 else {
            throw AudioDatagramDecodingError.sampleCountNotDivisibleByChannels(
                sampleCount: sampleCount, channels: channels)
        }

        self.channels = channels
        self.sampleRate = sampleRate
        self.sequence = sequence
        self.sampleCount = sampleCount
    }
}

/// A whole audio datagram: its header and the interleaved PCM16 samples behind it.
///
/// This is the convenient shape, and it allocates. The render path reads
/// ``AudioDatagramHeader`` alone and copies the payload straight into its own buffer instead.
public struct AudioDatagram: Equatable, Sendable {
    public let header: AudioDatagramHeader
    public let samples: [Int16]

    public init(decoding bytes: some RandomAccessCollection<UInt8>) throws {
        let header = try AudioDatagramHeader(decoding: bytes)
        let payloadByteCount = bytes.count - AudioDatagramHeader.byteCount

        guard payloadByteCount == header.payloadByteCount else {
            throw AudioDatagramDecodingError.payloadLengthMismatch(
                declaredSampleCount: header.sampleCount, payloadByteCount: payloadByteCount)
        }

        var reader = ByteReader(bytes, startingAt: AudioDatagramHeader.byteCount)
        var samples = [Int16]()
        samples.reserveCapacity(header.sampleCount)
        for _ in 0..<header.sampleCount {
            samples.append(Int16(bitPattern: reader.nextLittleEndian(UInt16.self)))
        }

        self.header = header
        self.samples = samples
    }
}

/// Why a datagram was refused. Each case names what disagreed, so a client can log the reason
/// instead of a bare count of dropped packets.
public enum AudioDatagramDecodingError: Error, Equatable, Sendable {
    /// Fewer bytes arrived than a header needs.
    case headerTooShort(byteCount: Int)
    /// The first four bytes were not `PMON`; something else is on the port.
    case unknownMagic([UInt8])
    /// A protocol version this client was not written against.
    case unsupportedVersion(UInt8)
    /// A channel count of zero, which would make the frame count meaningless.
    case zeroChannels
    /// The payload is not exactly `declaredSampleCount` PCM16 samples long.
    case payloadLengthMismatch(declaredSampleCount: Int, payloadByteCount: Int)
    /// The sample count leaves a frame half finished, so the stream would go out of phase.
    case sampleCountNotDivisibleByChannels(sampleCount: Int, channels: UInt8)
}
