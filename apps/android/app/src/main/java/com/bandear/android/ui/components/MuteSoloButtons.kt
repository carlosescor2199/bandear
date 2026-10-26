package com.bandear.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme

/** Every key is a full 44 dp square, the smallest comfortable touch target. */
private val KeySize = 44.dp

/** A disabled key fades but keeps its colours, so it still shows whether it is on. */
private const val DisabledKeyAlpha = 0.4f

/** What a screen reader calls a channel's Mute key: "Mute CH 3". */
fun muteKeyDescription(channelLabel: String): String = "Mute $channelLabel"

/** What a screen reader calls a channel's Solo key: "Solo CH 3". */
fun soloKeyDescription(channelLabel: String): String = "Solo $channelLabel"

/**
 * The Mute and Solo keys of one channel strip. Each is a switch for a screen reader, named with
 * [channelLabel] ("Mute CH 3") so it is clear which channel it acts on; a key that is on is filled
 * (red for mute, amber for solo) rather than only tinted.
 */
@Composable
fun MuteSoloButtons(
    muted: Boolean,
    soloed: Boolean,
    onMute: (Boolean) -> Unit,
    onSolo: (Boolean) -> Unit,
    channelLabel: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    Row(modifier = modifier, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        ToggleKey(
            letter = "M",
            description = muteKeyDescription(channelLabel),
            on = muted,
            onColor = colors.red,
            onToggle = onMute,
            enabled = enabled,
        )
        ToggleKey(
            letter = "S",
            description = soloKeyDescription(channelLabel),
            on = soloed,
            onColor = colors.accent,
            onToggle = onSolo,
            enabled = enabled,
        )
    }
}

@Composable
private fun ToggleKey(
    letter: String,
    description: String,
    on: Boolean,
    onColor: Color,
    onToggle: (Boolean) -> Unit,
    enabled: Boolean,
) {
    val colors = BandEarTheme.colors
    Box(
        modifier = Modifier
            .size(KeySize)
            .alpha(if (enabled) 1f else DisabledKeyAlpha)
            .clip(BandEarShapes.Key)
            .background(if (on) onColor else colors.surfaceRaised)
            .border(1.dp, if (on) onColor else colors.border, BandEarShapes.Key)
            .toggleable(value = on, enabled = enabled, role = Role.Switch, onValueChange = onToggle)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = letter,
            style = MaterialTheme.typography.labelLarge,
            color = if (on) colors.onAccent else colors.text,
        )
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun MuteSoloButtonsPreview() {
    BandEarTheme {
        var muted by remember { mutableStateOf(true) }
        var soloed by remember { mutableStateOf(false) }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            MuteSoloButtons(muted, soloed, { muted = it }, { soloed = it }, "CH 4")
            MuteSoloButtons(muted = false, soloed = true, onMute = {}, onSolo = {}, channelLabel = "CH 5")
            MuteSoloButtons(false, false, {}, {}, "CH 6", enabled = false)
        }
    }
}
