import CoreText
import SwiftUI
import UIKit

/// The two faces of the BandEar look: one for text, one for numbers (levels, pans, channel labels,
/// addresses), so digits keep their width while a level changes.
///
/// Both are bundled with the app (`Fonts/`, listed under `UIAppFonts`) under the SIL Open Font
/// License 1.1, whose texts ship beside them, as the Android client bundles them
/// (`ui/theme/Type.kt:14-31`): IBM Plex Sans for text and JetBrains Mono for numbers. Each is one
/// variable font file; a style's weight is set on the file's weight axis (`wght`), so it gets the
/// font's own design for that weight rather than the nearest named instance or a synthetic bold.
/// JetBrains Mono has no named SemiBold, so the axis is the only way to its 600.
enum BandEarFace: Hashable, Sendable {
    case text
    case numbers

    /// The PostScript name of the file's default instance, as the files themselves name it.
    var postScriptName: String {
        switch self {
        case .text: "IBMPlexSans-Regular"
        case .numbers: "JetBrainsMono-Regular"
        }
    }
}

/// A weight on a face's `wght` axis.
enum BandEarWeight: CGFloat, Hashable, Sendable {
    case regular = 400
    case medium = 500
    case semibold = 600

    /// The system weight closest to this one, for the fallback when a bundled font is missing.
    var system: UIFont.Weight {
        switch self {
        case .regular: .regular
        case .medium: .medium
        case .semibold: .semibold
        }
    }
}

/// One text style of the BandEar look: its face, size, weight, line height and letter spacing, and
/// the system text style whose Dynamic Type curve it grows along.
struct BandEarTextStyle: Hashable, Sendable {
    let face: BandEarFace
    /// The size at the default Dynamic Type setting, in points (Android's sp).
    let size: CGFloat
    let weight: BandEarWeight
    /// The distance between baselines at the default setting; `nil` keeps the font's own.
    var lineHeight: CGFloat?
    /// Extra space between letters, in ems (Android's `letterSpacing`).
    var tracking: CGFloat = 0
    let relativeTo: UIFont.TextStyle

    /// This style at another size, weight or face, keeping the rest (Android's `TextStyle.copy`).
    func with(face: BandEarFace? = nil, size: CGFloat? = nil, weight: BandEarWeight? = nil) -> Self {
        BandEarTextStyle(
            face: face ?? self.face, size: size ?? self.size, weight: weight ?? self.weight,
            lineHeight: lineHeight, tracking: tracking, relativeTo: relativeTo)
    }
}

/// The BandEar text styles: Material's type roles as the Android client sets them
/// (`BandEarTypography`, `ui/theme/Type.kt:77-132`), and its own number and caps styles
/// (`BandEarType`, `Type.kt:53-74`).
enum BandEarType {
    static let titleLarge = BandEarTextStyle(
        face: .text, size: 24, weight: .semibold, lineHeight: 30, relativeTo: .title2)
    static let titleMedium = BandEarTextStyle(
        face: .text, size: 18, weight: .semibold, lineHeight: 24, relativeTo: .title3)
    static let titleSmall = BandEarTextStyle(
        face: .text, size: 17, weight: .semibold, lineHeight: 22, relativeTo: .headline)
    static let bodyLarge = BandEarTextStyle(
        face: .text, size: 15, weight: .regular, lineHeight: 22, relativeTo: .subheadline)
    static let bodyMedium = BandEarTextStyle(
        face: .text, size: 14, weight: .regular, lineHeight: 20, relativeTo: .callout)
    static let bodySmall = BandEarTextStyle(
        face: .text, size: 13, weight: .regular, lineHeight: 18, relativeTo: .footnote)
    static let labelLarge = BandEarTextStyle(
        face: .text, size: 14, weight: .semibold, lineHeight: 20, relativeTo: .callout)
    static let labelMedium = BandEarTextStyle(
        face: .text, size: 12, weight: .medium, lineHeight: 16, relativeTo: .caption1)
    static let labelSmall = BandEarTextStyle(
        face: .text, size: 11, weight: .medium, lineHeight: 14, relativeTo: .caption2)

    /// Section labels such as "CHANNELS · 8" and "MASTER" (the caller upper-cases the words).
    static let caps = BandEarTextStyle(
        face: .text, size: 12, weight: .medium, tracking: 0.14, relativeTo: .caption1)

    /// Channel labels and levels on a strip.
    static let number = BandEarTextStyle(face: .numbers, size: 14, weight: .semibold, relativeTo: .callout)

    /// A number that heads a control, such as the channel on the "My channel" button.
    static let numberLarge = number.with(size: 17)

    /// Small numbers under a title, such as the server's address.
    static let numberSmall = number.with(size: 12, weight: .medium)
}

/// Builds the bundled fonts at a style's weight and at the reader's Dynamic Type size.
enum BandEarFonts {
    /// The `wght` axis tag, as CoreText names variation axes by their four-letter tag.
    private static let weightAxis = NSNumber(value: 0x7767_6874)

    private struct Key: Hashable {
        let style: BandEarTextStyle
        let category: UIContentSizeCategory
    }

    /// Fonts already built: a console redraws many strips while a fader moves.
    private static var cache: [Key: UIFont] = [:]

    /// `style` at the size the reader's Dynamic Type setting `category` asks for.
    ///
    /// When the bundled font cannot be found (it is not in the bundle, or not registered), the
    /// system font at the same size and weight stands in, monospaced for numbers, so text still
    /// shows and digits keep their width.
    static func uiFont(_ style: BandEarTextStyle, category: UIContentSizeCategory) -> UIFont {
        let key = Key(style: style, category: category)
        if let font = cache[key] { return font }
        let size = scaled(style.size, style, category)
        let font: UIFont
        if let base = UIFont(name: style.face.postScriptName, size: size) {
            let variation = UIFontDescriptor.AttributeName(rawValue: kCTFontVariationAttribute as String)
            let descriptor = base.fontDescriptor.addingAttributes([
                variation: [weightAxis: NSNumber(value: Double(style.weight.rawValue))]
            ])
            font = UIFont(descriptor: descriptor, size: size)
        } else {
            font =
                style.face == .numbers
                ? .monospacedSystemFont(ofSize: size, weight: style.weight.system)
                : .systemFont(ofSize: size, weight: style.weight.system)
        }
        cache[key] = font
        return font
    }

    /// `value`, a size or a line height at the default setting, as `category` scales `style`'s
    /// text style.
    static func scaled(_ value: CGFloat, _ style: BandEarTextStyle, _ category: UIContentSizeCategory)
        -> CGFloat
    {
        UIFontMetrics(forTextStyle: style.relativeTo).scaledValue(
            for: value, compatibleWith: UITraitCollection(preferredContentSizeCategory: category))
    }

    /// `style` as a SwiftUI font at the Dynamic Type size `dynamicTypeSize`.
    static func font(_ style: BandEarTextStyle, dynamicTypeSize: DynamicTypeSize) -> Font {
        Font(uiFont(style, category: UIContentSizeCategory(dynamicTypeSize)) as CTFont)
    }
}

/// Sets a ``BandEarTextStyle`` on the text inside, following the reader's Dynamic Type setting as
/// it changes, with the style's letter spacing and line height.
private struct BandEarFontModifier: ViewModifier {
    let style: BandEarTextStyle
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize

    func body(content: Content) -> some View {
        let category = UIContentSizeCategory(dynamicTypeSize)
        let font = BandEarFonts.uiFont(style, category: category)
        let lineSpacing = style.lineHeight.map {
            max(0, BandEarFonts.scaled($0, style, category) - font.lineHeight)
        }
        content
            .font(Font(font as CTFont))
            .tracking(style.tracking * font.pointSize)
            .lineSpacing(lineSpacing ?? 0)
    }
}

extension View {
    /// Draws the text inside in `style`, scaled with Dynamic Type.
    func bandEarFont(_ style: BandEarTextStyle) -> some View {
        modifier(BandEarFontModifier(style: style))
    }
}
