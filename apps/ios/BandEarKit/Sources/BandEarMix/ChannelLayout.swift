import BandEarProtocol
import Foundation

/// The source channels one strip controls: `first` alone, or `first` and `second` as a stereo pair.
/// A strip is known by its first channel everywhere: the edits it reports, the sheet it opens, the
/// musician's channel.
public struct StripChannels: Hashable, Sendable {
    public let first: Int
    public let second: Int?

    public init(first: Int, second: Int? = nil) {
        self.first = first
        self.second = second
    }

    /// Every channel the strip writes, the first one first.
    public var sides: [Int] { [first] + (second.map { [$0] } ?? []) }
}

/// The two pans a stereo pair writes.
public struct StereoPans: Hashable, Sendable {
    public let first: Int
    public let second: Int

    public init(first: Int, second: Int) {
        self.first = first
        self.second = second
    }
}

/// How the desk's channels become strips, and how a stereo pair's balance becomes two pans.
/// Ported from the Android client's `ChannelLayout.kt`.
public enum ChannelLayout {
    private static let percent = 0...100

    /// The strips for `channelCount` source channels with the desk's `details`, left to right.
    /// `nil` details (an older desk announced none) show every channel as a strip of its own.
    ///
    /// A hidden channel has no strip. A channel that links to the next makes one strip of the two
    /// when both are shown; a link that cannot make a pair (on the last channel, or into a hidden
    /// one, which the desk never sends) leaves each channel to its own flags. A channel past the
    /// details is one the engineer never touched.
    public static func strips(channelCount: Int, details: [SourceChannel]?) -> [StripChannels] {
        func detail(_ channel: Int) -> SourceChannel? {
            guard let details, details.indices.contains(channel) else { return nil }
            return details[channel]
        }
        var strips: [StripChannels] = []
        var channel = 0
        while channel < channelCount {
            let shown = detail(channel)?.isHidden != true
            let pairs =
                detail(channel)?.isLinkedToNext == true && channel + 1 < channelCount && shown
                && detail(channel + 1)?.isHidden != true
            if pairs {
                strips.append(StripChannels(first: channel, second: channel + 1))
            } else if shown {
                strips.append(StripChannels(first: channel))
            }
            channel += pairs ? 2 : 1
        }
        return strips
    }

    /// The two pans a pair's `balance` writes (0 left, 50 centre, 100 right): the first pan is
    /// `max(0, 2b - 100)` and the second `min(100, 2b)`, the rule every client of the desk uses
    /// (`apps/server/src/protocol.rs:427-432`). The centre is the pair at full width, 0 and 100.
    public static func pans(balance: Int) -> StereoPans {
        let level = min(max(balance, percent.lowerBound), percent.upperBound)
        return StereoPans(first: max(0, 2 * level - 100), second: min(100, 2 * level))
    }

    /// The balance a pair's two pans show: their mean, halves rounded up, which reads back
    /// ``pans(balance:)``.
    ///
    /// Kotlin's `roundToInt` and Swift's `rounded()` disagree on negative halves (-0.5 becomes 0
    /// and -1). Both are then held to 0...100, which makes them agree everywhere.
    public static func balance(of pans: StereoPans) -> Int {
        let mean = (Double(pans.first) + Double(pans.second)) / 2
        return min(max(Int(mean.rounded()), percent.lowerBound), percent.upperBound)
    }

    /// How the strip of `channel` is named: the engineer's name, or "CH n" when it has none.
    public static func label(channel: Int, detail: SourceChannel?) -> String {
        let name = detail?.name.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        return name.isEmpty ? defaultName(channel: channel) : name
    }

    /// "CH 1" for channel 0: how a channel is named when the engineer gave it no name.
    public static func defaultName(channel: Int) -> String {
        "CH \(channel + 1)"
    }

    /// Where a channel is panned until someone moves it: even channels left, odd ones right, so a
    /// desk's adjacent inputs open as stereo (Android's `defaultPan`, `ClientMixState.kt:146`).
    public static func defaultPan(channel: Int) -> Int {
        channel % 2 == 0 ? 0 : 100
    }
}
