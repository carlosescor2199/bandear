import SwiftUI

/// A row of joined choices of which one is selected, such as Channels and Groups on the Mix screen
/// (Android's `SegmentedChoice`): the selected one is raised and bolder, not only lighter.
///
/// VoiceOver hears it exactly as the native segmented `Picker` it replaces, with no label of its
/// own: each choice by its title, the selected one as selected, and its place among them.
///
/// Drawn 36 pt tall, as the design's header controls are (taller only for large Dynamic Type), in
/// the middle of a 44 pt frame: each choice's touch target runs past its drawn segment to the
/// frame's top and bottom. As wide as its choices need: it never shortens a title, so leave it the
/// room.
struct SegmentedChoice<Value: Hashable>: View {
    let options: [Value]
    @Binding var selection: Value
    let title: (Value) -> String

    var body: some View {
        HStack(spacing: 0) {
            ForEach(options, id: \.self) { option in
                let selected = option == selection
                Button {
                    selection = option
                } label: {
                    Text(title(option))
                        .bandEarFont(BandEarType.bodySmall.with(weight: selected ? .semibold : .medium))
                        .foregroundStyle(selected ? BandEarColors.text : BandEarColors.textSecondary)
                        .lineLimit(1)
                        .fixedSize(horizontal: true, vertical: false)
                        .padding(.horizontal, 12)
                        .frame(minHeight: 30)
                        .background(
                            selected ? BandEarColors.surfaceRaised : .clear,
                            in: BandEarShapes.rounded(BandEarShapes.key))
                        // The control's 3 pt inset and the frame's 4 pt margin, above and below.
                        .padding(.vertical, 7)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 3)
        .background {
            let shape = BandEarShapes.rounded(BandEarShapes.control)
            shape.fill(BandEarColors.surface)
                .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
                .padding(.vertical, 4)
        }
        .accessibilityRepresentation {
            Picker(selection: $selection) {
                ForEach(options, id: \.self) { option in
                    Text(title(option)).tag(option)
                }
            } label: {
                EmptyView()
            }
            .pickerStyle(.segmented)
        }
    }
}
