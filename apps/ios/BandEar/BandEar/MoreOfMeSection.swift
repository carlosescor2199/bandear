import BandEarMix
import BandEarScreens
import SwiftUI
import UIKit

/// The compact More of me switch in the Mix screen's header (Android's `MoreOfMeSwitch`): "More of
/// me" and a switch, in the accent on its tint while it lifts the musician's channel, plain while
/// it is off.
///
/// VoiceOver hears a switch named "More of me" whose value is the caption
/// (``MoreOfMeWords/caption(for:)``: "On · Lead vox lifted", "Off · tap to lift your channel"), so
/// the state is said in words. Drawn 36 pt tall in the middle of a 44 pt touch target; it is as wide
/// as its words and switch, or as wide as its container makes it, the switch then at the far end.
struct MoreOfMeSwitch: View {
    let mixSession: MixSession
    let apply: MixApply

    var body: some View {
        let on = mixSession.moreOfMe.isEnabled
        Toggle(
            isOn: Binding(
                get: { on },
                set: { apply(.moreOfMeToggled(on: $0)) })
        ) {
            Text(MoreOfMeWords.title)
                .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
                .foregroundStyle(on ? BandEarColors.accent : BandEarColors.text)
                .lineLimit(1)
                .fixedSize()
                .padding(.leading, 12)
                .frame(minHeight: 44)
        }
        .toggleStyle(BandEarSwitchStyle(compact: true))
        .accessibilityValue(MoreOfMeWords.caption(for: mixSession))
        .padding(.trailing, 6)
        .background {
            let shape = BandEarShapes.rounded(BandEarShapes.control)
            shape.fill(on ? BandEarColors.amberTint : BandEarColors.surface)
                .overlay(
                    shape.strokeBorder(on ? BandEarColors.amberTintBorder : BandEarColors.border, lineWidth: 1))
                .padding(.vertical, 4)
        }
    }
}

/// The musician's channel beside the More of me switch, and a menu of the announced channels to
/// change it: one entry per strip, by name, so a hidden channel is not offered and a stereo pair is
/// offered once (Android's `MyChannelPicker`). Drawn 36 pt tall in the middle of a 44 pt touch
/// target, with "MY CHANNEL" over the chosen channel.
struct MoreOfMeChannelPicker: View {
    let mixSession: MixSession
    let apply: MixApply

    var body: some View {
        let chosenLabel = ChannelPickerWords.chosenLabel(in: mixSession)
        let choices = mixSession.mix.musicianChoices
        Menu {
            Picker(
                selection: Binding(
                    get: { mixSession.moreOfMe.musicianChannel },
                    set: { apply(.musicianChannelChosen($0)) })
            ) {
                Text(ChannelPickerWords.none).tag(Int?.none)
                ForEach(choices, id: \.index) { choice in
                    Text(choice.label).tag(Int?.some(choice.index))
                }
            } label: {
                EmptyView()
            }
            .pickerStyle(.inline)
            if choices.isEmpty {
                Button(ChannelPickerWords.waiting) {}
                    .disabled(true)
            }
        } label: {
            let shape = BandEarShapes.rounded(BandEarShapes.control)
            HStack(spacing: 6) {
                VStack(alignment: .leading, spacing: 0) {
                    Text(ChannelPickerWords.title)
                        .bandEarFont(BandEarType.caps.with(size: 9))
                        .foregroundStyle(BandEarColors.textSecondary)
                    Text(ChannelPickerWords.text(chosenLabel))
                        .bandEarFont(BandEarType.bodySmall.with(weight: .semibold))
                        .foregroundStyle(BandEarColors.text)
                }
                .lineLimit(1)
                Image(systemName: "chevron.up.chevron.down")
                    .imageScale(.small)
                    .foregroundStyle(BandEarColors.textSecondary)
            }
            .padding(.horizontal, 10)
            .frame(minHeight: 36)
            .background(BandEarColors.surface, in: shape)
            .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
            .padding(.vertical, 4)
            .contentShape(Rectangle())
        }
        .accessibilityLabel(ChannelPickerWords.description(chosenLabel))
        .accessibilityHint(ChannelPickerWords.chooseHint)
    }
}

/// Why More of me is not doing what the musician last asked, as a line under the header in the
/// accent. ``SwiftUI/View/moreOfMeAnnouncements(_:)`` reads each new one out.
struct MoreOfMeNoticeLine: View {
    let notice: MoreOfMeNotice

    var body: some View {
        HStack(alignment: .firstTextBaseline, spacing: 6) {
            Image(systemName: "exclamationmark.circle")
                .imageScale(.small)
                .accessibilityHidden(true)
            Text(MoreOfMeWords.message(for: notice))
                .bandEarFont(BandEarType.bodySmall.with(weight: .medium))
                .fixedSize(horizontal: false, vertical: true)
        }
        .foregroundStyle(BandEarColors.accent)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }
}

extension View {
    /// Reads each new More of me notice out, as Android reads it as a polite live region.
    func moreOfMeAnnouncements(_ notice: MoreOfMeNotice?) -> some View {
        onChange(of: notice) { notice in
            guard let notice else { return }
            UIAccessibility.post(
                notification: .announcement, argument: MoreOfMeWords.message(for: notice))
        }
    }
}
