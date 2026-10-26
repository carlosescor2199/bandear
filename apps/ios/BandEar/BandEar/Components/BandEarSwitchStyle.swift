import SwiftUI

/// The BandEar switch: a track in the accent with a dark knob while on, and the fader track's grey
/// with a grey knob while off, as the design draws it. The native switch tinted with the accent
/// keeps a white knob and the system's grey track, which the dark stage look does not use.
///
/// Its label sits on the leading side and the switch on the trailing side, as the native switch
/// lays them out; the whole row toggles it. VoiceOver hears it exactly as the native switch: its
/// label, "switch button", and on or off; a `.accessibilityValue` or `.accessibilityHint` set on the
/// `Toggle` is read with it.
///
/// `compact` draws the 36 by 22 pt switch that sits inside the More of me button; otherwise it is
/// 44 by 26 pt. The caller gives the row its height (44 pt for a touch target).
struct BandEarSwitchStyle: ToggleStyle {
    var compact = false

    func makeBody(configuration: Configuration) -> some View {
        let track = compact ? CGSize(width: 36, height: 22) : CGSize(width: 44, height: 26)
        let knob = track.height - 4
        let isOn = configuration.isOn
        Button {
            configuration.isOn.toggle()
        } label: {
            HStack(spacing: 8) {
                configuration.label
                Spacer(minLength: 8)
                Capsule()
                    .fill(isOn ? BandEarColors.accent : BandEarColors.faderTrack)
                    .frame(width: track.width, height: track.height)
                    .overlay(alignment: isOn ? .trailing : .leading) {
                        Circle()
                            .fill(isOn ? BandEarColors.onAccent : BandEarColors.textSecondary)
                            .frame(width: knob, height: knob)
                            .padding(2)
                    }
                    .animation(.easeOut(duration: 0.15), value: isOn)
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(PressedDimStyle())
        .accessibilityRepresentation {
            // Named here, or the representation would take this style from the environment again.
            Toggle(isOn: configuration.$isOn) {
                configuration.label
            }
            .toggleStyle(.switch)
        }
    }
}
