import BandEarScreens
import SwiftUI

/// A strip's Mute or Solo key, or the master's Mute key (Android's `ToggleKey` in
/// `MuteSoloButtons.kt`): raised while off; filled while on, red for mute and the accent for solo,
/// with dark text, so its state shows by the fill and not by a tint alone.
///
/// VoiceOver hears it exactly as the native button-style `Toggle` the strips used before: a button
/// named `voiceOverLabel` ("Mute CH 3", ``StripWords/muteDescription(for:)``) that says it is
/// selected while on. `onToggle` gets the state the key asks for, the opposite of `isOn`.
///
/// A ``Size/letter`` key ("M", "S") is 28 pt tall in mono digits, a ``Size/word`` key ("Mute" on the
/// master strip) 32 pt in the text face; both take the width their container gives them. Inside a
/// disabled view it fades to 40% but keeps its colours, so it still shows whether it is on.
struct ConsoleKey: View {
    enum Kind: Equatable, Sendable {
        case mute
        case solo
    }

    enum Size: Equatable, Sendable {
        /// A strip's "M" or "S": 28 pt, mono.
        case letter
        /// A word such as the master's "Mute": 32 pt, the text face.
        case word
    }

    let text: String
    let kind: Kind
    var size: Size = .letter
    let isOn: Bool
    let voiceOverLabel: String
    let onToggle: @MainActor (Bool) -> Void

    @Environment(\.isEnabled) private var isEnabled

    var body: some View {
        let shape = BandEarShapes.rounded(BandEarShapes.key)
        let onColour = kind == .mute ? BandEarColors.red : BandEarColors.accent
        let offText = size == .letter ? BandEarColors.textSecondary : BandEarColors.text
        Button {
            onToggle(!isOn)
        } label: {
            Text(text)
                .bandEarFont(
                    size == .letter
                        ? BandEarType.number.with(size: 12) : BandEarType.bodySmall.with(weight: .semibold))
                .foregroundStyle(isOn ? BandEarColors.onAccent : offText)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
                .padding(.horizontal, 2)
                .frame(maxWidth: .infinity, minHeight: size == .letter ? 28 : 32)
                .background(isOn ? onColour : BandEarColors.surfaceRaised, in: shape)
                .overlay(shape.strokeBorder(isOn ? onColour : BandEarColors.border, lineWidth: 1))
                .contentShape(shape)
        }
        .buttonStyle(PressedDimStyle())
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityRepresentation {
            Toggle(isOn: Binding(get: { isOn }, set: { onToggle($0) })) {
                Text(voiceOverLabel)
            }
            .toggleStyle(.button)
        }
    }
}

/// Why a strip takes no input, in the keys' place: a lock or the following mark and the words
/// ("Locked", "Following"), never told by the strip's fade alone. VoiceOver reads the words; the
/// symbol is hidden.
struct HeldTag: View {
    enum Reason: Equatable, Sendable {
        /// The engineer locked the strip.
        case locked
        /// The mix follows another musician's.
        case following

        /// The reason a strip's tag gives, or `nil` when the tag is a group's or "No group".
        init?(_ tag: StripTag) {
            switch tag {
            case .locked: self = .locked
            case .following: self = .following
            case .group, .noGroup: return nil
            }
        }

        var text: String {
            switch self {
            case .locked: StripWords.lockedTag
            case .following: StripWords.followingTag
            }
        }

        var symbol: String {
            switch self {
            case .locked: "lock.fill"
            case .following: "person.2.fill"
            }
        }
    }

    let reason: Reason

    var body: some View {
        HStack(spacing: 4) {
            Image(systemName: reason.symbol)
                .imageScale(.small)
                .accessibilityHidden(true)
            Text(reason.text)
                .lineLimit(1)
                .minimumScaleFactor(0.7)
        }
        .bandEarFont(BandEarType.labelSmall)
        .foregroundStyle(BandEarColors.text)
        .padding(.horizontal, 4)
        .frame(maxWidth: .infinity, minHeight: 28)
        .background(BandEarColors.surfaceRaised, in: BandEarShapes.rounded(BandEarShapes.tag))
        .accessibilityElement(children: .combine)
    }
}

/// A button style that only dims its label while pressed, for controls that draw their own look.
struct PressedDimStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .opacity(configuration.isPressed ? 0.7 : 1)
    }
}
