import BandEarProtocol

@testable import BandEarMix

/// A channel's details, with the Android client's `ChannelInfo` defaults: no name, kind `other`,
/// shown, not linked.
func info(
    _ name: String = "", _ kind: ChannelKind = .other, hidden: Bool = false, link: Bool = false
) -> SourceChannel {
    SourceChannel(name: name, kind: kind, isHidden: hidden, isLinkedToNext: link)
}

/// A group as Android's `GroupInfo(name, channels)`; it has no id there, so the name stands in.
func group(_ name: String, _ channels: [Int]) -> ChannelGroup {
    ChannelGroup(id: name, name: name, channels: channels)
}

let vox = info("Vox", .voice)
let keys = info("Keys", .keys, link: true)
let keysRight = info("Keys R", .keys)
let spare = info("Spare", hidden: true)

/// Five channels: a voice, a keys pair on channels 1 and 2, a hidden spare and an untouched input.
/// Android's `band` fixture in `MonitorReducerTest.kt:330-336`.
let band = [vox, keys, keysRight, spare, info()]

/// The mix a first `config` builds: neutral levels, even channels panned left and odd ones right,
/// nothing muted or soloed, every group neutral. Built here by hand rather than through the code
/// under test, so a wrong default there is caught rather than copied. T5b ports the tests that
/// reach this state through the reducer.
func firstConfig(
    channels count: Int, groups: [ChannelGroup] = [], details: [SourceChannel]? = nil
) -> MixState {
    var mix = MixState()
    mix.sourceChannels = count
    mix.levels = Array(repeating: 100, count: count)
    mix.pans = (0..<count).map { $0 % 2 == 0 ? 0 : 100 }
    mix.mutes = Array(repeating: false, count: count)
    mix.solos = Array(repeating: false, count: count)
    mix.groups = groups
    mix.groupLevels = Array(repeating: 100, count: groups.count)
    mix.channelDetails = details
    return mix
}

/// A `config` as the desk sends it: `count` channels, its groups, and its details or none.
func config(
    _ count: Int, _ groups: [ChannelGroup] = [], _ details: [SourceChannel]? = nil
) -> DeskConfiguration {
    DeskConfiguration(sourceChannels: UInt8(count), sampleRate: nil, groups: groups, channels: details)
}

extension MixSession {
    /// The session after `events`, in order: Android's `after` helper.
    func after(_ events: MixEvent...) -> MixSession {
        events.reduce(self) { $0.reduced(by: $1) }
    }
}

/// A session whose first `config` announced `count` channels and `groups`, with no details.
func withChannels(_ count: Int, _ groups: [ChannelGroup] = []) -> MixSession {
    MixSession().after(.configReceived(config(count, groups)))
}

/// A session whose first `config` announced the five-channel ``band``.
func withBand(_ groups: [ChannelGroup] = []) -> MixSession {
    MixSession().after(.configReceived(config(5, groups, band)))
}
