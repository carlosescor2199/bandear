import BandEarScreens
import SwiftUI

/// The small pan at the top of a channel strip: a line with a mark at the centre, a dot where the
/// pan is, and the pan in mono under it ("C", "L20", "R100", ``PanScale/text(_:)``).
///
/// It behaves like Android's `PanControl` in little: a sideways drag moves the dot from wherever it
/// is, a tap moves it to the tapped point, and the centre holds a detent so it is easy to land on
/// (``FaderScale/pan``). A drag up or down is not the pan's (see ``FaderTouchSurface``). For
/// VoiceOver it is one adjustable element: it says `voiceOverLabel` and `voiceOverValue`, the
/// strip's existing pan wording, and a swipe up or down moves the pan ``FaderMath/screenReaderStep``
/// right or left, ignoring the detent.
///
/// Stateless: it shows `pan` and reports a new pan through `onChange`, only when it changes, as
/// `0...100` with the centre at 50. The dot is the accent on the musician's own strip (`mine`).
/// It is 26 pt tall and as wide as its container; inside a disabled view it fades to 40% and takes
/// no input.
struct StripPan: View {
    let pan: Int
    var mine = false
    let voiceOverLabel: String
    let voiceOverValue: String
    let onChange: @MainActor (Int) -> Void

    @Environment(\.isEnabled) private var isEnabled
    @State private var input = PanInput()

    /// The dot's diameter; its centre runs from `inset` in from one end to `inset` in from the
    /// other, as the design draws it, so the gesture's thumb is twice the inset.
    private static let dot: CGFloat = 10
    fileprivate static let inset: CGFloat = 4

    var body: some View {
        VStack(spacing: 0) {
            Canvas { context, size in
                draw(in: &context, size: size)
            }
            .frame(height: 14)
            Text(PanScale.text(pan))
                .bandEarFont(BandEarType.numberSmall.with(size: 10))
                .foregroundStyle(BandEarColors.textSecondary)
                .lineLimit(1)
                .minimumScaleFactor(0.6)
                .frame(minHeight: 12)
        }
        .frame(maxWidth: .infinity, minHeight: 26)
        .overlay(
            FaderTouchSurface(
                axis: .horizontal, isEnabled: isEnabled,
                onLength: { input.gesture.width = $0 },
                onTap: { report(input.gesture.tap(atX: $0, current: pan)) },
                onDragStart: { input.gesture.startDrag(current: pan) },
                onDrag: { report(input.gesture.drag(byX: $0, current: pan)) })
        )
        .opacity(isEnabled ? 1 : 0.4)
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(voiceOverLabel)
        .accessibilityValue(voiceOverValue)
        .accessibilityAdjustableAction { direction in
            guard isEnabled else { return }
            switch direction {
            case .increment: report(input.gesture.increment(current: pan))
            case .decrement: report(input.gesture.decrement(current: pan))
            @unknown default: break
            }
        }
    }

    private func report(_ next: Int?) {
        if let next { onChange(next) }
    }

    private func draw(in context: inout GraphicsContext, size: CGSize) {
        let inset = Self.inset
        let travel = max(size.width - 2 * inset, 0)
        let line = CGRect(x: inset, y: 6, width: travel, height: 2)
        context.fill(Path(roundedRect: line, cornerRadius: 1), with: .color(BandEarColors.faderTrack))
        let mark = CGRect(x: size.width / 2 - 0.5, y: 2, width: 1, height: 10)
        context.fill(Path(mark), with: .color(BandEarColors.neutral))
        // The gesture's thumb is twice the inset, so the dot is drawn where a tap reads its pan.
        let centreX = CGFloat(
            PanScale.thumbCentre(of: pan, length: Double(size.width), thumb: Double(2 * inset)))
        let dot = CGRect(x: centreX - Self.dot / 2, y: 2, width: Self.dot, height: Self.dot)
        context.fill(
            Path(ellipseIn: dot), with: .color(mine ? BandEarColors.accent : BandEarColors.textSecondary))
    }
}

/// Keeps the pan's gesture across updates without redrawing on every touch.
private final class PanInput {
    var gesture = HorizontalFaderGesture(thumbWidth: Double(2 * StripPan.inset), scale: .pan)
}
