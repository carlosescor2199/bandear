import BandEarDiscovery
import BandEarMix
import BandEarProtocol
import Foundation
import Testing

@testable import BandEarSession

/// The desk every test connects to, found over Bonjour as the browser spells it.
let stage = DeskEndpoint.bonjour(name: "BandEar on Stage", type: "_bandear._tcp", domain: "local.")

/// A request to connect to ``stage``, listening for audio on the phone's default port.
let toStage = ConnectionRequest(endpoint: stage, audioPort: 50_000)

/// A transport the test drives by hand: it records what the session opens, sends and closes, and
/// hands the test the session's event callback. It sends whatever it is given, so whether a message
/// goes out while the connection is down is the session's decision, which is what is under test.
///
/// Used from the main actor only; ``sink`` is a `@Sendable` closure a test may hand to a thread.
final class FakeTransport: ControlTransport, @unchecked Sendable {
    /// What the session asked of the transport, in the order it asked.
    enum Call: Equatable {
        case open(DeskEndpoint)
        case send(String)
        case close
    }

    /// One connection the session opened, as Android's `FakeSocket` is one socket: where it goes,
    /// what was sent while it was the newest, and the callback its events go to.
    final class Attempt: @unchecked Sendable {
        let endpoint: DeskEndpoint
        let sink: @Sendable (TransportEvent) -> Void
        fileprivate(set) var sent: [String] = []

        init(_ endpoint: DeskEndpoint, _ sink: @escaping @Sendable (TransportEvent) -> Void) {
            self.endpoint = endpoint
            self.sink = sink
        }

        /// The `type` of every message sent on this connection, in order: Android's `sentTypes()`.
        func sentTypes() throws -> [String] {
            try parsed(sent).map { $0["type"] as? String ?? "" }
        }

        /// Every message of `type` sent on this connection: Android's `sentOfType(type)`.
        func sent(ofType type: String) throws -> [[String: Any]] {
            try parsed(sent).filter { $0["type"] as? String == type }
        }
    }

    /// Every connection ``open(_:events:)`` started, in order.
    private(set) var attempts: [Attempt] = []
    /// Every call, in order.
    private(set) var calls: [Call] = []
    /// Every text sent, on any connection, in order.
    private(set) var sent: [String] = []
    private(set) var closes = 0

    /// Every endpoint ``open(_:events:)`` was asked for, in order.
    var opened: [DeskEndpoint] { attempts.map(\.endpoint) }

    /// What the latest ``open(_:events:)`` was told to report to.
    var sink: (@Sendable (TransportEvent) -> Void)? { attempts.last?.sink }

    func open(_ endpoint: DeskEndpoint, events: @escaping @Sendable (TransportEvent) -> Void) {
        attempts.append(Attempt(endpoint, events))
        calls.append(.open(endpoint))
    }

    /// Recorded against the newest connection, the only one the transport keeps.
    func send(_ text: String) {
        sent.append(text)
        attempts.last?.sent.append(text)
        calls.append(.send(text))
    }

    func close() {
        closes += 1
        calls.append(.close)
    }

    /// Every sent message as the desk reads it, parsed, so a test checks keys and values and never
    /// their order, as Android's tests read the sent `JSONObject`.
    func sentObjects() throws -> [[String: Any]] {
        try parsed(sent)
    }

    /// The `type` of every sent message, in order: Android's `sentTypes()`.
    func sentTypes() throws -> [String] {
        try sentObjects().map { $0["type"] as? String ?? "" }
    }

    /// Every sent message of `type`: Android's `sentOfType(type)`.
    func sent(ofType type: String) throws -> [[String: Any]] {
        try sentObjects().filter { $0["type"] as? String == type }
    }
}

/// `texts` as the desk reads them, each parsed to an object.
private func parsed(_ texts: [String]) throws -> [[String: Any]] {
    try texts.map { text in
        try #require(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any])
    }
}

/// A scheduler that runs nothing until the test says so, as Android's `FakeScheduler` does: the
/// session's retries wait here, with their delays, and a test fires them by hand.
@MainActor
final class ManualScheduler: ControlScheduler {
    /// One piece of work the session scheduled.
    final class Work: ScheduledTask {
        let delay: Duration
        let run: @MainActor () -> Void
        private(set) var cancelled = false
        fileprivate(set) var ran = false

        init(_ delay: Duration, _ run: @escaping @MainActor () -> Void) {
            self.delay = delay
            self.run = run
        }

        func cancel() { cancelled = true }
    }

    /// Everything ever scheduled, in order.
    private(set) var scheduled: [Work] = []

    func schedule(after delay: Duration, _ work: @escaping @MainActor () -> Void)
        -> any ScheduledTask
    {
        let work = Work(delay, work)
        scheduled.append(work)
        return work
    }

    /// The work neither cancelled nor run yet: Android's `pending()`.
    var pending: [Work] { scheduled.filter { !$0.cancelled && !$0.ran } }

    /// The delays of ``pending``: Android's `pendingDelays()`.
    var pendingDelays: [Duration] { pending.map(\.delay) }

    /// Runs the pending work with the shortest delay: Android's `runNextDue()`.
    func runNextDue(sourceLocation: SourceLocation = #_sourceLocation) throws {
        let next = try #require(
            pending.min { $0.delay < $1.delay }, "nothing is scheduled", sourceLocation: sourceLocation)
        next.ran = true
        next.run()
    }
}

/// The desk at the other end of a ``FakeTransport``, as Android's `FakeSocket` plays it.
///
/// Each event goes through the callback the session handed the transport, so it takes the same hop
/// to the main actor a real one does; the awaiting forms then ``settle()`` so the session has
/// handled it before the test looks.
@MainActor
final class FakeDesk {
    let transport: FakeTransport
    /// How many of the phone's mixes on each connection the desk has answered.
    private var answered: [ObjectIdentifier: Int] = [:]

    init(_ transport: FakeTransport) {
        self.transport = transport
    }

    /// Reports `event` to the session without waiting for the session to handle it.
    func post(_ event: TransportEvent) {
        transport.sink?(event)
    }

    func open() async {
        post(.opened)
        await settle()
    }

    func receive(_ json: String) async {
        post(.received(json))
        await settle()
    }

    func receive(_ object: [String: Any]) async throws {
        await receive(try text(object))
    }

    func closes() async {
        post(.closed)
        await settle()
    }

    /// The connection breaks: Android's `fail(message)`.
    func fails(_ reason: String) async {
        post(.failed(reason))
        await settle()
    }

    /// Android's `answerMixes`: the desk answers every mix of the newest connection it has not
    /// answered yet, in order, each with the mix as sent under its own number. With `settling` false
    /// the answers are only on their way, as answers already in flight when the musician moves on.
    func answerMixes(settling: Bool = true) async throws {
        let attempt = try #require(transport.attempts.last)
        let mixes = try attempt.sent(ofType: "mix")
        for var mix in mixes.dropFirst(answered[ObjectIdentifier(attempt), default: 0]) {
            mix["type"] = "mix_ack"
            post(.received(try text(mix)))
        }
        answered[ObjectIdentifier(attempt)] = mixes.count
        if settling { await settle() }
    }

    /// The newest mix the phone sent.
    func lastMix() throws -> [String: Any] {
        try #require(try transport.sent(ofType: "mix").last)
    }
}

/// `object` as the desk writes it.
func text(_ object: [String: Any]) throws -> String {
    String(decoding: try JSONSerialization.data(withJSONObject: object), as: UTF8.self)
}

/// Waits until every block already queued on the main queue has run, which is where the session
/// handles the transport's events.
///
/// The main queue runs blocks in order, so one queued now runs after them. A watchdog thread gives
/// up after `seconds` and fails the test, because a main queue that is never serviced would
/// otherwise hang the run for good (Swift Testing's time limit cannot stop that, as T3 learned).
@MainActor
func settle(seconds: Double = 5, sourceLocation: SourceLocation = #_sourceLocation) async {
    let ran = await withCheckedContinuation { (continuation: CheckedContinuation<Bool, Never>) in
        let once = ResumeOnce(continuation)
        let semaphore = DispatchSemaphore(value: 0)
        DispatchQueue.main.async {
            semaphore.signal()
            once.resume(returning: true)
        }
        Thread {
            if semaphore.wait(timeout: .now() + seconds) == .timedOut {
                once.resume(returning: false)
            }
        }.start()
    }
    #expect(ran, "the main queue did not run within \(seconds) s", sourceLocation: sourceLocation)
}

/// Waits until the work `start` hands out calls `done` on the main queue, for at most `seconds`,
/// and says whether it did. The main queue keeps running meanwhile, which is where that work runs;
/// a watchdog thread gives up as ``settle(seconds:sourceLocation:)``'s does.
@MainActor
func waitUntilDone(
    seconds: Double = 5, _ start: (_ done: @escaping @MainActor () -> Void) -> Void
) async -> Bool {
    await withCheckedContinuation { (continuation: CheckedContinuation<Bool, Never>) in
        let once = ResumeOnce(continuation)
        let semaphore = DispatchSemaphore(value: 0)
        start {
            semaphore.signal()
            once.resume(returning: true)
        }
        Thread {
            if semaphore.wait(timeout: .now() + seconds) == .timedOut {
                once.resume(returning: false)
            }
        }.start()
    }
}

/// Resumes a continuation once, whichever of the main queue and the watchdog gets there first.
private final class ResumeOnce: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Bool, Never>?

    init(_ continuation: CheckedContinuation<Bool, Never>) {
        self.continuation = continuation
    }

    func resume(returning value: Bool) {
        lock.lock()
        let pending = continuation
        continuation = nil
        lock.unlock()
        pending?.resume(returning: value)
    }
}

// MARK: - The desk's messages

/// The `config` a desk with `channels` channels and no groups sends on connect.
func configMessage(channels: Int) -> String {
    #"{"type":"config","source_channels":\#(channels),"groups":[]}"#
}

/// A mix report of `type` as the desk writes it (`mix_state` when the mix changed elsewhere,
/// `mix_ack` answering the phone's mix number `seq`): its full 32-channel and 16-group tables, with
/// `levels` on the first channels and the rest neutral. Android's `mixState` helper in
/// `MonitorViewModelTest.kt:97-114`.
func mixState(
    levels: [Int], volume: Int = 100, cap: Int = 100, muted: Bool = false,
    type: String = "mix_state", seq: Int? = nil
) -> [String: Any] {
    var object: [String: Any] = [
        "type": type,
        "volume_percent": volume,
        "max_level_percent": cap,
        "muted": muted,
        "channels": (0..<32).map { levels.indices.contains($0) ? levels[$0] : 100 },
        "pans": (0..<32).map { $0 % 2 == 0 ? 0 : 100 },
        "mutes": [Bool](repeating: false, count: 32),
        "solos": [Bool](repeating: false, count: 32),
        "group_levels": [Int](repeating: 100, count: 16),
    ]
    if let seq { object["seq"] = seq }
    return object
}

/// A mix report as the desk writes it: its full 32-channel and 16-group tables, and `seq` if any.
/// Android's `mixReport` helper in `ControlChannelTest.kt:47-59`.
func mixReport(_ type: String, firstLevel: Int = 10, seq: Int? = nil) -> [String: Any] {
    var object: [String: Any] = [
        "type": type,
        "volume_percent": 80,
        "max_level_percent": 90,
        "muted": true,
        "channels": (0..<32).map { firstLevel + $0 },
        "pans": (0..<32).map { $0 == 1 ? 25 : ($0 % 2 == 0 ? 0 : 100) },
        "mutes": (0..<32).map { $0 == 2 },
        "solos": (0..<32).map { $0 == 3 },
        "group_levels": (0..<16).map { 50 + $0 },
    ]
    if let seq { object["seq"] = seq }
    return object
}

/// The whole numbers under `key` of a parsed message: Android's `ints(key)`.
func ints(_ object: [String: Any], _ key: String) -> [Int]? {
    object[key] as? [Int]
}
