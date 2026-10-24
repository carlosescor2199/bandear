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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.ColorToken

/** The switch is drawn 36 dp tall in the middle of a 48 dp touch target. */
private val SwitchTouchHeight = 48.dp
private val SwitchMargin = 6.dp

/** The track and knob drawn at the switch's end. */
private val TrackWidth = 36.dp
private val TrackHeight = 22.dp
private val TrackPadding = 2.dp
private val KnobSize = 18.dp
private val TrackShape = RoundedCornerShape(TrackHeight / 2)

private const val DisabledAlpha = 0.4f

/**
 * The compact More of me switch for the Mix screen's header: "More of me" and a switch, amber on
 * the amber tint with an amber border while it lifts the musician's channel ([channelLabel]), a
 * plain neutral panel while it is off.
 *
 * It means to a screen reader what [MoreOfMeToggle] does: a switch named "More of me" whose state
 * is the caption ([moreOfMeCaption]: "On · CH 3 lifted", "Off · tap to lift your channel"), so the
 * state is said in words. Drawn 36 dp tall in the middle of a 48 dp touch target; it is as wide as
 * its text and switch, or as wide as its modifier makes it, the switch then at the far end.
 */
@Composable
fun MoreOfMeSwitch(
    on: Boolean,
    channelLabel: String,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    val interaction = remember { MutableInteractionSource() }
    val background = if (on) colors.tint(ColorToken.AMBER) else colors.surface
    val edge = if (on) colors.tintBorder(ColorToken.AMBER) else colors.border
    val content = if (on) colors.accent else colors.text
    Row(
        modifier = modifier
            .heightIn(min = SwitchTouchHeight)
            .alpha(if (enabled) 1f else DisabledAlpha)
            .toggleable(
                value = on,
                interactionSource = interaction,
                indication = null,
                enabled = enabled,
                role = Role.Switch,
                onValueChange = onToggle,
            )
            .semantics { stateDescription = moreOfMeCaption(on, channelLabel) }
            // The press shows on the drawn panel only, not on the touch target's margin around it.
            .padding(vertical = SwitchMargin)
            .clip(BandEarShapes.Control)
            .background(background)
            .border(1.dp, edge, BandEarShapes.Control)
            .indication(interaction, LocalIndication.current)
            .padding(start = 12.dp, end = 6.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = "More of me",
            modifier = Modifier.padding(end = 8.dp),
            style = MaterialTheme.typography.labelLarge.copy(fontSize = 13.sp),
            color = content,
            maxLines = 1,
        )
        Box(
            modifier = Modifier
                .size(TrackWidth, TrackHeight)
                .background(if (on) colors.accent else colors.faderTrack, TrackShape)
                .padding(TrackPadding),
            contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart,
        ) {
            Box(
                Modifier
                    .size(KnobSize)
                    .background(if (on) colors.onAccent else colors.textSecondary, CircleShape),
            )
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun MoreOfMeSwitchPreview() {
    BandEarTheme {
        var on by remember { mutableStateOf(true) }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            MoreOfMeSwitch(on, "CH 3", { on = it })
            MoreOfMeSwitch(on = false, channelLabel = "CH 3", onToggle = {})
            MoreOfMeSwitch(on, "CH 3", { on = it }, Modifier.fillMaxWidth())
            MoreOfMeSwitch(on = true, channelLabel = "CH 3", onToggle = {}, enabled = false)
        }
    }
}
