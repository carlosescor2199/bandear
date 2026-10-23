/// How a pan is written, and how the pan control's centre detent turns a position into a pan.
///
/// Ported from the Android client's `PanScale.kt`: its words and the pure arithmetic of its detent.
/// Android's own fader gesture is not ported. The native `Slider` takes the touches, and binds its
/// position to a pan through ``value(at:travel:detentShare:)`` and
/// ``position(of:travel:detentShare:)`` over a travel of its own, `0...1` for example, so a pan near
/// the centre snaps to it there too.
public enum PanScale {
    /// Hard left: the channel plays only in the left ear.
    public static let left = 0
    /// The centre: the channel plays equally in both ears.
    public static let centre = 50
    /// Hard right: the channel plays only in the right ear.
    public static let right = 100

    /// How far the centre detent reaches on each side of the middle, as a share of the travel: a
    /// touch anywhere in the middle tenth of the control reads as the centre, so the centre is easy
    /// to find by feel and hard to miss by a hair (`PAN_DETENT_SHARE`, `PanScale.kt:15-20`).
    public static let detentShare = 0.05

    /// The pan at `fromLeft` along a travel `travel` long (`panValueAt`, `PanScale.kt:22-40`).
    ///
    /// The middle of the travel, `detentShare` of it on each side, is the centre; the rest maps
    /// evenly onto the values on either side, so every pan can still be reached and the ends are hard
    /// left and hard right. A point past either end reads as that end, and a travel of nothing reads
    /// as the centre.
    ///
    /// Kotlin's `roundToInt` and Swift's `rounded()` differ only on negative halves, and every value
    /// rounded here is at least 0.
    public static func value(
        at fromLeft: Double, travel: Double, detentShare: Double = PanScale.detentShare
    ) -> Int {
        let side = sideTravel(travel, detentShare)
        guard side > 0 else { return centre }
        let position = min(max(fromLeft, 0), travel)
        let rightSideStart = travel - side
        let pan: Int
        if position <= side {
            pan = Int((position / side * Double(centre)).rounded())
        } else if position >= rightSideStart {
            pan = centre + Int(((position - rightSideStart) / side * Double(centre)).rounded())
        } else {
            pan = centre
        }
        return min(max(pan, left), right)
    }

    /// Where `pan` sits along a travel `travel` long: the inverse of
    /// ``value(at:travel:detentShare:)``. The centre sits exactly in the middle, a pan next to it at
    /// the detent's edge, and a pan outside ``left``...``right`` at the nearest end
    /// (`panPositionOf`, `PanScale.kt:42-55`).
    public static func position(
        of pan: Int, travel: Double, detentShare: Double = PanScale.detentShare
    ) -> Double {
        let value = min(max(pan, left), right)
        let side = sideTravel(travel, detentShare)
        if value < centre { return Double(value) / Double(centre) * side }
        if value > centre { return travel - side + Double(value - centre) / Double(centre) * side }
        return travel / 2
    }

    /// A pan as the strips and the channel sheet write it: "C" at the centre, otherwise the side and
    /// how far toward it, from "L2" next to the centre to "L100" hard left, and "R2" to "R100"
    /// (`panText`, `PanScale.kt:61-72`).
    public static func text(_ pan: Int) -> String {
        let amount = amount(pan)
        if amount == 0 { return "C" }
        return pan < centre ? "L\(amount)" : "R\(amount)"
    }

    /// What a screen reader says for a pan: "Centre", "Left 30", "Right 100"
    /// (`panSpokenText`, `PanScale.kt:74-82`).
    public static func spokenText(_ pan: Int) -> String {
        let amount = amount(pan)
        if amount == 0 { return "Centre" }
        return pan < centre ? "Left \(amount)" : "Right \(amount)"
    }

    /// How far `pan` is from the centre, in percent of the way to its side: 0 at the centre, 100 at
    /// an end (`panAmount`, `PanScale.kt:84-86`).
    private static func amount(_ pan: Int) -> Int {
        abs(min(max(pan, left), right) - centre) * 100 / (right - centre)
    }

    /// How much of the travel each side outside the detent spans (`sideTravel`, `PanScale.kt:57-59`).
    private static func sideTravel(_ travel: Double, _ detentShare: Double) -> Double {
        travel * (0.5 - min(max(detentShare, 0), 0.5))
    }
}
