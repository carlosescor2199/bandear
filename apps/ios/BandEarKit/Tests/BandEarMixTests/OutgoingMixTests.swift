import BandEarProtocol
import Foundation
import Testing

@testable import BandEarMix

/// The outgoing mix and the send rule beyond the Android parity suite: the rules Android's tests
/// only reach through its ViewModel and its socket.
@Suite("Outgoing mix")
struct OutgoingMixTests {

    /// Every `mix` is numbered, the first one too (the desk's answers are matched by it), and the
    /// number is the newest change's, not the newest the desk answered.
    @Test("goes out under the number of the newest change, from the first mix on")
    func numberedByTheNewestChange() throws {
        #expect(MixSession().outgoingMix().sequence == 0)
        #expect(try WireMix(MixSession().outgoingMix()).int("seq") == 0)

        let twoSent = connectedWith(2)
            .reducedNumberingSends(by: .channelLevelChanged(channel: 0, level: 40))
            .reducedNumberingSends(by: .channelLevelChanged(channel: 0, level: 50))
        let firstAnswered = twoSent.reduced(by: .mixAnswered(sequence: 1, report: nil))

        #expect(twoSent.outgoingMix().sequence == 2)
        #expect(firstAnswered.sync.answered == 1)
        #expect(firstAnswered.outgoingMix().sequence == 2)
    }

    @Test("lifts the musician's strip only in what it sends, and sends everything else as stored")
    func liftOnlyInWhatIsSent() {
        let on = connectedWith(3, [group("A", [0]), group("B", [1, 2])]).after(
            .channelLevelChanged(channel: 0, level: 80), .channelPanChanged(channel: 1, pan: 30),
            .channelMuteChanged(channel: 2, muted: true), .channelSoloChanged(channel: 0, soloed: true),
            .groupLevelChanged(group: 1, level: 60), .masterVolumeChanged(level: 70),
            .levelCapChanged(level: 90), .masterMuteChanged(muted: true),
            .musicianChannelChosen(1), .moreOfMeToggled(on: true))

        #expect(
            on.outgoingMix()
                == MixCommand(
                    sequence: 0, volumePercent: 70, maxLevelPercent: 90, muted: true,
                    channelGains: [40, 100, 50], pans: [0, 30, 0], channelMutes: [false, false, true],
                    channelSolos: [true, false, false], groupLevels: [100, 60]))
        #expect(on.mix.levels == [80, 100, 100])
        #expect(on.after(.moreOfMeToggled(on: false)).outgoingMix().channelGains == [80, 100, 100])
    }

    /// The keys `buildMixMessage` writes, and only those (`ControlChannel.kt:491-503`): every list on
    /// every `mix`, an empty one as an empty list, and nothing of what the engineer set, even while
    /// the engineer limits, locks and makes the mix follow another's.
    @Test("writes every list Android writes, an empty one too, and nothing the engineer set")
    func wireShape() throws {
        let held = connectedWith(2).after(
            .mixPushed(
                report(
                    gains: [70, 70], pans: [0, 100], mutes: [false, false], solos: [false, false]
                ).with(EngineerControls(limitPercent: 60, lockedChannels: [0], following: "Ana"))))
        try #require(held.mix.following == "Ana" && held.mix.engineer.isLimited)

        let wire = try WireMix(held.outgoingMix())

        #expect(
            Set(wire.object.keys) == [
                "type", "seq", "volume_percent", "max_level_percent", "muted", "channels", "pans",
                "mutes", "solos", "group_levels",
            ])
        #expect(wire.ints("channels") == [70, 70])
        #expect(wire.ints("group_levels") == [])
        #expect(wire.int("max_level_percent") == 100)
    }

    /// The desk already plays what it reports, so a report is never sent back, even one that changed
    /// the mix; nor is the start or the end of a session.
    @Test("never sends for the desk's reports or the start and end of a session")
    func reportsAndSessionEventsNeverSend() throws {
        let settled = connectedWith(2)
        let outstanding = settled.reducedNumberingSends(by: .channelLevelChanged(channel: 0, level: 40))
        let unmuted = [false, false]
        let pushed = MixEvent.mixPushed(
            report(gains: [80, 70], pans: [0, 100], mutes: unmuted, solos: unmuted))
        let answered = MixEvent.mixAnswered(
            sequence: 1, report: report(gains: [30, 70], pans: [0, 100], mutes: unmuted, solos: unmuted))
        let cases: [(MixSession, MixEvent)] = [
            (settled, pushed), (outstanding, answered),
            (outstanding, .mixAnswered(sequence: 1, report: nil)),
            (MixSession(), .sessionStarted), (settled, .sessionEnded),
        ]

        for (before, event) in cases {
            let after = before.reduced(by: event)
            // Each one changes the session, so a rule that sent whatever changed would send it.
            try #require(after != before)

            #expect(!MixSession.sends(event, before: before, after: after))
            #expect(before.reducedNumberingSends(by: event) == after)
        }
    }

    /// The one report the phone answers with a send is the first after a link turned More of me off,
    /// and the reducer numbers that one itself (`MonitorViewModel.kt:390-394`): through this rule it
    /// is numbered once, not twice.
    @Test("numbers the restore after a link once, as the reducer does")
    func restoreNumberedOnce() throws {
        let paired = running().after(
            .configReceived(config(3)), .channelLevelChanged(channel: 0, level: 80),
            .channelLevelChanged(channel: 1, level: 60), .musicianChannelChosen(1),
            .moreOfMeToggled(on: true),
            .configReceived(config(3, [], [info("Keys", .keys, link: true), info(), info()])))
        try #require(paired.ownLevelsToRestore != nil)
        // The link's `mix_state`: the second side took the first side's lifted level, pans apart.
        let unmuted = [false, false, false]
        let link = MixEvent.mixPushed(
            report(gains: [40, 40, 50], pans: [0, 100, 0], mutes: unmuted, solos: unmuted))

        let restored = paired.reducedNumberingSends(by: link)

        #expect(restored == paired.reduced(by: link))
        #expect(restored.sync.latest == paired.sync.latest + 1)
        #expect(restored.outgoingMix().channelGains == [80, 80, 100])
        #expect(restored.outgoingMix().pans == [0, 100, 0])
    }

    /// Android sends a `config` only when it turned More of me off, so the desk stops lifting
    /// (`MonitorViewModel.kt:318-321`); its parity suite only reaches the link, which is not sent.
    @Test("sends a config that turned More of me off, and one that left it alone not at all")
    func configSendsWhenItTurnedMoreOfMeOff() {
        let on = connectedWith(5).after(.musicianChannelChosen(3), .moreOfMeToggled(on: true))

        let dropped = on.reducedNumberingSends(by: .configReceived(config(2)))
        let kept = on.reducedNumberingSends(by: .configReceived(config(5)))

        #expect(dropped.moreOfMe.notice == .channelOutOfRange)
        #expect(dropped.sync.latest == on.sync.latest + 1)
        #expect(dropped.outgoingMix().channelGains == [100, 100])
        #expect(kept.moreOfMe.isEnabled)
        #expect(kept.sync == on.sync)
    }

    /// Android sends a choice only when it was taken while More of me was on: the mix sent then lifts
    /// another channel, or none (`MonitorViewModel.kt:419-425`). Choosing the channel already chosen
    /// sends the same mix again, as Android's rule does.
    @Test("sends a channel choice only when it was taken while More of me was on")
    func choiceSendsOnlyWhileOn() {
        let off = connectedWith(3)
        let on = off.after(.musicianChannelChosen(0), .moreOfMeToggled(on: true))

        let chosenWhileOff = off.reducedNumberingSends(by: .musicianChannelChosen(1))
        let moved = on.reducedNumberingSends(by: .musicianChannelChosen(1))
        let refused = on.reducedNumberingSends(by: .musicianChannelChosen(9))
        let again = on.reducedNumberingSends(by: .musicianChannelChosen(0))

        #expect(chosenWhileOff.moreOfMe.musicianChannel == 1)
        #expect(chosenWhileOff.sync == off.sync)
        #expect(moved.sync.latest == on.sync.latest + 1)
        #expect(moved.outgoingMix().channelGains == [50, 100, 50])
        #expect(refused == on)
        #expect(again.sync.latest == on.sync.latest + 1)
    }

    /// Android sends a toggle only when More of me actually turned on or off
    /// (`MonitorViewModel.kt:414-416`).
    @Test("sends a toggle only when More of me actually turned on or off")
    func toggleSendsOnlyWhenItSwitched() {
        let on = connectedWith(2).after(.musicianChannelChosen(0), .moreOfMeToggled(on: true))

        let onAgain = on.reducedNumberingSends(by: .moreOfMeToggled(on: true))
        let turnedOff = on.reducedNumberingSends(by: .moreOfMeToggled(on: false))

        #expect(onAgain.sync == on.sync)
        #expect(turnedOff.sync.latest == on.sync.latest + 1)
        #expect(turnedOff.outgoingMix().channelGains == [100, 100])
    }
}
