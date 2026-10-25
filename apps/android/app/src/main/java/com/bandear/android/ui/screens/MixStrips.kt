package com.bandear.android.ui.screens

import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.indication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.HeldTag
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.ScribbleStrip
import com.bandear.android.ui.components.ScribbleTone
import com.bandear.android.ui.components.StripKeySlotHeight
import com.bandear.android.ui.components.StripKeys
import com.bandear.android.ui.components.StripPan
import com.bandear.android.ui.components.VerticalFader
import com.bandear.android.ui.components.groupChipDescription
import com.bandear.android.ui.components.groupToneFor
import com.bandear.android.ui.components.scribbleColors
import com.bandear.android.ui.components.scribbleToneFor
import com.bandear.android.ui.state.ChannelUiState
import com.bandear.android.ui.state.GroupUiState
import com.bandear.android.ui.state.MixUiState
import com.bandear.android.ui.state.MoreOfMeUiState
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType
import com.bandear.android.ui.theme.ColorToken
import kotlin.math.floor

/**
 * A strip is never narrower than this, so its keys, pan and label stay usable; while the row has
 * room to spare its strips widen up to [StripMaxWidth] and fill it.
 */
private val StripMinWidth = 76.dp
private val StripMaxWidth = 96.dp
private val StripGap = 6.dp

/** The master strip at the end of the row: wider in landscape, as the boards draw it. */
internal val MasterStripWidth = 88.dp
internal val MasterStripWidthLandscape = 112.dp

/** How tall the master's Mute key and level-cap button are drawn, each in a 48 dp touch target. */
private val MasterKeyHeight = 32.dp
private val CapButtonHeight = 40.dp
private val CapButtonTouchHeight = 48.dp

/** The drawn label at the foot of a group strip, as tall as a channel's scribble strip. */
private val GroupLabelHeight = 40.dp

/** A held strip's value fades with its controls. */
private const val HeldAlpha = 0.4f

/** The fill colour of a strip's fader for [fill]. */
@Composable
internal fun stripColor(fill: StripFill): Color {
    val colors = BandEarTheme.colors
    return when (fill) {
        StripFill.ACCENT -> colors.accent
        StripFill.IDLE -> colors.faderIdle
        StripFill.MUTED -> colors.faderMuted
    }
}

/**
 * One console strip per channel, side by side and scrolling sideways through [state] when they do
 * not all fit; the musician's own strip stands out. A strip's scribble strip opens that channel's
 * panel through [onOpen].
 */
@Composable
internal fun ChannelRow(
    channels: List<ChannelUiState>,
    moreOfMe: MoreOfMeUiState,
    state: LazyListState,
    onLevel: (channel: Int, level: Int) -> Unit,
    onPan: (channel: Int, pan: Int) -> Unit,
    onMuted: (channel: Int, muted: Boolean) -> Unit,
    onSoloed: (channel: Int, soloed: Boolean) -> Unit,
    onOpen: (channel: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (channels.isEmpty()) {
        EmptyRow("Waiting for the server to announce its channels.", modifier)
        return
    }
    StripRow(count = channels.size, state = state, modifier = modifier) { stripWidth ->
        items(channels, key = { it.index }) { channel ->
            ChannelStrip(
                channel = channel,
                mine = channel.index == moreOfMe.musicianChannel,
                moreOfMeOn = moreOfMe.enabled,
                onLevel = { onLevel(channel.index, it) },
                onPan = { onPan(channel.index, it) },
                onMuted = { onMuted(channel.index, it) },
                onSoloed = { onSoloed(channel.index, it) },
                onOpen = { onOpen(channel.index) },
                modifier = Modifier
                    .width(stripWidth)
                    .fillMaxHeight(),
            )
        }
    }
}

/** One fader strip per group the server announced, scrolling sideways through [state]. */
@Composable
internal fun GroupRow(
    groups: List<GroupUiState>,
    state: LazyListState,
    onLevel: (group: Int, level: Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    if (groups.isEmpty()) {
        EmptyRow("The server has no groups.", modifier)
        return
    }
    StripRow(count = groups.size, state = state, modifier = modifier) { stripWidth ->
        items(groups, key = { it.index }) { group ->
            GroupStrip(
                group = group,
                onLevel = { onLevel(group.index, it) },
                modifier = Modifier
                    .width(stripWidth)
                    .fillMaxHeight(),
            )
        }
    }
}

/**
 * The row of [count] strips: each [StripMinWidth] wide when they overflow the row, so it scrolls,
 * or as wide as fills the row (up to [StripMaxWidth]) when they all fit, so a landscape window
 * shows every strip it has room for without a gap at the end.
 */
@Composable
private fun StripRow(
    count: Int,
    state: LazyListState,
    modifier: Modifier,
    content: LazyListScope.(stripWidth: Dp) -> Unit,
) {
    BoxWithConstraints(modifier) {
        val density = LocalDensity.current
        val stripWidth = with(density) {
            // Whole pixels, rounded down, so strips that exactly fill the row never overflow it by one.
            val room = constraints.maxWidth - StripGap.roundToPx() * (count - 1)
            val fill = floor(room.toFloat() / count).toDp()
            fill.coerceIn(StripMinWidth, StripMaxWidth)
        }
        LazyRow(
            modifier = Modifier.fillMaxSize(),
            state = state,
            horizontalArrangement = Arrangement.spacedBy(StripGap),
        ) {
            content(stripWidth)
        }
    }
}

/**
 * A channel's console strip, top to bottom: its M and S keys, or why it takes no input ("Locked",
 * "Following") in their place; its pan; its fader, as tall as the strip allows; its level in
 * words; and its scribble strip, which names it and its group and opens its panel.
 *
 * The musician's own strip sits on the selected surface with an amber pan dot and an amber
 * scribble strip; while More of me lifts it, its border and fader are amber too. A held strip's
 * pan, fader and level fade and take no input; its scribble strip still opens the panel, which
 * says why in words.
 */
@Composable
private fun ChannelStrip(
    channel: ChannelUiState,
    mine: Boolean,
    moreOfMeOn: Boolean,
    onLevel: (Int) -> Unit,
    onPan: (Int) -> Unit,
    onMuted: (Boolean) -> Unit,
    onSoloed: (Boolean) -> Unit,
    onOpen: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    val highlighted = mine && moreOfMeOn
    val following = channel.following
    val open = !channel.locked && following == null
    StripPanel(
        background = if (mine) colors.surfaceSelected else colors.surface,
        border = if (highlighted) colors.tintBorder(ColorToken.AMBER) else Color.Transparent,
        modifier = modifier,
    ) {
        // The lock outlasts a mirror, so a locked follower says "Locked" here, and both in its
        // spoken names and its panel.
        when {
            channel.locked -> HeldTag(LineIcons.Lock, LOCKED_TAG)
            following != null -> HeldTag(LineIcons.Follow, FOLLOWING_TAG)
            else -> StripKeys(
                muted = channel.muted,
                soloed = channel.soloed,
                onMute = onMuted,
                onSolo = onSoloed,
                channelLabel = lockedLabel(channel.label, channel.locked, following),
            )
        }
        // A stereo pair's pan is its balance: the same control, named as the panel names it.
        StripPan(
            pan = channel.pan,
            onPanChange = onPan,
            contentDescription = lockedLabel(channelPanDescription(channel.label, channel.stereo), channel.locked, following),
            modifier = Modifier.fillMaxWidth(),
            enabled = open,
            dotColor = if (mine) colors.accent else colors.textSecondary,
        )
        VerticalFader(
            value = channel.level,
            onValueChange = onLevel,
            accent = stripColor(stripFillFor(channel.muted, mine, moreOfMeOn)),
            contentDescription = stripFaderDescription(channel.label, mine, channel.muted, channel.locked, following),
            modifier = Modifier.weight(1f),
            enabled = open,
        )
        StripValue(
            text = stripValueText(channel.level, channel.muted),
            color = when {
                channel.muted -> colors.red
                highlighted -> colors.accent
                else -> colors.text
            },
            held = !open,
        )
        val group = channel.group
        val subtitle = group?.name ?: "No group"
        ScribbleStrip(
            title = channel.label,
            subtitle = subtitle,
            tone = scribbleToneFor(mine, group?.index),
            contentDescription = channelSettingsLabel(channel),
            onClick = onOpen,
            onClickLabel = "Open the channel's settings",
            stateDescription = channelPanState(channel.pan, channel.stereo),
            // The group is read after the button as its chip was beside the label button. A held
            // strip shows its group here too, so it reads it as well, as iOS does.
            subtitleDescription = if (group != null) groupChipDescription(group.name) else subtitle,
            stereo = channel.stereo,
        )
    }
}

/**
 * A group's strip in the same shell, level only: the "Following" tag where the keys would be while
 * the mix follows another's (faded and taking no input then), the fader, the level, and the group's
 * name over its channels on the group's tone.
 */
@Composable
private fun GroupStrip(group: GroupUiState, onLevel: (Int) -> Unit, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    val following = group.following
    StripPanel(background = colors.surface, border = Color.Transparent, modifier = modifier) {
        if (following != null) {
            HeldTag(LineIcons.Follow, FOLLOWING_TAG)
        } else {
            // The keys' slot stays, so a group's fader starts where a channel's keys end.
            Spacer(Modifier.height(StripKeySlotHeight))
        }
        VerticalFader(
            value = group.level,
            onValueChange = onLevel,
            accent = colors.faderIdle,
            contentDescription = groupFaderDescription(group.name, following),
            modifier = Modifier.weight(1f),
            enabled = following == null,
        )
        StripValue(text = stripValueText(group.level, muted = false), color = colors.text, held = following != null)
        GroupLabel(name = group.name, members = groupMembersText(group.members), tone = ScribbleTone.Group(groupToneFor(group.index)))
    }
}

/** The level under a strip's fader, in numbers. */
@Composable
private fun StripValue(text: String, color: Color, held: Boolean) {
    Text(
        text = text,
        modifier = Modifier.alpha(if (held) HeldAlpha else 1f),
        style = BandEarType.Number.copy(fontSize = 13.sp, lineHeight = 18.sp),
        color = color,
        maxLines = 1,
    )
}

/**
 * The foot of a group strip, drawn as a scribble strip on the group's [tone]: its [name] over its
 * [members]. It is not a button: a group has no panel to open. A screen reader reads the name as
 * the group chip on the strip was read ("Group: Drums"), then the members.
 */
@Composable
private fun GroupLabel(name: String, members: String, tone: ScribbleTone) {
    val colors = scribbleColors(tone, BandEarTheme.colors)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = StripKeySlotHeight),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = GroupLabelHeight)
                .clip(BandEarShapes.Key)
                .background(colors.background)
                .padding(horizontal = 6.dp, vertical = 2.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(2.dp, alignment = Alignment.CenterVertically),
        ) {
            Text(
                text = name,
                modifier = Modifier.clearAndSetSemantics { contentDescription = groupChipDescription(name) },
                style = MaterialTheme.typography.labelLarge.copy(fontSize = 13.sp, lineHeight = 16.sp),
                color = colors.content,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            // A long list is cut short here; a screen reader reads it whole.
            Text(
                text = members,
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp, lineHeight = 12.sp),
                color = colors.content,
                textAlign = TextAlign.Center,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
    }
}

/** The body of a console strip: a rounded panel, its parts centred one above the other. */
@Composable
private fun StripPanel(
    background: Color,
    border: Color,
    modifier: Modifier = Modifier,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier = modifier
            .clip(BandEarShapes.Panel)
            .background(background)
            .border(1.dp, border, BandEarShapes.Panel)
            .padding(start = 6.dp, end = 6.dp, bottom = 4.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        content = content,
    )
}

/** Takes the row's place while it has nothing to show, and says why. */
@Composable
private fun EmptyRow(message: String, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Box(
        modifier = modifier
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(message, style = MaterialTheme.typography.bodyMedium, color = colors.textSecondary, textAlign = TextAlign.Center)
    }
}

/**
 * The master strip, pinned at the end of the row: "MASTER", its Mute key, the master fader with
 * the engineer's limit marked across it when there is one, the master level in words, and a button
 * showing the level cap that opens the master's panel through [onOpenPanel].
 */
@Composable
internal fun MasterStrip(
    mix: MixUiState,
    onMasterVolume: (Int) -> Unit,
    onMasterMuted: (Boolean) -> Unit,
    onOpenPanel: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    val engineer = mix.engineer
    StripPanel(background = colors.surface, border = colors.border, modifier = modifier) {
        Text(
            text = "MASTER",
            modifier = Modifier.padding(top = 8.dp),
            style = BandEarType.Caps.copy(fontSize = 11.sp),
            color = colors.textSecondary,
            maxLines = 1,
        )
        MasterMuteKey(muted = mix.masterMuted, onMuted = onMasterMuted)
        VerticalFader(
            value = mix.masterVolume,
            onValueChange = onMasterVolume,
            accent = if (mix.masterMuted) colors.faderMuted else colors.textSecondary,
            contentDescription = if (mix.masterMuted) "Master level, muted" else "Master level",
            modifier = Modifier.weight(1f),
            limitMarker = if (engineer.limited) engineer.limitPercent else null,
        )
        StripValue(
            text = masterValueText(mix.masterVolume, mix.masterMuted),
            color = if (mix.masterMuted) colors.red else colors.text,
            held = false,
        )
        LevelCapButton(levelCap = mix.levelCap, onClick = onOpenPanel)
    }
}

/**
 * The master Mute: filled red and reading "Muted" while on, so it is not told by colour alone.
 * Drawn 32 dp tall across the strip, in a 48 dp touch target.
 */
@Composable
private fun MasterMuteKey(muted: Boolean, onMuted: (Boolean) -> Unit) {
    val colors = BandEarTheme.colors
    val interaction = remember { MutableInteractionSource() }
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(StripKeySlotHeight)
            .toggleable(
                value = muted,
                interactionSource = interaction,
                indication = null,
                role = Role.Switch,
                onValueChange = onMuted,
            )
            .semantics { contentDescription = "Mute master" },
        contentAlignment = Alignment.Center,
    ) {
        // The press shows on the drawn key only, not on the touch target's margin around it.
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(MasterKeyHeight)
                .clip(BandEarShapes.Key)
                .background(if (muted) colors.red else colors.surfaceRaised)
                .border(1.dp, if (muted) colors.red else colors.border, BandEarShapes.Key)
                .indication(interaction, LocalIndication.current),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                text = masterMuteText(muted),
                style = MaterialTheme.typography.labelLarge.copy(fontSize = 13.sp),
                color = if (muted) colors.onAccent else colors.text,
            )
        }
    }
}

/**
 * The foot of the master strip: the phone's level cap, which protects the musician's ears, as a
 * button that opens the master's panel where it is set. A screen reader reads its two lines.
 */
@Composable
private fun LevelCapButton(levelCap: Int, onClick: () -> Unit) {
    val colors = BandEarTheme.colors
    val interaction = remember { MutableInteractionSource() }
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = CapButtonTouchHeight)
            .clickable(interactionSource = interaction, indication = null, role = Role.Button, onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = CapButtonHeight)
                .clip(BandEarShapes.Key)
                .background(colors.surfaceRaised)
                .indication(interaction, LocalIndication.current)
                .padding(horizontal = 4.dp, vertical = 2.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(1.dp, alignment = Alignment.CenterVertically),
        ) {
            Text(
                text = "Level cap",
                style = MaterialTheme.typography.labelSmall.copy(fontSize = 10.sp, lineHeight = 12.sp),
                color = colors.textSecondary,
                maxLines = 1,
            )
            Text(
                text = "$levelCap%",
                style = BandEarType.NumberSmall.copy(fontSize = 13.sp, lineHeight = 16.sp),
                color = colors.text,
                maxLines = 1,
            )
        }
    }
}
