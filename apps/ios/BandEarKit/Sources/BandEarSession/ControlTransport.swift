import BandEarDiscovery

/// What happens to one control connection, in the order the transport reports it.
public enum TransportEvent: Equatable, Sendable {
    /// The WebSocket handshake completed: messages can go both ways.
    case opened
    /// A text message from the desk. Only text frames carry messages; the desk sends no others.
    case received(String)
    /// The connection ended without an error, as when the desk closes it.
    case closed
    /// The connection could not be opened, or broke, described for diagnostics.
    case failed(String)
}

/// How the control session reaches the desk.
///
/// The session is written against this port, so its rules are proved with a fake from the test
/// target and the network stays in one adapter, ``WebSocketTransport``, proved by its own round
/// trip. Android draws the same seam (`ControlTransport.kt`) for the same reason.
///
/// One connection at a time: ``open(_:events:)`` replaces any connection already open or opening,
/// and after ``close()`` or a later ``open(_:events:)`` the earlier connection reports nothing
/// more. Events may arrive on any thread, but always in order.
public protocol ControlTransport: AnyObject {
    /// Starts connecting to `endpoint`, and reports what happens to that connection to `events`.
    func open(_ endpoint: DeskEndpoint, events: @escaping @Sendable (TransportEvent) -> Void)

    /// Sends `text` as one text message on the open connection, or drops it when none is open.
    func send(_ text: String)

    /// Closes the connection with a normal closure, or stops it opening. Nothing more is reported.
    func close()
}
