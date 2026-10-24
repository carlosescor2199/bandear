import SwiftUI

/// The BandEar buttons (Android's `Buttons.kt`, in the design's colours): 44 pt tall at least, the
/// control radius, the label in the label face.
struct BandEarButtonStyle: ButtonStyle {
    enum Kind: Equatable, Sendable {
        /// The one main action of a screen or card, such as Connect or Save: the accent.
        case primary
        /// Any other action, such as Recall: raised, with an outline.
        case secondary
        /// An action that cannot be undone or ends something, such as Delete or Disconnect: red on
        /// its tint. Its words say what it does; red only adds to them.
        case danger
    }

    let kind: Kind
    /// Whether the button takes the whole width its container offers.
    var fillsWidth = false

    func makeBody(configuration: Configuration) -> some View {
        StyledButton(kind: kind, fillsWidth: fillsWidth, configuration: configuration)
    }

    /// The button itself, so it can read whether it is enabled.
    private struct StyledButton: View {
        let kind: Kind
        let fillsWidth: Bool
        let configuration: Configuration
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            let shape = BandEarShapes.rounded(BandEarShapes.control)
            configuration.label
                .bandEarFont(BandEarType.labelLarge)
                .foregroundStyle(foreground)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 16)
                .padding(.vertical, 6)
                .frame(maxWidth: fillsWidth ? .infinity : nil, minHeight: 44)
                .background(background, in: shape)
                .overlay(shape.strokeBorder(border, lineWidth: 1))
                .contentShape(shape)
                .opacity(configuration.isPressed ? 0.8 : 1)
        }

        private var foreground: Color {
            switch kind {
            case .primary: isEnabled ? BandEarColors.onAccent : BandEarColors.neutral
            case .secondary: isEnabled ? BandEarColors.text : BandEarColors.textSecondary
            case .danger: isEnabled ? BandEarColors.red : BandEarColors.textSecondary
            }
        }

        private var background: Color {
            switch kind {
            case .primary: isEnabled ? BandEarColors.accent : BandEarColors.surfaceRaised
            case .secondary: BandEarColors.surfaceRaised
            case .danger: isEnabled ? BandEarColors.redTint : BandEarColors.surfaceRaised
            }
        }

        private var border: Color {
            switch kind {
            case .primary: .clear
            case .secondary: BandEarColors.border
            case .danger: isEnabled ? BandEarColors.redTintBorder : BandEarColors.border
            }
        }
    }
}

/// The one main action of a screen or card, in the accent.
struct PrimaryButton: View {
    let title: String
    var fillsWidth = false
    let action: @MainActor () -> Void

    var body: some View {
        Button(title) { action() }
            .buttonStyle(BandEarButtonStyle(kind: .primary, fillsWidth: fillsWidth))
    }
}

/// Any other action: raised, with an outline.
struct SecondaryButton: View {
    let title: String
    var fillsWidth = false
    let action: @MainActor () -> Void

    var body: some View {
        Button(title) { action() }
            .buttonStyle(BandEarButtonStyle(kind: .secondary, fillsWidth: fillsWidth))
    }
}

/// An action that cannot be undone or ends something: red on its tint.
struct DangerButton: View {
    let title: String
    var fillsWidth = false
    let action: @MainActor () -> Void

    var body: some View {
        Button(title, role: .destructive) { action() }
            .buttonStyle(BandEarButtonStyle(kind: .danger, fillsWidth: fillsWidth))
    }
}
