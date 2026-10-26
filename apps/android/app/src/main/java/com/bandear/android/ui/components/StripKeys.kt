package com.bandear.android.ui.components

import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.toggleable
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
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/**
 * The height of the slot at the top of a console strip: the 28 dp keys ([StripKeys]) or the held
 * tag ([HeldTag]) drawn in the middle of a 48 dp touch target. A strip with neither (a group strip
 * that follows no one) can leave a spacer this tall, so its fader lines up with its neighbours'.
 */
val StripKeySlotHeight: Dp = 48.dp

/** How tall a strip key, and the held tag in its place, is drawn. */
internal val StripKeyHeight = 28.dp

/** A disabled key fades but keeps its colours, so it still shows whether it is on. */
private const val DisabledKeyAlpha = 0.4f

/**
 * The M and S keys at the top of a console strip, side by side and sharing the strip's width: the
 * compact form of [MuteSoloButtons], with the same colours and the same meaning for a screen
 * reader. Each is a switch named with [channelLabel] ("Mute CH 3", "Solo CH 3"); a key that is on
 * is filled (red for mute, amber for solo) rather than only tinted.
 *
 * Each key is drawn 28 dp tall in the middle of a [StripKeySlotHeight] (48 dp) touch target, so the
 * slot is the gap above and below the keys a strip has anyway. Each key takes half the slot's
 * width: 48 dp each from 100 dp inside the strip's padding. In a narrower strip Compose's minimum
 * touch target (48 dp, which pointer input reaches around a smaller target) makes up the width,
 * a touch between the two keys going to the nearer one.
 */
@Composable
fun StripKeys(
    muted: Boolean,
    soloed: Boolean,
    onMute: (Boolean) -> Unit,
    onSolo: (Boolean) -> Unit,
    channelLabel: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    Row(modifier = modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        StripKey(
            letter = "M",
            description = muteKeyDescription(channelLabel),
            on = muted,
            onColor = colors.red,
            onToggle = onMute,
            enabled = enabled,
            modifier = Modifier.weight(1f),
        )
        StripKey(
            letter = "S",
            description = soloKeyDescription(channelLabel),
            on = soloed,
            onColor = colors.accent,
            onToggle = onSolo,
            enabled = enabled,
            modifier = Modifier.weight(1f),
        )
    }
}

@Composable
private fun StripKey(
    letter: String,
    description: String,
    on: Boolean,
    onColor: Color,
    onToggle: (Boolean) -> Unit,
    enabled: Boolean,
    modifier: Modifier,
) {
    val colors = BandEarTheme.colors
    val interaction = remember { MutableInteractionSource() }
    Box(
        modifier = modifier
            .height(StripKeySlotHeight)
            .alpha(if (enabled) 1f else DisabledKeyAlpha)
            .toggleable(
                value = on,
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onToggle,
            )
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        // The press shows on the drawn key only, not on the touch target's margin around it.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(StripKeyHeight)
                .clip(BandEarShapes.Key)
                .background(if (on) onColor else colors.surfaceRaised)
                .border(1.dp, if (on) onColor else colors.border, BandEarShapes.Key)
                .indication(interaction, LocalIndication.current),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = letter,
                style = BandEarType.Number.copy(fontSize = 12.sp),
                color = if (on) colors.onAccent else colors.text,
            )
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun StripKeysPreview() {
    BandEarTheme {
        var muted by remember { mutableStateOf(true) }
        var soloed by remember { mutableStateOf(false) }
        Column(
            modifier = Modifier
                .width(92.dp)
                .padding(vertical = 8.dp),
        ) {
            StripKeys(muted, soloed, { muted = it }, { soloed = it }, "CH 4")
            StripKeys(muted = false, soloed = true, onMute = {}, onSolo = {}, channelLabel = "CH 5")
            StripKeys(false, false, {}, {}, "CH 6", enabled = false)
            HeldTag(LineIcons.Lock, "Locked")
        }
    }
}
