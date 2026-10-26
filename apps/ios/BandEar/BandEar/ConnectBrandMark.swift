import SwiftUI

/// The BandEar app icon drawn at `size`, for the brand header of Connect: the mark's strokes in the
/// icon's amber on the icon's dark tile, as the "Connect" boards draw it. Decoration only: the
/// header's words name the app.
///
/// Its two colours are the launcher icon's (Android's `bandear_icon_ground` and `bandear_mark`,
/// `res/values/colors.xml`), not the screens' palette.
struct ConnectBrandMark: View {
    var size: CGFloat = 44

    private static let ground = Color(hex: 0x192223)
    private static let mark = Color(hex: 0xFFBF43)

    var body: some View {
        // The icon's grid is 1024 units; the mark's strokes are 46 of them, scaled by 1.16.
        let unit = size / 1024
        ZStack {
            BandEarShapes.rounded(228 * unit)
                .fill(Self.ground)
            BrandMarkShape()
                .stroke(
                    Self.mark,
                    style: StrokeStyle(lineWidth: 46 * 1.16 * unit, lineCap: .round, lineJoin: .round))
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

/// The mark's six strokes (the launcher icon's paths, `drawable/bandear_mark.xml`) placed in a
/// square as the icon places them: on the 1024-unit grid, scaled by 1.16 about its centre.
private struct BrandMarkShape: Shape {
    nonisolated func path(in rect: CGRect) -> Path {
        var mark = Path()
        mark.move(to: CGPoint(x: 520, y: 224))
        mark.addLine(to: CGPoint(x: 344, y: 224))
        mark.addSVGArc(to: CGPoint(x: 296, y: 272), radius: 48, sweep: false)
        mark.addLine(to: CGPoint(x: 296, y: 738))
        mark.addSVGArc(to: CGPoint(x: 366, y: 808), radius: 70, sweep: false)
        mark.addLine(to: CGPoint(x: 556, y: 808))
        mark.addSVGArc(to: CGPoint(x: 692, y: 528), radius: 186, sweep: false)

        mark.move(to: CGPoint(x: 736, y: 446))
        mark.addQuadCurve(to: CGPoint(x: 678, y: 514), control: CGPoint(x: 720, y: 486))

        mark.move(to: CGPoint(x: 314, y: 522))
        mark.addCurve(
            to: CGPoint(x: 476, y: 470), control1: CGPoint(x: 354, y: 482),
            control2: CGPoint(x: 404, y: 470))
        mark.addLine(to: CGPoint(x: 606, y: 470))

        mark.move(to: CGPoint(x: 402, y: 708))
        mark.addLine(to: CGPoint(x: 562, y: 708))
        mark.addSVGArc(to: CGPoint(x: 562, y: 566), radius: 71, sweep: false)
        mark.addLine(to: CGPoint(x: 474, y: 566))
        mark.addSVGArc(to: CGPoint(x: 402, y: 638), radius: 72, sweep: false)

        mark.move(to: CGPoint(x: 586, y: 302))
        mark.addSVGArc(to: CGPoint(x: 662, y: 386), radius: 92, sweep: true)

        mark.move(to: CGPoint(x: 584, y: 224))
        mark.addSVGArc(to: CGPoint(x: 738, y: 386), radius: 162, sweep: true)

        let unit = min(rect.width, rect.height) / 1024
        let placement = CGAffineTransform(translationX: -517, y: -516)
            .concatenating(CGAffineTransform(scaleX: 1.16, y: 1.16))
            .concatenating(CGAffineTransform(translationX: 512, y: 512))
            .concatenating(CGAffineTransform(scaleX: unit, y: unit))
            .concatenating(CGAffineTransform(translationX: rect.minX, y: rect.minY))
        return mark.applying(placement)
    }
}

extension Path {
    /// An SVG elliptical arc of a circle (`A r r 0 0 sweep x y`), the smaller of the two that join
    /// the current point to `end`: `sweep` true turns the way the angle grows, clockwise on a
    /// screen whose y runs down. A radius too short to join them is lengthened, as SVG does.
    nonisolated fileprivate mutating func addSVGArc(to end: CGPoint, radius: CGFloat, sweep: Bool) {
        guard let start = currentPoint else { return }
        // The centre, from the SVG specification's endpoint-to-centre conversion (F.6.5).
        let halfX = (start.x - end.x) / 2
        let halfY = (start.y - end.y) / 2
        let halfChordSquared = halfX * halfX + halfY * halfY
        guard halfChordSquared > 0 else { return }
        let r = max(radius, halfChordSquared.squareRoot())
        let factor = (max(0, r * r - halfChordSquared) / halfChordSquared).squareRoot() * (sweep ? 1 : -1)
        let center = CGPoint(
            x: (start.x + end.x) / 2 + factor * halfY, y: (start.y + end.y) / 2 - factor * halfX)
        addArc(
            center: center, radius: r,
            startAngle: .radians(atan2(start.y - center.y, start.x - center.x)),
            endAngle: .radians(atan2(end.y - center.y, end.x - center.x)),
            // `clockwise` reads as on a screen whose y runs up.
            clockwise: !sweep)
    }
}
