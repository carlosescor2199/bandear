import Foundation

/// The connection fields as typed, kept as text so a mistake can be corrected in place.
///
/// `audioPort` is this phone's own UDP listen port, the one the desk sends audio to. It applies to
/// a desk found over Bonjour too, exactly as on the Android client.
public struct ConnectionForm: Equatable, Sendable {
    /// The desk's WebSocket control port until the musician says otherwise (`DEFAULT_CONTROL_PORT`
    /// on Android, the desk's `--control-port` default).
    public static let defaultControlPort: UInt16 = 50_001
    /// This phone's UDP listen port until the musician says otherwise (`DEFAULT_AUDIO_PORT`).
    public static let defaultAudioPort: UInt16 = 50_000

    public var host: String
    public var controlPort: String
    public var audioPort: String

    public init(
        host: String = "",
        controlPort: String = String(ConnectionForm.defaultControlPort),
        audioPort: String = String(ConnectionForm.defaultAudioPort)
    ) {
        self.host = host
        self.controlPort = controlPort
        self.audioPort = audioPort
    }

    /// A connection to the desk typed by hand. Checked in the Android client's order (host, then
    /// this phone's port, then the desk's), so the same mistake gets the same answer on both.
    public func manualRequest() -> Result<ConnectionRequest, ConnectionFormError> {
        let host = host.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !host.isEmpty else { return .failure(.missingHost) }
        guard let audioPort = Self.port(from: audioPort) else { return .failure(.invalidAudioPort) }
        guard let controlPort = Self.port(from: controlPort) else {
            return .failure(.invalidControlPort)
        }
        return .success(
            ConnectionRequest(
                endpoint: .manual(host: host, controlPort: controlPort), audioPort: audioPort))
    }

    /// A connection to a desk found over Bonjour. Its address and control port come from the
    /// network, so only this phone's port is checked.
    public func request(for desk: DiscoveredDesk) -> Result<ConnectionRequest, ConnectionFormError> {
        guard let audioPort = Self.port(from: audioPort) else { return .failure(.invalidAudioPort) }
        return .success(ConnectionRequest(endpoint: desk.endpoint, audioPort: audioPort))
    }

    /// A port typed with or without blanks around it: 1 through 65535 (Android's `PORTS`).
    private static func port(from text: String) -> UInt16? {
        guard let value = Int(text.trimmingCharacters(in: .whitespacesAndNewlines)),
            (1...Int(UInt16.max)).contains(value)
        else { return nil }
        return UInt16(value)
    }
}

/// A checked request to connect: the desk, and the port this phone listens on for its audio.
public struct ConnectionRequest: Equatable, Sendable {
    public let endpoint: DeskEndpoint
    public let audioPort: UInt16

    public init(endpoint: DeskEndpoint, audioPort: UInt16) {
        self.endpoint = endpoint
        self.audioPort = audioPort
    }
}

/// Why the form cannot connect yet. The screen owns the words; see `ConnectionFormParityTests`
/// for the Android client's, which the screen must use.
public enum ConnectionFormError: Error, Equatable, Sendable {
    case missingHost
    case invalidAudioPort
    case invalidControlPort
}
