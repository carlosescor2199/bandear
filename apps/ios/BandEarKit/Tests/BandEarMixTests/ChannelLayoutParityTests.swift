import BandEarProtocol
import Testing

@testable import BandEarMix

private let shown = info()
private let hidden = info(hidden: true)
private let linked = info(link: true)

private func strip(_ first: Int, _ second: Int? = nil) -> StripChannels {
    StripChannels(first: first, second: second)
}

private func strips(_ count: Int, _ details: [SourceChannel]?) -> [StripChannels] {
    ChannelLayout.strips(channelCount: count, details: details)
}

private func pans(_ first: Int, _ second: Int) -> StereoPans {
    StereoPans(first: first, second: second)
}

/// The Android client's `ChannelLayoutTest.kt`, case for case
/// (`apps/android/app/src/test/java/com/bandear/android/ui/state/`).
@Suite("Channel layout: Android parity")
struct ChannelLayoutParityTests {

    @Test func withoutDetailsEveryChannelIsAStripOfItsOwn() {
        #expect(strips(3, nil) == [strip(0), strip(1), strip(2)])
        #expect(strips(0, nil) == [])
    }

    @Test func hiddenChannelsHaveNoStrip() {
        #expect(strips(4, [shown, hidden, shown, hidden]) == [strip(0), strip(2)])
    }

    @Test func aLinkedChannelAndTheNextAreOneStripKnownByTheFirst() throws {
        let layout = strips(5, [shown, linked, shown, linked, shown])

        #expect(layout == [strip(0), strip(1, 2), strip(3, 4)])
        // Required before indexing, so a short layout fails this test instead of crashing the run.
        try #require(layout.count == 3)
        #expect(layout[1].sides == [1, 2])
        #expect(layout[0].sides == [0])
    }

    @Test func aHiddenPairHasNoStrip() {
        let details = [info(hidden: true, link: true), hidden, shown]

        #expect(strips(3, details) == [strip(2)])
    }

    @Test func aLinkThatCannotMakeAPairLeavesEachChannelAsItIs() {
        // The server never sends these; the phone still shows what it can. A link on the last
        // channel, or into a hidden channel, makes no pair; a link on a pair's second side is part
        // of the pair.
        #expect(strips(2, [shown, linked]) == [strip(0), strip(1)])
        #expect(strips(3, [linked, hidden, shown]) == [strip(0), strip(2)])
        #expect(strips(3, [linked, linked, shown]) == [strip(0, 1), strip(2)])
    }

    @Test func channelsPastTheDetailsAreChannelsTheEngineerNeverTouched() {
        #expect(strips(3, [linked]) == [strip(0, 1), strip(2)])
        // Details past the count have no channel to describe.
        #expect(strips(1, [linked, shown]) == [strip(0)])
    }

    @Test func aStripIsNamedByTheEngineerOrByItsNumber() {
        #expect(ChannelLayout.label(channel: 0, detail: info(" Vox ", .voice)) == "Vox")
        #expect(ChannelLayout.label(channel: 1, detail: info("  ")) == "CH 2")
        #expect(ChannelLayout.label(channel: 2, detail: nil) == "CH 3")
    }

    @Test func aBalanceIsWrittenAsBothPans() {
        #expect(ChannelLayout.pans(balance: 0) == pans(0, 0))
        #expect(ChannelLayout.pans(balance: 25) == pans(0, 50))
        #expect(ChannelLayout.pans(balance: 50) == pans(0, 100))
        #expect(ChannelLayout.pans(balance: 75) == pans(50, 100))
        #expect(ChannelLayout.pans(balance: 100) == pans(100, 100))
        // Out of range reads as the nearest end.
        #expect(ChannelLayout.pans(balance: -10) == pans(0, 0))
        #expect(ChannelLayout.pans(balance: 140) == pans(100, 100))
    }

    @Test func twoPansReadBackAsTheirMean() {
        #expect(ChannelLayout.balance(of: pans(0, 100)) == 50)
        #expect(ChannelLayout.balance(of: pans(10, 50)) == 30)
        // Halves round up, as the desktop rounds them.
        #expect(ChannelLayout.balance(of: pans(0, 1)) == 1)
        for balance in 0...100 {
            #expect(
                ChannelLayout.balance(of: ChannelLayout.pans(balance: balance)) == balance,
                "balance \(balance)")
        }
    }
}
