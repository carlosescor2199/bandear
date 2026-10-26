import BandEarScreens
import SwiftUI

/// The BandEar palette, value for value the Android client's `BandEarDarkColors`
/// (`ui/theme/Color.kt:92-125`). Dark only: the app is used on dim stages, and a bright screen would
/// dazzle the musician and the audience.
///
/// The signal colours a state can take come from ``ConnectionTone``, the port of Android's
/// `ColorToken`: ``solid(_:)``, ``tint(_:)`` and ``tintBorder(_:)`` are its `solid`, `tint` and
/// `tintBorder`. Groups' colours come from ``GroupTone``.
enum BandEarColors {
    /// The page itself.
    static let ground = Color(hex: 0x0D1117)
    /// The bottom bar and the rail, a shade darker than the page.
    static let bar = Color(hex: 0x0B0F14)
    /// Cards, channel strips and panels.
    static let surface = Color(hex: 0x161B22)
    /// Buttons and controls that sit on a surface.
    static let surfaceRaised = Color(hex: 0x1F2630)
    /// The musician's own channel strip.
    static let surfaceSelected = Color(hex: 0x1A1F27)
    static let border = Color(hex: 0x2B3440)
    static let text = Color(hex: 0xE6EDF3)
    static let textSecondary = Color(hex: 0x9BA8B5)
    /// Text on an accent-filled control.
    static let onAccent = Color(hex: 0x0D1117)
    static let accent = Color(hex: 0xF5A524)
    static let green = Color(hex: 0x3FB950)
    static let red = Color(hex: 0xF85149)
    static let blue = Color(hex: 0x4CC2FF)
    /// A dot that has nothing to report yet.
    static let neutral = Color(hex: 0x6E7B88)
    static let faderTrack = Color(hex: 0x262F3A)
    /// The fill of a fader that is not the musician's own channel.
    static let faderIdle = Color(hex: 0x5B6B7C)
    /// The fill of a muted fader.
    static let faderMuted = Color(hex: 0x3A4452)
    static let faderThumb = Color(hex: 0xE6EDF3)

    static let greenTint = Color(hex: 0x10261A)
    static let greenTintBorder = Color(hex: 0x1F6F3A)
    static let amberTint = Color(hex: 0x2A1F0B)
    static let amberTintBorder = Color(hex: 0x6B4A12)
    static let redTint = Color(hex: 0x2D1215)
    static let redTintBorder = Color(hex: 0x6E2B2A)
    static let blueTint = Color(hex: 0x0E2233)
    static let blueTintBorder = Color(hex: 0x1D4A6B)

    /// The full-strength colour of `tone`, for dots and fills (`solid`, `Color.kt:60-67`).
    static func solid(_ tone: ConnectionTone) -> Color {
        switch tone {
        case .green: green
        case .amber: accent
        case .red: red
        case .blue: blue
        case .neutral: neutral
        }
    }

    /// A dark background tinted with `tone`, for pills and banners (`tint`, `Color.kt:69-76`).
    static func tint(_ tone: ConnectionTone) -> Color {
        switch tone {
        case .green: greenTint
        case .amber: amberTint
        case .red: redTint
        case .blue: blueTint
        case .neutral: surface
        }
    }

    /// The border that goes with ``tint(_:)`` (`tintBorder`, `Color.kt:78-85`).
    static func tintBorder(_ tone: ConnectionTone) -> Color {
        switch tone {
        case .green: greenTintBorder
        case .amber: amberTintBorder
        case .red: redTintBorder
        case .blue: blueTintBorder
        case .neutral: border
        }
    }

    /// The background of a group's chip or scribble strip (`groupBackground`, `Color.kt:87`).
    static func groupBackground(_ tone: GroupTone) -> Color {
        switch tone {
        case .teal: Color(hex: 0x123A3A)
        case .violet: Color(hex: 0x2A2346)
        case .orange: Color(hex: 0x3A2412)
        case .blue: Color(hex: 0x1C2A44)
        }
    }

    /// The text on a group's chip or scribble strip (`groupText`, `Color.kt:89`).
    static func groupText(_ tone: GroupTone) -> Color {
        switch tone {
        case .teal: Color(hex: 0x7FE0D0)
        case .violet: Color(hex: 0xC3B5FF)
        case .orange: Color(hex: 0xFFC58A)
        case .blue: Color(hex: 0x9CC3FF)
        }
    }

    /// A strip's level fill for its role: the accent while More of me lifts the musician's own
    /// strip, the idle grey otherwise, and the muted grey when muted.
    static func faderFill(_ fill: StripFill) -> Color {
        switch fill {
        case .accent: accent
        case .idle: faderIdle
        case .muted: faderMuted
        }
    }
}

extension Color {
    /// An opaque sRGB colour written as Android writes it, `0xRRGGBB` (Android's `0xFFRRGGBB` with the
    /// alpha byte left out).
    init(hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255,
            opacity: 1)
    }
}
