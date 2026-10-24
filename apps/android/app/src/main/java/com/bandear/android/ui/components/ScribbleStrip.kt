package com.bandear.android.ui.components

import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.theme.BandEarColors
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.ColorToken
import com.bandear.android.ui.theme.GroupTone

/** How tall a scribble strip is drawn; its touch target is [ScribbleTouchHeight]. */
private val ScribbleHeight = 40.dp

/** The scribble strip's touch target: the drawn strip plus 4 dp above and below it. */
private val ScribbleTouchHeight = 48.dp

/** A scribble strip's subtitle is a little quieter than its title. */
private const val SubtitleAlpha = 0.8f

/** What colours a scribble strip: the channel's group, the musician's own channel, or neither. */
@Immutable
sealed interface ScribbleTone {
    /** A channel in a group: the group's [tone] ([groupToneFor]). */
    data class Group(val tone: GroupTone) : ScribbleTone

    /** The musician's own channel: amber, whatever its group, so it is found at a glance. */
    data object Mine : ScribbleTone

    /** A channel in no group: plain text on a raised surface. */
    data object NoGroup : ScribbleTone
}

/** A scribble strip's [background] and the colour of its text and icon ([content]). */
@Immutable
data class ScribbleColors(val background: Color, val content: Color)

/**
 * The tone of a channel's scribble strip: [ScribbleTone.Mine] when it is the musician's own
 * channel ([mine]), otherwise the tone of the group at [groupIndex] in the server's group list, or
 * [ScribbleTone.NoGroup] when the channel is in no group (null).
 */
fun scribbleToneFor(mine: Boolean, groupIndex: Int?): ScribbleTone = when {
    mine -> ScribbleTone.Mine
    groupIndex != null -> ScribbleTone.Group(groupToneFor(groupIndex))
    else -> ScribbleTone.NoGroup
}

/**
 * The colours of a scribble strip in [tone], from [colors]: a group's chip colours, amber on the
 * amber tint for the musician's own channel, plain text on the raised surface for no group.
 */
fun scribbleColors(tone: ScribbleTone, colors: BandEarColors): ScribbleColors = when (tone) {
    is ScribbleTone.Group -> ScribbleColors(colors.groupBackground(tone.tone), colors.groupText(tone.tone))
    ScribbleTone.Mine -> ScribbleColors(colors.tint(ColorToken.AMBER), colors.accent)
    ScribbleTone.NoGroup -> ScribbleColors(colors.surfaceRaised, colors.text)
}

/**
 * The scribble strip at the bottom of a console strip: a button that names the strip with a
 * [title] (the channel's label, followed by the stereo rings when [stereo]) over a smaller
 * [subtitle] (its group, "No group", "My channel"), coloured by [tone] (see [scribbleColors]).
 *
 * It is as wide as the strip and drawn 40 dp tall in the middle of a 48 dp touch target, so a long
 * title is cut short here rather than wrapped. For a screen reader it is one button, built as the
 * strip's label button is today: named [contentDescription] (the full name, of which the visible
 * texts are the short form), with [stateDescription] as its state and [onClickLabel] as what a
 * double tap does, and nothing else: the visible texts are not read again after the name. The
 * stereo icon has no name of its own; the description says it.
 */
@Composable
fun ScribbleStrip(
    title: String,
    subtitle: String,
    tone: ScribbleTone,
    contentDescription: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    onClickLabel: String? = null,
    stateDescription: String? = null,
    stereo: Boolean = false,
) {
    val colors = scribbleColors(tone, BandEarTheme.colors)
    val interaction = remember { MutableInteractionSource() }
    Box(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = ScribbleTouchHeight)
            .clickable(
                interactionSource = interaction,
                indication = null,
                role = Role.Button,
                onClickLabel = onClickLabel,
                onClick = onClick,
            )
            // All a screen reader hears, in place of the texts inside. The role and the click
            // action are set again here so the button keeps them, with their label, whichever of
            // these two modifiers Compose applies last.
            .clearAndSetSemantics {
                this.contentDescription = contentDescription
                if (stateDescription != null) this.stateDescription = stateDescription
                role = Role.Button
                this.onClick(label = onClickLabel) {
                    onClick()
                    true
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        // The press shows on the drawn strip only, not on the touch target's margin around it.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = ScribbleHeight)
                .clip(BandEarShapes.Key)
                .background(colors.background)
                .indication(interaction, LocalIndication.current)
                .padding(horizontal = 6.dp, vertical = 2.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(2.dp, alignment = Alignment.CenterVertically),
        ) {
            Row(
                horizontalArrangement = Arrangement.spacedBy(4.dp, alignment = Alignment.CenterHorizontally),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    text = title,
                    modifier = Modifier.weight(1f, fill = false),
                    style = MaterialTheme.typography.labelLarge.copy(fontSize = 13.sp, lineHeight = 16.sp),
                    color = colors.content,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (stereo) {
                    Icon(LineIcons.Stereo, contentDescription = null, tint = colors.content, modifier = Modifier.size(12.dp))
                }
            }
            Text(
                text = subtitle,
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp, lineHeight = 12.sp),
                color = colors.content.copy(alpha = SubtitleAlpha),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun ScribbleStripPreview() {
    BandEarTheme {
        Row(
            modifier = Modifier.padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            val strip = Modifier.width(80.dp)
            ScribbleStrip("Snare", "Drums", scribbleToneFor(false, 0), "Snare", {}, strip)
            ScribbleStrip("Keys", "Band", scribbleToneFor(false, 1), "Keys", {}, strip, stereo = true)
            ScribbleStrip("Lead vox", "My channel", scribbleToneFor(true, 0), "Lead vox", {}, strip)
            ScribbleStrip("Click", "No group", scribbleToneFor(false, null), "Click", {}, strip)
        }
    }
}
