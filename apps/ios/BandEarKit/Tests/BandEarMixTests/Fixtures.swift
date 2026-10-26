import BandEarProtocol
import Foundation
import Testing

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

/// The mix as the phone holds it, reported back whole: Android's `MixUiState.toSnapshot()`. With
/// `gains`, the report carries them in place of the stored levels, as the desk reports the lift it
/// plays while More of me is on.
func report(of mix: MixState, gains: [Int]? = nil) -> MixReport {
    report(
        gains: gains ?? mix.levels, pans: mix.pans, mutes: mix.mutes, solos: mix.solos,
        groupLevels: mix.groupLevels, volume: mix.masterVolume, maxLevel: mix.levelCap,
        muted: mix.masterMuted)
}

extension MixSession {
    /// The levels the desk plays while More of me is on, `locked` channels as stored: Android's
    /// `mix.toSnapshot().moreOfMeFor(moreOfMeChannels, locked).channelGains`.
    func played(locked: Set<Int> = []) -> [Int] {
        MoreOfMe.liftedLevels(mix.levels, musicianChannels: moreOfMeChannels, lockedChannels: locked)
    }
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

// MARK: - What the phone sends

/// A `mix` as the desk reads it: the JSON ``ClientMessage/mix(_:)`` writes, parsed back, so a test
/// checks keys and values and never their order. Android's tests read the sent `JSONObject` the
/// same way (`MonitorViewModelTest.kt:64-68`).
struct WireMix {
    let object: [String: Any]

    init(_ command: MixCommand) throws {
        let data = try ClientMessage.mix(command).encodedJSON()
        object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    }

    /// The whole numbers under `key`, or nil when there is no such list: Android's `ints(key)`.
    func ints(_ key: String) -> [Int]? { object[key] as? [Int] }

    func bools(_ key: String) -> [Bool]? { object[key] as? [Bool] }

    func int(_ key: String) -> Int? { object[key] as? Int }

    func has(_ key: String) -> Bool { object[key] != nil }
}

/// The phone as `MonitorViewModelTest.kt` drives it, with the socket left out.
///
/// Every event goes through ``MixSession/reducedNumberingSends(by:)``, and whenever that numbered a
/// new mix the phone sends ``MixSession/outgoingMix()`` as the session then stands
/// (`MonitorViewModel.kt:346-348,390-394`). Android's fake scheduler runs each send at once, so
/// ``sent`` is what its fake socket records as `sentOfType("mix")`.
struct Phone {
    private(set) var session = MixSession()
    /// Every `mix` sent, in order: the opening one, then one per numbered change.
    private(set) var sent: [MixCommand] = []
    /// How many of ``sent`` the desk has answered.
    private var answered = 0

    /// Android's `connected(channels)`: the session starts, the socket opens and sends the opening
    /// mix, and then the desk's `config` announces its channels.
    static func connected(_ config: DeskConfiguration) -> Phone {
        var phone = Phone()
        phone.apply(.sessionStarted)
        phone.opened()
        phone.apply(.configReceived(config))
        return phone
    }

    static func connected(_ channels: Int = 2) -> Phone {
        connected(config(channels))
    }

    /// The socket opened: the phone sends its mix as it stands, under its newest number, and numbers
    /// nothing new (`ControlChannel.kt:370-389`).
    mutating func opened() {
        sent.append(session.outgoingMix())
    }

    mutating func apply(_ events: MixEvent...) {
        for event in events {
            let before = session
            session = session.reducedNumberingSends(by: event)
            if session.sync.latest != before.sync.latest {
                sent.append(session.outgoingMix())
            }
        }
    }

    /// The desk's message `object`, read as the phone reads it. One that reports nothing about the
    /// mix changes nothing.
    mutating func receive(_ object: [String: Any]) throws {
        let data = try JSONSerialization.data(withJSONObject: object)
        if let event = MixEvent(reportedBy: try ServerMessage(decodingJSON: data)) {
            apply(event)
        }
    }

    /// Android's `answerMixes`: the desk answers every mix not answered yet, in order, each with
    /// the mix as sent under its own number.
    mutating func answerMixes() throws {
        let unanswered = sent.dropFirst(answered)
        answered = sent.count
        for mix in unanswered {
            var answer = try WireMix(mix).object
            answer["type"] = "mix_ack"
            try receive(answer)
        }
    }

    func lastMix() throws -> WireMix {
        try WireMix(try #require(sent.last))
    }
}
