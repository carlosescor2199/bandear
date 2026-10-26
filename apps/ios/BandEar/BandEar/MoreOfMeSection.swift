import BandEarMix
import BandEarScreens
import SwiftUI
import UIKit

/// More of me and the musician's channel (Android's `MoreOfMeRow`): the switch with its caption,
/// the "My channel" menu, and why More of me is not doing what the musician last asked.
struct MoreOfMeSection: View {
    let mixSession: MixSession
    let apply: MixApply

    var body: some View {
        let moreOfMe = mixSession.moreOfMe
        let chosenLabel = ChannelPickerWords.chosenLabel(in: mixSession)
        Section {
            Toggle(
                isOn: Binding(
                    get: { moreOfMe.isEnabled },
                    set: { apply(.moreOfMeToggled(on: $0)) })
            ) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(MoreOfMeWords.title)
                        .font(.headline)
                    Text(MoreOfMeWords.caption(for: mixSession))
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                }
            }
            .tint(.orange)
            MoreOfMeChannelPicker(
                chosen: moreOfMe.musicianChannel, chosenLabel: chosenLabel,
                choices: mixSession.mix.musicianChoices, apply: apply)
        } footer: {
            if let notice = moreOfMe.notice {
                Text(MoreOfMeWords.message(for: notice))
                    .font(.footnote.weight(.medium))
                    .foregroundStyle(.orange)
            }
        }
        // Android reads a new notice out as a polite live region.
        .onChange(of: moreOfMe.notice) { notice in
            guard let notice else { return }
            UIAccessibility.post(
                notification: .announcement, argument: MoreOfMeWords.message(for: notice))
        }
    }
}

/// The musician's channel, and a menu of the announced channels to change it: one entry per strip,
/// by name, so a hidden channel is not offered and a stereo pair is offered once
/// (Android's `MyChannelPicker`).
private struct MoreOfMeChannelPicker: View {
    let chosen: Int?
    let chosenLabel: String?
    let choices: [ChannelChoice]
    let apply: MixApply

    var body: some View {
        LabeledContent {
            Menu {
                Picker(
                    selection: Binding(
                        get: { chosen },
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
                HStack(spacing: 4) {
                    Text(ChannelPickerWords.text(chosenLabel))
                        .lineLimit(1)
                    Image(systemName: "chevron.up.chevron.down")
                        .font(.caption)
                }
            }
            .accessibilityLabel(ChannelPickerWords.description(chosenLabel))
            .accessibilityHint(ChannelPickerWords.chooseHint)
        } label: {
            Text(ChannelPickerWords.title)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
        }
    }
}
