import Foundation

/// Where to reach a desk.
public enum DeskEndpoint: Hashable, Sendable {
    /// Found over Bonjour. Network resolves the address and the control port (the service's SRV
    /// record) when the connection opens, so neither is known at discovery time.
    case bonjour(name: String, type: String, domain: String)
    /// Typed by hand in the connection form.
    case manual(host: String, controlPort: UInt16)
}

/// A desk the musician can pick from the list.
public struct DiscoveredDesk: Hashable, Sendable, Identifiable {
    /// What the list shows: the Bonjour instance name, `BandEar on <host>` as the desk writes it.
    public let name: String
    public let endpoint: DeskEndpoint
    /// The rate the desk announced in its TXT record, or nil when it is missing, unreadable, or
    /// two copies of the service disagree. It only labels the row; `config` states the real rate.
    public let sampleRate: UInt32?

    public var id: DeskEndpoint { endpoint }

    public init(name: String, endpoint: DeskEndpoint, sampleRate: UInt32?) {
        self.name = name
        self.endpoint = endpoint
        self.sampleRate = sampleRate
    }
}

/// One Bonjour result as the browser reported it, before any BandEar rule is applied. Plain data,
/// so the rules below are tested without a network.
public struct BonjourRecord: Hashable, Sendable {
    public let name: String
    public let type: String
    public let domain: String
    /// The TXT record's keys and values, or nil when the browser has not delivered one yet.
    public let txt: [String: String]?

    public init(name: String, type: String, domain: String, txt: [String: String]?) {
        self.name = name
        self.type = type
        self.domain = domain
        self.txt = txt
    }
}

/// Turns what the Bonjour browser currently sees into the list of desks the musician picks from.
///
/// The browser hands over its *whole* current set on every change, so this is one pure function
/// from that set to the list, with no add, remove or change bookkeeping to get wrong.
///
/// Deliberately absent: the Android client's rule that drops a desk not heard from for 5 seconds
/// (`ServerBeacon.STALE_TIMEOUT_MS`). That rule exists because a UDP beacon has no way to say it
/// stopped. Bonjour does: the desk sends a goodbye when it unregisters (`apps/server/src/mdns.rs`),
/// and the operating system expires records it stops hearing. A desk that is present raises no new
/// events, it simply stays in the set, so a timer keyed on the last event would remove live desks.
/// The browser's set is the source of truth for what is there.
public enum DeskDirectory {
    /// The service type the desk announces (`SERVICE_TYPE`, `apps/server/src/mdns.rs:17`), in the
    /// form `NWBrowser` takes it.
    public static let serviceType = "_bandear._tcp"

    /// `DISCOVERY_VERSION`, carried in TXT `v`. The Android client ignores a beacon of any other
    /// version, and so does this: a desk at another version speaks a protocol this build does not.
    public static let supportedDiscoveryVersion = 1

    /// The desks in `records`, listed once each and ordered by name.
    ///
    /// - A record with no TXT record yet, or whose `v` is not the supported version, is not listed.
    ///   The type is not checked: the browser was asked for this type only, and its exact spelling
    ///   in a result (with or without a trailing dot) is the browser's business, not a rule here.
    /// - `sr` is read leniently: anything but a positive whole number is no sample rate.
    /// - A service reported more than once is listed once. When its copies disagree on the sample
    ///   rate, the rate is dropped: one copy is stale and nothing says which, and the answer must
    ///   not depend on the order the browser's set happened to iterate in.
    public static func desks(from records: some Sequence<BonjourRecord>) -> [DiscoveredDesk] {
        var sampleRates: [DeskEndpoint: [UInt32?]] = [:]
        var names: [DeskEndpoint: String] = [:]

        for record in records where isSupported(record) {
            let endpoint = DeskEndpoint.bonjour(
                name: record.name, type: record.type, domain: record.domain)
            names[endpoint] = record.name
            sampleRates[endpoint, default: []].append(sampleRate(in: record))
        }

        return names.map { endpoint, name in
            let rates = Set(sampleRates[endpoint] ?? [])
            return DiscoveredDesk(
                name: name, endpoint: endpoint, sampleRate: rates.count == 1 ? rates.first! : nil)
        }
        .sorted(by: listOrder)
    }

    private static func isSupported(_ record: BonjourRecord) -> Bool {
        guard let version = record.txt?["v"] else { return false }
        return Int(version) == supportedDiscoveryVersion
    }

    private static func sampleRate(in record: BonjourRecord) -> UInt32? {
        guard let text = record.txt?["sr"], let rate = UInt32(text), rate > 0 else { return nil }
        return rate
    }

    /// By name, ignoring case and reading digits as numbers ("Studio 2" before "Studio 10"), then
    /// by domain and type so that no two different desks ever compare equal and the order is total.
    private static func listOrder(_ left: DiscoveredDesk, _ right: DiscoveredDesk) -> Bool {
        let byName = left.name.compare(right.name, options: [.caseInsensitive, .numeric])
        if byName != .orderedSame { return byName == .orderedAscending }
        guard case .bonjour(_, let leftType, let leftDomain) = left.endpoint,
            case .bonjour(_, let rightType, let rightDomain) = right.endpoint
        else { return false }
        if leftDomain != rightDomain { return leftDomain < rightDomain }
        if leftType != rightType { return leftType < rightType }
        return left.name < right.name
    }
}
