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

// MARK: The desk's reports

/// A whole mix as the desk reports it, with the defaults of Android's `MixSnapshot`: no lists,
/// master and cap at 100, unmuted. The engineer set nothing, as an older desk reports.
func report(
    gains: [Int] = [], pans: [Int] = [], mutes: [Bool] = [], solos: [Bool] = [],
    groupLevels: [Int] = [], volume: Int = 100, maxLevel: Int = 100, muted: Bool = false
) -> MixReport {
    MixReport(
        sequence: nil, volumePercent: volume, maxLevelPercent: maxLevel, muted: muted,
        channelGains: gains, pans: pans, channelMutes: mutes, channelSolos: solos,
        groupLevels: groupLevels, engineerLimitPercent: MixReport.noEngineerLimitPercent,
        locked: [], following: nil)
}

/// The mix as the phone holds it, reported back whole: Android's `MixUiState.toSnapshot()`.
func report(of mix: MixState) -> MixReport {
    report(
        gains: mix.levels, pans: mix.pans, mutes: mix.mutes, solos: mix.solos,
        groupLevels: mix.groupLevels, volume: mix.masterVolume, maxLevel: mix.levelCap,
        muted: mix.masterMuted)
}

extension MixReport {
    /// This report carrying the engineer's `controls`, as the desk writes them: a limit, one
    /// `locked` entry per channel up to the last locked one, and whom the mix follows. Android hands
    /// the controls beside the mix (`MixPushed(snapshot, engineer)`); the desk sends them inside it.
    func with(_ controls: EngineerControls) -> MixReport {
        let lockedCount = (controls.lockedChannels.max() ?? -1) + 1
        return MixReport(
            sequence: sequence, volumePercent: volumePercent, maxLevelPercent: maxLevelPercent,
            muted: muted, channelGains: channelGains, pans: pans, channelMutes: channelMutes,
            channelSolos: channelSolos, groupLevels: groupLevels,
            engineerLimitPercent: controls.limitPercent,
            locked: (0..<lockedCount).map(controls.isLocked), following: controls.following)
    }
}

extension MixSession {
    /// `event` as the musician's change that the phone sends, numbered as its newest mix: Android's
    /// `sent` helper over `reduceChange`.
    func sent(_ event: MixEvent) -> MixSession {
        reducedSending(event) { _, _ in true }
    }
}

/// A session that started and has heard nothing from its desk yet: Android's `running()`.
func running() -> MixSession {
    MixSession().after(.sessionStarted)
}

/// A running session whose first `config` announced `count` channels and `groups`.
func connectedWith(_ count: Int, _ groups: [ChannelGroup] = []) -> MixSession {
    running().after(.configReceived(config(count, groups)))
}
