package com.mixlink.android.ui.theme

import androidx.compose.runtime.Immutable
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Color

/**
 * The signal colours a state can take, named by colour rather than by meaning so that pure
 * mappings (connection state, link health) can be tested on the JVM without Compose and the theme
 * stays the only place that knows the actual colour values.
 */
enum class ColorToken { GREEN, AMBER, RED, BLUE, NEUTRAL }

/** The tones a group chip can take; groups get them in turn, so neighbours differ. */
enum class GroupTone { TEAL, VIOLET, ORANGE, BLUE }

/**
 * The MixLink palette. Dark only: the app is used on dim stages, and a bright screen would
 * dazzle the musician and the audience.
 */
@Immutable
class MixLinkColors(
    /** The page itself. */
    val ground: Color,
    /** The bottom bar, a shade darker than the page. */
    val bar: Color,
    /** Cards, channel strips and panels. */
    val surface: Color,
    /** Buttons and controls that sit on a surface. */
    val surfaceRaised: Color,
    /** The musician's own channel strip. */
    val surfaceSelected: Color,
    val border: Color,
    val text: Color,
    val textSecondary: Color,
    /** Text on an accent-filled control. */
    val onAccent: Color,
    val accent: Color,
    val green: Color,
    val red: Color,
    val blue: Color,
    /** A dot that has nothing to report yet. */
    val neutral: Color,
    val faderTrack: Color,
    /** The fill of a fader that is not the musician's own channel. */
    val faderIdle: Color,
    /** The fill of a muted fader. */
    val faderMuted: Color,
    val faderThumb: Color,
    private val greenTint: Color,
    private val greenTintBorder: Color,
    private val amberTint: Color,
    private val amberTintBorder: Color,
    private val redTint: Color,
    private val redTintBorder: Color,
    private val blueTint: Color,
    private val blueTintBorder: Color,
    private val groupTones: Map<GroupTone, Pair<Color, Color>>,
) {
    /** The full-strength colour of [token], for dots and fills. */
    fun solid(token: ColorToken): Color = when (token) {
        ColorToken.GREEN -> green
        ColorToken.AMBER -> accent
        ColorToken.RED -> red
        ColorToken.BLUE -> blue
        ColorToken.NEUTRAL -> neutral
    }

    /** A dark background tinted with [token], for pills and banners. */
    fun tint(token: ColorToken): Color = when (token) {
        ColorToken.GREEN -> greenTint
        ColorToken.AMBER -> amberTint
        ColorToken.RED -> redTint
        ColorToken.BLUE -> blueTint
        ColorToken.NEUTRAL -> surface
    }

    /** The border that goes with [tint]. */
    fun tintBorder(token: ColorToken): Color = when (token) {
        ColorToken.GREEN -> greenTintBorder
        ColorToken.AMBER -> amberTintBorder
        ColorToken.RED -> redTintBorder
        ColorToken.BLUE -> blueTintBorder
        ColorToken.NEUTRAL -> border
    }

    fun groupBackground(tone: GroupTone): Color = groupTones.getValue(tone).first

    fun groupText(tone: GroupTone): Color = groupTones.getValue(tone).second
}

val MixLinkDarkColors = MixLinkColors(
    ground = Color(0xFF0D1117),
    bar = Color(0xFF0B0F14),
    surface = Color(0xFF161B22),
    surfaceRaised = Color(0xFF1F2630),
    surfaceSelected = Color(0xFF1A1F27),
    border = Color(0xFF2B3440),
    text = Color(0xFFE6EDF3),
    textSecondary = Color(0xFF9BA8B5),
    onAccent = Color(0xFF0D1117),
    accent = Color(0xFFF5A524),
    green = Color(0xFF3FB950),
    red = Color(0xFFF85149),
    blue = Color(0xFF4CC2FF),
    neutral = Color(0xFF6E7B88),
    faderTrack = Color(0xFF262F3A),
    faderIdle = Color(0xFF5B6B7C),
    faderMuted = Color(0xFF3A4452),
    faderThumb = Color(0xFFE6EDF3),
    greenTint = Color(0xFF10261A),
    greenTintBorder = Color(0xFF1F6F3A),
    amberTint = Color(0xFF2A1F0B),
    amberTintBorder = Color(0xFF6B4A12),
    redTint = Color(0xFF2D1215),
    redTintBorder = Color(0xFF6E2B2A),
    blueTint = Color(0xFF0E2233),
    blueTintBorder = Color(0xFF1D4A6B),
    groupTones = mapOf(
        GroupTone.TEAL to (Color(0xFF123A3A) to Color(0xFF7FE0D0)),
        GroupTone.VIOLET to (Color(0xFF2A2346) to Color(0xFFC3B5FF)),
        GroupTone.ORANGE to (Color(0xFF3A2412) to Color(0xFFFFC58A)),
        GroupTone.BLUE to (Color(0xFF1C2A44) to Color(0xFF9CC3FF)),
    ),
)

/** The palette in use, provided by [MixLinkTheme]. */
val LocalMixLinkColors = staticCompositionLocalOf { MixLinkDarkColors }
