import BandEarScreens
import SwiftUI

/// A console fader from 0 at the bottom to 100 at the top (Android's `VerticalFader`), drawn as the
/// design's console strip draws it: tick marks on both sides at every quarter, a 4 pt track, a fill
/// that rises to the middle of the cap, and a light cap with a dark line across its middle.
///
/// A drag moves the cap with the finger from wherever it is, so touching the strip never makes the
/// level jump; a tap moves the cap to the tapped point. A sideways swipe is left to the parent, so a
/// row of faders still scrolls sideways (see ``FaderTouchSurface``). For VoiceOver it is one
/// adjustable element: it says `voiceOverLabel` and `voiceOverValue`, and a swipe up or down moves
/// the level by ``FaderMath/screenReaderStep``.
///
/// Stateless: it shows `value` and reports a new level through `onChange`, only when the level
/// actually changes, as `0...100`. A fader laid out no taller than its cap ignores input rather
/// than report silence (see ``FaderGesture``). `fill` is the caller's (``BandEarColors/faderFill(_:)``
/// for a channel strip); `muted` dims the cap, as the design draws a muted strip. A `limit` draws the
/// engineer's limit as an accent line across the track, with the number beside it.
///
/// It takes the width and the height its container gives it; the ticks sit 4 pt in from its sides
/// and the cap is 36 pt wide (narrower if the fader is). Inside a disabled view (`.disabled(true)`,
/// as a held strip is) it fades to 40% and takes no input, so do not fade it again.
struct VerticalFader: View {
    let value: Int
    let fill: Color
    var muted = false
    var limit: Int?
    let voiceOverLabel: String
    let voiceOverValue: String
    let onChange: @MainActor (Int) -> Void

    @Environment(\.isEnabled) private var isEnabled
    @State private var input = FaderInput()

    static let capWidth: CGFloat = 36
    static let capHeight: CGFloat = 14

    var body: some View {
        let limitLabel = limit.map {
            Text(verbatim: "\($0)").font(Self.limitFont).foregroundColor(BandEarColors.accent)
        }
        Canvas { context, size in
            draw(in: &context, size: size, limitLabel: limitLabel)
        }
        .overlay(
            FaderTouchSurface(
                axis: .vertical, isEnabled: isEnabled,
                onLength: { input.gesture.length = $0 },
                onTap: { report(input.gesture.tap(at: $0, current: value)) },
                onDragStart: { input.gesture.startDrag(current: value) },
                onDrag: { report(input.gesture.drag(by: $0, current: value)) })
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

    /// The limit's number: small mono digits in the accent, at a fixed size, since the master panel
    /// says the limit in words and VoiceOver never reads the number.
    private static let limitFont = Font(
        BandEarFonts.uiFont(BandEarType.numberSmall.with(size: 9), category: .large) as CTFont)

    private func report(_ next: Int?) {
        if let next { onChange(next) }
    }

    private func draw(in context: inout GraphicsContext, size: CGSize, limitLabel: Text?) {
        let cap = CGSize(width: min(Self.capWidth, size.width), height: Self.capHeight)
        // Everything is placed from where the cap's centre sits at a level (FaderMath.centre), the
        // place a tap reads back as that level, so the drawing and a touch agree.
        let length = Double(size.height)
        let capHeight = Double(cap.height)
        let midX = size.width / 2

        for y in FaderMath.tickCentres(length: length, cap: capHeight) {
            for x in [4, size.width - 12] {
                let tick = CGRect(x: x, y: CGFloat(y) - 0.5, width: 8, height: 1)
                context.fill(Path(tick), with: .color(BandEarColors.border))
            }
        }

        let trackTop = CGFloat(FaderMath.centre(of: FaderMath.max, length: length, cap: capHeight))
        let trackBottom = CGFloat(FaderMath.centre(of: 0, length: length, cap: capHeight))
        let track = CGRect(x: midX - 2, y: trackTop, width: 4, height: max(trackBottom - trackTop, 0))
        context.fill(Path(roundedRect: track, cornerRadius: 2), with: .color(BandEarColors.faderTrack))
        let capMiddle = CGFloat(FaderMath.centre(of: value, length: length, cap: capHeight))
        let filled = CGRect(
            x: track.minX, y: capMiddle, width: track.width, height: max(track.maxY - capMiddle, 0))
        context.fill(Path(roundedRect: filled, cornerRadius: 2), with: .color(fill))

        if let limitCentre = FaderMath.limitCentre(limit, length: length, cap: capHeight) {
            let y = CGFloat(limitCentre)
            let width = min(52, size.width)
            let line = CGRect(x: midX - width / 2, y: y - 1, width: width, height: 2)
            context.fill(Path(roundedRect: line, cornerRadius: 1), with: .color(BandEarColors.accent))
            if let limitLabel {
                context.draw(limitLabel, at: CGPoint(x: size.width, y: y - 2), anchor: .bottomTrailing)
            }
        }

        let capTop = capMiddle - cap.height / 2
        let capRect = CGRect(origin: CGPoint(x: midX - cap.width / 2, y: capTop), size: cap)
        context.drawLayer { layer in
            layer.opacity = muted ? 0.6 : 1
            layer.drawLayer { shadowed in
                shadowed.addFilter(.shadow(color: .black.opacity(0.6), radius: 1.5, x: 0, y: 1))
                shadowed.fill(
                    Path(roundedRect: capRect, cornerRadius: 3), with: .color(BandEarColors.faderThumb))
            }
            let line = CGRect(x: capRect.minX, y: capRect.minY + 6, width: capRect.width, height: 2)
            layer.fill(Path(line), with: .color(BandEarColors.ground))
        }
    }
}

/// Keeps a fader's gesture across updates without redrawing on every touch, as Android keeps it out
/// of composition with `remember`.
private final class FaderInput {
    var gesture = FaderGesture(thumbLength: Double(VerticalFader.capHeight))
}
