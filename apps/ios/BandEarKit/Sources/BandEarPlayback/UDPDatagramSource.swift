import Foundation

/// Where the receive loop's datagrams come from: the desk's UDP socket, or a fake in tests.
///
/// The loop calls ``receive(into:)`` from its own thread, once per wake-up. ``wake()`` may come from
/// any thread, to end a session at once; ``close()`` comes from the loop's thread once it has
/// stopped receiving, so the socket is never released while a receive may still use it.
public protocol DatagramSource: AnyObject {
    /// Waits for one datagram and copies it into `buffer`, answering its length in bytes, or `nil`
    /// when none came within the receive timeout. Throws ``DatagramSourceError/closed`` once the
    /// source was closed, and any other error when the socket fails.
    func receive(into buffer: UnsafeMutableRawBufferPointer) throws -> Int?

    /// Makes the receive under way, or the next one, answer at once, from any thread. The caller
    /// lowers the session flag first, so the loop sees it as soon as the receive answers. Does
    /// nothing once the source is closed.
    func wake()

    /// Releases the socket. Called once, on the receiving thread, after its last receive.
    func close()
}

/// Why a ``DatagramSource`` stopped answering.
public enum DatagramSourceError: Error, Equatable, Sendable {
    /// It was closed: the session ended, nothing failed.
    case closed
    /// The system refused `operation` with `errno` `code`.
    case socket(operation: String, code: Int32)
}

/// The phone's audio socket: IPv4 UDP on every interface at one port, with a receive timeout.
///
/// What Android's `DatagramSocket(port)` with `soTimeout = 250` does (`PmonPlayer.kt:160-163`): it
/// takes a datagram from any sender, with no `connect` and no filter on the source, so the desk can
/// send from whatever port it likes. IPv4 only, because the desk listens and sends on IPv4 only.
///
/// The timeout is the receive loop's only clock. A wake-up comes with every packet and at the
/// latest every ``receiveTimeoutMilliseconds``, and the loop sends its link reports and releases
/// its reorder hold on those wake-ups (``HoldPlayout``).
///
/// Stopping takes care on Darwin. Closing a socket from another thread does not reliably wake a
/// `recv` blocked on it, `shutdown` is refused on an unconnected UDP socket (`ENOTCONN`) and wakes
/// nothing, and the number of a closed descriptor can be handed to the next socket opened while a
/// receive still holds it. So:
/// - ``wake()``, from any thread, sends one empty datagram to this port on 127.0.0.1 from a
///   throwaway socket it closes at once. The receive under way answers it, and the loop, finding
///   the session flag down, ends without reading it. The source's lock is held while it sends, and
///   a closed source sends nothing, so the wake-up can only reach this socket, never a newer one
///   bound to the same port since. Were the datagram lost (a receive queue full, no socket to
///   spare), the receive would still answer within ``receiveTimeoutMilliseconds``.
/// - ``close()`` releases the descriptor, and only the receiving thread calls it, after its last
///   receive. No receive can then be using the number when the system hands it out again. A new
///   socket can bind the port as soon as it returns.
public final class UDPDatagramSource: DatagramSource, @unchecked Sendable {
    /// How long one receive waits before it wakes the loop with nothing: Android's `soTimeout`.
    public static let receiveTimeoutMilliseconds = 250

    /// The port the socket is bound to: the one asked for, or the one the system chose for 0.
    public let port: UInt16

    private let descriptor: Int32
    private let lock = NSLock()
    private var isClosed = false

    /// Opens the socket on `port` of every IPv4 interface (`INADDR_ANY`), or on a port the system
    /// chooses for 0. Throws ``DatagramSourceError/socket(operation:code:)`` when the system
    /// refuses, for example a port already in use.
    public init(
        port: UInt16, receiveTimeoutMilliseconds: Int = UDPDatagramSource.receiveTimeoutMilliseconds
    ) throws {
        let descriptor = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
        guard descriptor >= 0 else { throw DatagramSourceError.socket(operation: "socket", code: errno) }
        do {
            var timeout = timeval(
                tv_sec: receiveTimeoutMilliseconds / 1_000,
                tv_usec: __darwin_suseconds_t(receiveTimeoutMilliseconds % 1_000 * 1_000))
            try Self.check(
                "setsockopt",
                setsockopt(
                    descriptor, SOL_SOCKET, SO_RCVTIMEO, &timeout,
                    socklen_t(MemoryLayout<timeval>.size)))

            var address = Self.address(port: port, host: INADDR_ANY)
            try Self.check(
                "bind",
                withUnsafePointer(to: &address) {
                    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                        bind(descriptor, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
                    }
                })

            var bound = sockaddr_in()
            var length = socklen_t(MemoryLayout<sockaddr_in>.size)
            try Self.check(
                "getsockname",
                withUnsafeMutablePointer(to: &bound) {
                    $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                        getsockname(descriptor, $0, &length)
                    }
                })
            self.port = UInt16(bigEndian: bound.sin_port)
        } catch {
            Darwin.close(descriptor)
            throw error
        }
        self.descriptor = descriptor
    }

    deinit {
        // A source no loop ever received from, or one whose loop never closed it.
        if !isClosed { Darwin.close(descriptor) }
    }

    public func receive(into buffer: UnsafeMutableRawBufferPointer) throws -> Int? {
        while true {
            guard !closed else { throw DatagramSourceError.closed }
            let count = recv(descriptor, buffer.baseAddress, buffer.count, 0)
            let code = errno
            if count >= 0 { return count }
            if code == EAGAIN || code == EWOULDBLOCK { return nil }
            // A signal is not the timeout: wait again rather than count a wake-up that never was.
            if code == EINTR { continue }
            throw DatagramSourceError.socket(operation: "recv", code: code)
        }
    }

    public func wake() {
        lock.lock()
        defer { lock.unlock() }
        // Closed: the port may belong to a newer socket by now, which must not hear this wake-up.
        guard !isClosed else { return }
        let sender = socket(AF_INET, SOCK_DGRAM, IPPROTO_UDP)
        // No socket to spare: the receive under way still answers with its timeout.
        guard sender >= 0 else { return }
        defer { Darwin.close(sender) }
        var loopback = Self.address(port: port, host: in_addr_t(0x7F00_0001).bigEndian)
        _ = withUnsafePointer(to: &loopback) {
            $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
                sendto(sender, nil, 0, 0, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
            }
        }
    }

    public func close() {
        lock.lock()
        defer { lock.unlock() }
        guard !isClosed else { return }
        isClosed = true
        Darwin.close(descriptor)
    }

    private var closed: Bool {
        lock.lock()
        defer { lock.unlock() }
        return isClosed
    }

    /// An IPv4 address for `port` on `host`, both as the system reads them.
    private static func address(port: UInt16, host: in_addr_t) -> sockaddr_in {
        var address = sockaddr_in()
        address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size)
        address.sin_family = sa_family_t(AF_INET)
        address.sin_port = port.bigEndian
        address.sin_addr.s_addr = host
        return address
    }

    private static func check(_ operation: String, _ result: Int32) throws {
        guard result == 0 else {
            throw DatagramSourceError.socket(operation: operation, code: errno)
        }
    }
}
