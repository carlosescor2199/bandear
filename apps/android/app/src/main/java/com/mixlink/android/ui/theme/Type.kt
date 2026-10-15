package com.mixlink.android.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/**
 * The two faces of the MixLink look: one for text, one for numbers (levels, channel labels,
 * addresses), so digits keep their width while a level changes.
 *
 * This is the single seam for fonts. Until the bundled IBM Plex Sans and JetBrains Mono arrive
 * (task T5) the system faces stand in; T5 only replaces these two values.
 */
object MixLinkFonts {
    val Text: FontFamily = FontFamily.SansSerif
    val Numbers: FontFamily = FontFamily.Monospace
}

/** Text styles outside Material's roles: numbers and the small capitals of section labels. */
object MixLinkType {
    /** Section labels such as "CHANNELS · 8" and "MASTER". */
    val Caps = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        letterSpacing = 0.14.em,
    )

    /** Channel labels and levels on a strip. */
    val Number = TextStyle(
        fontFamily = MixLinkFonts.Numbers,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
    )

    /** A number that heads a control, such as the channel on the "My channel" button. */
    val NumberLarge = Number.copy(fontSize = 17.sp)

    /** Small numbers under a title, such as the server's address. */
    val NumberSmall = Number.copy(fontWeight = FontWeight.Medium, fontSize = 12.sp)
}

/** Material's type roles in the MixLink text face and sizes. */
val MixLinkTypography = Typography(
    titleLarge = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.SemiBold,
        fontSize = 24.sp,
        lineHeight = 30.sp,
    ),
    titleMedium = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.SemiBold,
        fontSize = 18.sp,
        lineHeight = 24.sp,
    ),
    titleSmall = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.SemiBold,
        fontSize = 17.sp,
        lineHeight = 22.sp,
    ),
    bodyLarge = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Normal,
        fontSize = 15.sp,
        lineHeight = 22.sp,
    ),
    bodyMedium = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Normal,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    bodySmall = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Normal,
        fontSize = 13.sp,
        lineHeight = 18.sp,
    ),
    labelLarge = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.SemiBold,
        fontSize = 14.sp,
        lineHeight = 20.sp,
    ),
    labelMedium = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        lineHeight = 16.sp,
    ),
    labelSmall = TextStyle(
        fontFamily = MixLinkFonts.Text,
        fontWeight = FontWeight.Medium,
        fontSize = 11.sp,
        lineHeight = 14.sp,
    ),
)
