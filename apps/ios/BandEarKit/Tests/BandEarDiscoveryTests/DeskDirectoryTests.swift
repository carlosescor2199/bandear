import Testing

@testable import BandEarDiscovery

/// A record the way the desk announces itself (`apps/server/src/mdns.rs:50-69`): TXT `v` is the
/// discovery version and `sr` the sample rate in Hz.
private func record(
    _ name: String = "BandEar on STUDIO-PC", domain: String = "local.",
    txt: [String: String]? = ["v": "1", "sr": "48000"]
) -> BonjourRecord {
    BonjourRecord(name: name, type: "_bandear._tcp", domain: domain, txt: txt)
}

@Suite("Desk directory")
struct DeskDirectoryTests {

    @Test("browses the service type the desk announces, at the version it announces")
    func constants() {
        #expect(DeskDirectory.serviceType == "_bandear._tcp")
        #expect(DeskDirectory.supportedDiscoveryVersion == 1)
    }

    @Test("lists a desk with its name, where to reach it, and its sample rate")
    func listsADesk() {
        #expect(
            DeskDirectory.desks(from: [record()]) == [
                DiscoveredDesk(
                    name: "BandEar on STUDIO-PC",
                    endpoint: .bonjour(
                        name: "BandEar on STUDIO-PC", type: "_bandear._tcp", domain: "local."),
                    sampleRate: 48_000)
            ])
    }

    @Test("lists nothing when nothing was found")
    func emptySnapshot() {
        #expect(DeskDirectory.desks(from: []).isEmpty)
    }

    // MARK: The version, as the Android client reads its beacon's

    @Test("ignores a desk at a version it was not written against", arguments: ["2", "0", "", "one"])
    func ignoresOtherVersions(version: String) {
        #expect(DeskDirectory.desks(from: [record(txt: ["v": version, "sr": "48000"])]).isEmpty)
    }

    @Test("ignores a desk that announces no version")
    func ignoresMissingVersion() {
        #expect(DeskDirectory.desks(from: [record(txt: ["sr": "48000"])]).isEmpty)
    }

    /// The browser can report a service before its TXT record has been resolved, then send the
    /// record as a change. Without it the version is unknown, so the desk waits to be listed.
    @Test("waits to list a desk until its TXT record has arrived")
    func waitsForTXTRecord() {
        #expect(DeskDirectory.desks(from: [record(txt: nil)]).isEmpty)
    }

    @Test("ignores TXT keys it has never heard of")
    func ignoresExtraKeys() {
        let desks = DeskDirectory.desks(
            from: [record(txt: ["v": "1", "sr": "48000", "added_later": "yes"])])

        #expect(desks.map(\.sampleRate) == [48_000])
    }

    // MARK: The sample rate is informational

    /// The sample rate only labels the row. A desk is still worth connecting to without it, and
    /// the `config` message states the real rate once connected.
    @Test(
        "lists a desk whose sample rate is missing or unreadable, with no sample rate",
        arguments: [nil, "fast", "0", "-1", "99999999999", " 48000"] as [String?])
    func toleratesUnreadableSampleRate(sampleRate: String?) {
        var txt = ["v": "1"]
        txt["sr"] = sampleRate
        let desks = DeskDirectory.desks(from: [record(txt: txt)])

        #expect(desks.count == 1)
        #expect(desks.first?.sampleRate == nil)
    }

    // MARK: Several desks

    @Test("lists several desks rather than choosing one")
    func listsSeveral() {
        let desks = DeskDirectory.desks(
            from: [record("BandEar on STUDIO-PC"), record("BandEar on Rehearsal-Mac")])

        #expect(desks.count == 2)
    }

    /// The browser hands over an unordered set, so the order is decided here. By name, ignoring
    /// case and reading digits as numbers, so rows never jump around between two snapshots.
    @Test("orders desks by name, ignoring case and reading numbers as numbers")
    func ordersByName() {
        let desks = DeskDirectory.desks(
            from: [
                record("BandEar on Studio 10"), record("BandEar on alpha"),
                record("BandEar on studio 2"),
            ])

        #expect(desks.map(\.name) == ["BandEar on alpha", "BandEar on studio 2", "BandEar on Studio 10"])
    }

    @Test("gives the same order whatever order the snapshot came in")
    func orderIsStable() {
        let names = ["BandEar on C", "BandEar on A", "BandEar on B"]
        let forward = DeskDirectory.desks(from: names.map { record($0) })
        let backward = DeskDirectory.desks(from: names.reversed().map { record($0) })

        #expect(forward == backward)
    }

    /// mDNS renames the second of two services that claim one name. The desk's own tests expect
    /// exactly such a name (`apps/server/src/mdns.rs:349-350`).
    @Test("lists a desk renamed after a name conflict as a desk of its own")
    func conflictRenamedDeskIsSeparate() {
        let desks = DeskDirectory.desks(
            from: [record("BandEar on STUDIO-PC"), record("BandEar on STUDIO-PC (2)")])

        #expect(desks.map(\.name) == ["BandEar on STUDIO-PC", "BandEar on STUDIO-PC (2)"])
    }

    @Test("lists one name in two domains as two desks")
    func domainsAreDistinct() {
        let desks = DeskDirectory.desks(
            from: [record(domain: "local."), record(domain: "studio.example.")])

        #expect(desks.count == 2)
    }

    // MARK: The same service reported twice

    @Test("lists a service reported twice once, from the copy at the supported version")
    func duplicateKeepsSupportedCopy() {
        let desks = DeskDirectory.desks(
            from: [record(txt: ["v": "2", "sr": "96000"]), record(txt: ["v": "1", "sr": "48000"])])

        #expect(desks.map(\.sampleRate) == [48_000])
    }

    @Test("keeps the sample rate when every copy of a service agrees on it")
    func duplicatesThatAgree() {
        let desks = DeskDirectory.desks(from: [record(), record()])

        #expect(desks.map(\.sampleRate) == [48_000])
    }

    /// One copy is stale and nothing says which. An unknown rate is the truthful answer, and it
    /// does not depend on the order the browser's set happened to iterate in.
    @Test("drops the sample rate when two copies of a service disagree on it")
    func duplicatesThatDisagree() {
        let desks = DeskDirectory.desks(
            from: [record(txt: ["v": "1", "sr": "48000"]), record(txt: ["v": "1", "sr": "44100"])])

        #expect(desks.count == 1)
        #expect(desks.first?.sampleRate == nil)
    }
}
