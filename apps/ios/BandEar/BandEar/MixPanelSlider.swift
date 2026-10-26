import BandEarScreens
import SwiftUI

/// A slider across a panel, as the design's "Channel panel" boards draw the pan and the level: a
/// 4 pt track, a fill up to the thumb for a level, a mark at the centre for a pan, and a light
/// rounded thumb, in a row 44 pt tall. The channel panel's pan and level and the master panel's
/// level cap are these.
///
/// It moves as the strip's pan does (``FaderTouchSurface``, ``HorizontalFaderGesture``): a sideways
/// drag moves the thumb from wherever it is, a tap moves it to the tapped point, and on
/// ``FaderScale/pan`` the centre holds a detent; a drag up or down scrolls the panel instead. For
/// VoiceOver it is one adjustable element that says `voiceOverLabel` and `voiceOverValue`, and a
/// swipe up or down moves it ``FaderMath/screenReaderStep``, as the strip's controls move.
///
/// Stateless: it shows `value` and reports a new one through `onChange`, only when it changes, as
/// `0...100`. Inside a disabled view it fades to 40% and takes no input.
struct PanelSlider: View {
    let value: Int
    let scale: FaderScale
    /// The fill from the left end to the thumb, for a level; a pan has none.
    let fill: Color?
    let voiceOverLabel: String
    let voiceOverValue: String
    let onChange: @MainActor (Int) -> Void

    @Environment(\.isEnabled) private var isEnabled
    @State private var input: PanelSliderInput

    init(
        value: Int, scale: FaderScale = .level, fill: Color?, voiceOverLabel: String,
        voiceOverValue: String, onChange: @escaping @MainActor (Int) -> Void
    ) {
        self.value = value
        self.scale = scale
        self.fill = fill
        self.voiceOverLabel = voiceOverLabel
        self.voiceOverValue = voiceOverValue
        self.onChange = onChange
        _input = State(initialValue: PanelSliderInput(scale: scale))
    }

    fileprivate static let thumb = CGSize(width: 22, height: 22)

    var body: some View {
        Canvas { context, size in
            draw(in: &context, size: size)
        }
        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44)
        .overlay(
            FaderTouchSurface(
                axis: .horizontal, isEnabled: isEnabled,
                onLength: { input.gesture.width = $0 },
                onTap: { report(input.gesture.tap(atX: $0, current: value)) },
                onDragStart: { input.gesture.startDrag(current: value) },
                onDrag: { report(input.gesture.drag(byX: $0, current: value)) })
        )
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(voiceOverLabel)
        .accessibilityValue(voiceOverValue)
        .accessibilityAdjustableAction { direction in
            guard isEnabled else { return }
            switch direction {
            case .increment: report(input.gesture.increment(current: value))
            case .decrement: report(input.gesture.decrement(current: value))
            @unknown default: break
            }
        }
    }

    private func report(_ next: Int?) {
        if let next { onChange(next) }
    }

    private func draw(in context: inout GraphicsContext, size: CGSize) {
        let thumb = Self.thumb
        let midY = size.height / 2
        // The thumb's edge travels the width minus the thumb, as the gesture measures it, so the
        // thumb is drawn where a tap reads its value.
        let travel = max(size.width - thumb.width, 0)
        let track = CGRect(x: thumb.width / 2, y: midY - 2, width: travel, height: 4)
        context.fill(Path(roundedRect: track, cornerRadius: 2), with: .color(BandEarColors.faderTrack))
        let thumbX = travel - CGFloat(scale.offset(of: value, travel: Double(travel)))
        let centreX = thumbX + thumb.width / 2
        if let fill {
            let filled = CGRect(x: track.minX, y: track.minY, width: max(centreX - track.minX, 0), height: 4)
            context.fill(Path(roundedRect: filled, cornerRadius: 2), with: .color(fill))
        }
        if scale == .pan {
            let mark = CGRect(x: size.width / 2 - 1, y: midY - 9, width: 2, height: 18)
            context.fill(Path(roundedRect: mark, cornerRadius: 1), with: .color(BandEarColors.neutral))
        }
        let thumbRect = CGRect(x: thumbX, y: midY - thumb.height / 2, width: thumb.width, height: thumb.height)
        context.drawLayer { shadowed in
            shadowed.addFilter(.shadow(color: .black.opacity(0.6), radius: 1.5, x: 0, y: 1))
            shadowed.fill(Path(roundedRect: thumbRect, cornerRadius: 6), with: .color(BandEarColors.faderThumb))
        }
    }
}

/// Keeps a panel slider's gesture across updates without redrawing on every touch.
private final class PanelSliderInput {
    var gesture: HorizontalFaderGesture

    init(scale: FaderScale) {
        gesture = HorizontalFaderGesture(thumbWidth: Double(PanelSlider.thumb.width), scale: scale)
    }
}
