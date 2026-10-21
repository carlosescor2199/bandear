import Foundation
import Testing

@testable import BandEarProtocol

/// The packet the server's own test pins down, byte for byte: two stereo samples at 48 kHz,
/// sequence 9, carrying -1 and 0x1234. Source: `apps/server/src/protocol.rs:656-671`.
private let goldenPacket: [UInt8] = [
    0x50, 0x4D, 0x4F, 0x4E,  // "PMON"
    1,  // version
    2,  // channels
    0x80, 0xBB, 0x00, 0x00,  // sample rate 48000, little endian
    9, 0, 0, 0, 0, 0, 0, 0,  // sequence 9, little endian
    2, 0,  // sample count 2, little endian
    0xFF, 0xFF,  // -1
    0x34, 0x12,  // 4660
]

/// A valid packet with `sampleCount` stereo samples of silence, for the cases where only the
/// header fields matter.
private func packet(
    version: UInt8 = 1,
    channels: UInt8 = 2,
    sampleRate: UInt32 = 48_000,
    sequence: UInt64 = 0,
    sampleCount: UInt16,
    payloadByteCount: Int? = nil
) -> [UInt8] {
    var bytes: [UInt8] = Array("PMON".utf8)
    bytes.append(version)
    bytes.append(channels)
    bytes.append(contentsOf: withUnsafeBytes(of: sampleRate.littleEndian, Array.init))
    bytes.append(contentsOf: withUnsafeBytes(of: sequence.littleEndian, Array.init))
    bytes.append(contentsOf: withUnsafeBytes(of: sampleCount.littleEndian, Array.init))
    bytes.append(contentsOf: [UInt8](repeating: 0, count: payloadByteCount ?? Int(sampleCount) * 2))
    return bytes
}

@Suite("PMON audio datagram")
struct AudioDatagramTests {
    @Test("the header is the documented twenty bytes")
    func headerByteCount() {
        #expect(AudioDatagramHeader.byteCount == 20)
    }

    @Test("decodes the packet the server's test pins down")
    func decodesGoldenPacket() throws {
        let datagram = try AudioDatagram(decoding: goldenPacket)

        #expect(datagram.header.channels == 2)
        #expect(datagram.header.sampleRate == 48_000)
        #expect(datagram.header.sequence == 9)
        #expect(datagram.header.sampleCount == 2)
        #expect(datagram.samples == [-1, 4660])
    }

    @Test("reads one stereo frame out of the golden packet")
    func goldenFrameCount() throws {
        #expect(try AudioDatagram(decoding: goldenPacket).header.frameCount == 1)
    }

    @Test("reads an eight byte sequence as little endian")
    func readsWideSequence() throws {
        let header = try AudioDatagramHeader(
            decoding: packet(sequence: 0x0102_0304_0506_0708, sampleCount: 0))

        #expect(header.sequence == 0x0102_0304_0506_0708)
    }

    @Test("reads a four byte sample rate as little endian")
    func readsSampleRate() throws {
        let header = try AudioDatagramHeader(decoding: packet(sampleRate: 44_100, sampleCount: 0))

        #expect(header.sampleRate == 44_100)
    }

    @Test("accepts a packet that carries no samples")
    func acceptsEmptyPayload() throws {
        let datagram = try AudioDatagram(decoding: packet(sampleCount: 0))

        #expect(datagram.samples.isEmpty)
        #expect(datagram.header.frameCount == 0)
    }

    @Test("decodes a payload the length of the declared sample count")
    func decodesEveryDeclaredSample() throws {
        let datagram = try AudioDatagram(decoding: packet(sampleCount: 480))

        #expect(datagram.samples.count == 480)
        #expect(datagram.header.frameCount == 240)
    }

    @Test("refuses a header cut short", arguments: [0, 1, 19])
    func refusesShortHeader(byteCount: Int) {
        let bytes = Array(goldenPacket.prefix(byteCount))

        #expect(throws: AudioDatagramDecodingError.headerTooShort(byteCount: byteCount)) {
            try AudioDatagramHeader(decoding: bytes)
        }
    }

    @Test("refuses a datagram that is not PMON")
    func refusesUnknownMagic() {
        var bytes = goldenPacket
        bytes[0] = 0x51  // "Q"

        #expect(throws: AudioDatagramDecodingError.unknownMagic([0x51, 0x4D, 0x4F, 0x4E])) {
            try AudioDatagramHeader(decoding: bytes)
        }
    }

    @Test("refuses a version it was not written against", arguments: [0, 2, 255] as [UInt8])
    func refusesUnsupportedVersion(version: UInt8) {
        #expect(throws: AudioDatagramDecodingError.unsupportedVersion(version)) {
            try AudioDatagramHeader(decoding: packet(version: version, sampleCount: 0))
        }
    }

    @Test("refuses a stream with no channels")
    func refusesZeroChannels() {
        #expect(throws: AudioDatagramDecodingError.zeroChannels) {
            try AudioDatagramHeader(decoding: packet(channels: 0, sampleCount: 0))
        }
    }

    @Test("refuses a payload shorter than the declared sample count")
    func refusesTruncatedPayload() {
        let bytes = packet(sampleCount: 4, payloadByteCount: 6)

        #expect(
            throws: AudioDatagramDecodingError.payloadLengthMismatch(
                declaredSampleCount: 4, payloadByteCount: 6)
        ) {
            try AudioDatagram(decoding: bytes)
        }
    }

    @Test("refuses a payload longer than the declared sample count")
    func refusesOverlongPayload() {
        let bytes = packet(sampleCount: 2, payloadByteCount: 10)

        #expect(
            throws: AudioDatagramDecodingError.payloadLengthMismatch(
                declaredSampleCount: 2, payloadByteCount: 10)
        ) {
            try AudioDatagram(decoding: bytes)
        }
    }

    @Test("refuses a sample count that leaves a stereo frame half finished")
    func refusesOddSampleCountOnStereo() {
        #expect(
            throws: AudioDatagramDecodingError.sampleCountNotDivisibleByChannels(
                sampleCount: 3, channels: 2)
        ) {
            try AudioDatagramHeader(decoding: packet(sampleCount: 3))
        }
    }

    @Test("checks the magic before the version")
    func magicIsCheckedFirst() {
        var bytes = packet(version: 9, sampleCount: 0)
        bytes[0] = 0x51

        #expect(throws: AudioDatagramDecodingError.unknownMagic([0x51, 0x4D, 0x4F, 0x4E])) {
            try AudioDatagramHeader(decoding: bytes)
        }
    }

    @Test("decodes out of a slice that does not start at zero")
    func decodesFromOffsetSlice() throws {
        let padded = Data([0xAA, 0xBB, 0xCC] + goldenPacket)
        let slice = padded[padded.startIndex.advanced(by: 3)...]

        #expect(slice.startIndex != 0)

        let datagram = try AudioDatagram(decoding: slice)

        #expect(datagram.header.sequence == 9)
        #expect(datagram.samples == [-1, 4660])
    }
}
