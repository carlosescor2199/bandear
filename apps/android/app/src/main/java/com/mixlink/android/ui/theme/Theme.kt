package com.mixlink.android.ui.theme

import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.ReadOnlyComposable

/**
 * The MixLink look: the dark palette, the type and the shapes, for every screen and atom.
 *
 * Material components inside it pick up the same colours through the Material colour scheme;
 * MixLink's own atoms read [MixLinkTheme.colors] for the tokens Material has no role for.
 */
@Composable
fun MixLinkTheme(content: @Composable () -> Unit) {
    val colors = MixLinkDarkColors
    val scheme = darkColorScheme(
        primary = colors.accent,
        onPrimary = colors.onAccent,
        secondary = colors.blue,
        onSecondary = colors.onAccent,
        tertiary = colors.green,
        onTertiary = colors.onAccent,
        background = colors.ground,
        onBackground = colors.text,
        surface = colors.surface,
        onSurface = colors.text,
        surfaceVariant = colors.surfaceRaised,
        onSurfaceVariant = colors.textSecondary,
        outline = colors.border,
        outlineVariant = colors.surfaceRaised,
        error = colors.red,
        onError = colors.onAccent,
    )
    MaterialTheme(
        colorScheme = scheme,
        typography = MixLinkTypography,
        shapes = MixLinkMaterialShapes,
    ) {
        CompositionLocalProvider(
            LocalMixLinkColors provides colors,
            LocalContentColor provides colors.text,
            content = content,
        )
    }
}

/** Reads the MixLink look inside [MixLinkTheme]. */
object MixLinkTheme {
    val colors: MixLinkColors
        @Composable
        @ReadOnlyComposable
        get() = LocalMixLinkColors.current
}
