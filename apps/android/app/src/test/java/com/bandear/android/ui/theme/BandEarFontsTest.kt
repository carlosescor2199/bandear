package com.bandear.android.ui.theme

import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontListFontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.font.ResourceFont
import com.bandear.android.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BandEarFontsTest {
    @Test
    fun aVariableFamilySetsEachFontsWeightAxisToItsWeight() {
        val family = variableFontFamily(R.font.jetbrains_mono_variable, listOf(FontWeight.Light, FontWeight.Bold))

        assertEquals(listOf(300 to 300f, 700 to 700f), fontsOf(family).map { it.weight.weight to wghtOf(it) })
        assertTrue(fontsOf(family).all { it.resId == R.font.jetbrains_mono_variable })
    }

    @Test
    fun theTextFaceIsIbmPlexSansInRegularMediumAndSemibold() {
        val fonts = fontsOf(BandEarFonts.Text)

        assertEquals(listOf(400, 500, 600), fonts.map { it.weight.weight })
        assertEquals(listOf(400f, 500f, 600f), fonts.map(::wghtOf))
        assertTrue(fonts.all { it.resId == R.font.ibm_plex_sans_variable })
    }

    @Test
    fun theNumberFaceIsJetBrainsMonoInMediumAndSemibold() {
        val fonts = fontsOf(BandEarFonts.Numbers)

        assertEquals(listOf(500, 600), fonts.map { it.weight.weight })
        assertEquals(listOf(500f, 600f), fonts.map(::wghtOf))
        assertTrue(fonts.all { it.resId == R.font.jetbrains_mono_variable })
    }

    @Test
    fun everyStyleAsksForAWeightItsFaceBundles() {
        // A weight the face does not bundle would be drawn from the nearest one, or made bolder by
        // the renderer, instead of the font's own design.
        val bundled = mapOf(
            BandEarFonts.Text to BandEarFonts.TextWeights,
            BandEarFonts.Numbers to BandEarFonts.NumberWeights,
        )
        val styles: List<TextStyle> = listOf(
            BandEarType.Caps,
            BandEarType.Number,
            BandEarType.NumberLarge,
            BandEarType.NumberSmall,
            BandEarTypography.titleLarge,
            BandEarTypography.titleMedium,
            BandEarTypography.titleSmall,
            BandEarTypography.bodyLarge,
            BandEarTypography.bodyMedium,
            BandEarTypography.bodySmall,
            BandEarTypography.labelLarge,
            BandEarTypography.labelMedium,
            BandEarTypography.labelSmall,
        )

        for (style in styles) {
            val weights = bundled[style.fontFamily]
            assertTrue("$style uses a BandEar face", weights != null)
            assertTrue("${style.fontWeight} is bundled", style.fontWeight in weights!!)
        }
    }

    private fun fontsOf(family: FontFamily): List<ResourceFont> {
        assertTrue("$family is drawn from bundled files", family is FontListFontFamily)
        return (family as FontListFontFamily).fonts.map { it as ResourceFont }
    }

    private fun wghtOf(font: ResourceFont): Float =
        font.variationSettings.settings.single { it.axisName == "wght" }.toVariationValue(null)
}
