import BandEarScreens
import SwiftUI

// The parts Connect, Mixes and Settings share, as their boards draw them: a screen's title, the
// capital captions over its sections, the hint lines, the cards, and the boxed text field.

/// A screen's title, such as "Settings": larger in portrait, where the screen has the height for it.
struct ScreenTitle: View {
    let text: String
    @Environment(\.screenOrientation) private var orientation

    var body: some View {
        Text(text)
            .bandEarFont(orientation.isLandscape ? BandEarType.titleLarge : BandEarType.titleLarge.with(size: 28))
            .foregroundStyle(BandEarColors.text)
            .accessibilityAddTraits(.isHeader)
    }
}

/// The caption over a section, such as "YOUR NAME", in the words' own capitals. VoiceOver finds it
/// as a heading, as it found the section header it replaces.
struct SectionCaption: View {
    let text: String

    var body: some View {
        Text(text)
            .bandEarFont(BandEarType.caps)
            .foregroundStyle(BandEarColors.textSecondary)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A line that explains a section, under it.
struct SectionHint: View {
    let text: String

    var body: some View {
        Text(text)
            .bandEarFont(BandEarType.bodySmall)
            .foregroundStyle(BandEarColors.textSecondary)
            .fixedSize(horizontal: false, vertical: true)
    }
}

/// A ``ConnectionBanner`` on a card tinted by its tone, such as "Could not connect".
struct ConnectionBannerCard: View {
    let banner: ConnectionBanner

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.card)
        ConnectionBannerRow(banner: banner)
            .padding(14)
            .frame(maxWidth: .infinity, alignment: .leading)
            .background(BandEarColors.tint(banner.tone), in: shape)
            .overlay(shape.strokeBorder(BandEarColors.tintBorder(banner.tone), lineWidth: 1))
    }
}

/// The line between two rows of a card, inset from its sides.
struct CardDivider: View {
    var body: some View {
        Rectangle()
            .fill(BandEarColors.border)
            .frame(height: 1)
            .padding(.horizontal, 16)
            .accessibilityHidden(true)
    }
}

extension View {
    /// A card: the surface with an outline, at the panel radius.
    func screenCard() -> some View {
        let shape = BandEarShapes.rounded(BandEarShapes.panel)
        return background(BandEarColors.surface, in: shape)
            .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
    }

    /// A text field boxed on the surface, 52 pt tall, with its words in the medium text face.
    ///
    /// The field itself is only as tall as a line of text, so a tap anywhere else in the box calls
    /// `focus`, which focuses it: the whole box is the touch target.
    func boxedField(focus: @escaping @MainActor () -> Void) -> some View {
        let shape = BandEarShapes.rounded(BandEarShapes.control)
        return
            self
            .bandEarFont(BandEarType.bodyLarge.with(size: 16, weight: .medium))
            .foregroundStyle(BandEarColors.text)
            .tint(BandEarColors.accent)
            .padding(.horizontal, 16)
            .frame(minHeight: 52)
            .background(BandEarColors.surface, in: shape)
            .overlay(shape.strokeBorder(BandEarColors.border, lineWidth: 1))
            .contentShape(shape)
            .onTapGesture { focus() }
    }
}

/// The placeholder of a text field, in the grey the boards give it.
func fieldPrompt(_ words: String) -> Text {
    Text(words).foregroundColor(BandEarColors.neutral)
}
