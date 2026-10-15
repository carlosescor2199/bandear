package com.mixlink.android.ui.theme

import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontListFontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.font.ResourceFont
import com.mixlink.android.R
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MixLinkFontsTest {
    @Test
    fun aVariableFamilySetsEachFontsWeightAxisToItsWeight() {
        val family = variableFontFamily(R.font.jetbrains_mono_variable, listOf(FontWeight.Light, FontWeight.Bold))

        assertEquals(listOf(300 to 300f, 700 to 700f), fontsOf(family).map { it.weight.weight to wghtOf(it) })
        assertTrue(fontsOf(family).all { it.resId == R.font.jetbrains_mono_variable })
    }

    @Test
    fun theTextFaceIsIbmPlexSansInRegularMediumAndSemibold() {
        val fonts = fontsOf(MixLinkFonts.Text)

        assertEquals(listOf(400, 500, 600), fonts.map { it.weight.weight })
        assertEquals(listOf(400f, 500f, 600f), fonts.map(::wghtOf))
        assertTrue(fonts.all { it.resId == R.font.ibm_plex_sans_variable })
    }

    @Test
    fun theNumberFaceIsJetBrainsMonoInMediumAndSemibold() {
        val fonts = fontsOf(MixLinkFonts.Numbers)

        assertEquals(listOf(500, 600), fonts.map { it.weight.weight })
        assertEquals(listOf(500f, 600f), fonts.map(::wghtOf))
        assertTrue(fonts.all { it.resId == R.font.jetbrains_mono_variable })
    }

    @Test
    fun everyStyleAsksForAWeightItsFaceBundles() {
        // A weight the face does not bundle would be drawn from the nearest one, or made bolder by
        // the renderer, instead of the font's own design.
        val bundled = mapOf(
            MixLinkFonts.Text to MixLinkFonts.TextWeights,
            MixLinkFonts.Numbers to MixLinkFonts.NumberWeights,
        )
        val styles: List<TextStyle> = listOf(
            MixLinkType.Caps,
            MixLinkType.Number,
            MixLinkType.NumberLarge,
            MixLinkType.NumberSmall,
            MixLinkTypography.titleLarge,
            MixLinkTypography.titleMedium,
            MixLinkTypography.titleSmall,
            MixLinkTypography.bodyLarge,
            MixLinkTypography.bodyMedium,
            MixLinkTypography.bodySmall,
            MixLinkTypography.labelLarge,
            MixLinkTypography.labelMedium,
            MixLinkTypography.labelSmall,
        )

        for (style in styles) {
            val weights = bundled[style.fontFamily]
            assertTrue("$style uses a MixLink face", weights != null)
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
