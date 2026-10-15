package com.mixlink.android.ui.theme

import androidx.compose.material3.Typography
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontVariation
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.mixlink.android.R

/**
 * The two faces of the MixLink look: one for text, one for numbers (levels, channel labels,
 * addresses), so digits keep their width while a level changes.
 *
 * This is the single seam for fonts. Both are bundled with the app (`res/font`), under the SIL
 * Open Font License 1.1, whose texts ship in `assets/licenses`: IBM Plex Sans for text and
 * JetBrains Mono for numbers. Each is one variable font file, set to the weights the styles use.
 */
object MixLinkFonts {
    /** The weights the text styles use: regular, medium and semibold. */
    val TextWeights: List<FontWeight> = listOf(FontWeight.Normal, FontWeight.Medium, FontWeight.SemiBold)

    /** The weights the number styles use: medium and semibold. */
    val NumberWeights: List<FontWeight> = listOf(FontWeight.Medium, FontWeight.SemiBold)

    val Text: FontFamily = variableFontFamily(R.font.ibm_plex_sans_variable, TextWeights)
    val Numbers: FontFamily = variableFontFamily(R.font.jetbrains_mono_variable, NumberWeights)
}

/**
 * A family drawn from one variable font file ([resId]): one font per weight in [weights], each set
 * to that weight on the file's weight axis, so a style asking for a weight gets the font's own
 * design for it rather than the nearest one or a bolder rendering.
 *
 * The overload of [Font] that takes the variation settings is still marked experimental in
 * Compose 1.7; it is the one that names the axis instead of leaving it to a library default.
 */
@OptIn(ExperimentalTextApi::class)
fun variableFontFamily(resId: Int, weights: List<FontWeight>): FontFamily = FontFamily(
    weights.map { weight ->
        Font(
            resId = resId,
            weight = weight,
            variationSettings = FontVariation.Settings(FontVariation.weight(weight.weight)),
        )
    },
)

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
