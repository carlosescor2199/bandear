package com.mixlink.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.components.GroupChip
import com.mixlink.android.ui.components.HorizontalFader
import com.mixlink.android.ui.components.MuteSoloButtons
import com.mixlink.android.ui.components.VerticalFader
import com.mixlink.android.ui.components.groupToneFor
import com.mixlink.android.ui.components.panText
import com.mixlink.android.ui.state.ChannelUiState
import com.mixlink.android.ui.state.GroupUiState
import com.mixlink.android.ui.state.MixUiState
import com.mixlink.android.ui.state.MoreOfMeUiState
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme
import com.mixlink.android.ui.theme.MixLinkType

/** Two 44 dp keys, the gap between them and the strip's padding. */
private val StripWidth = 104.dp

/** The strip's padding above its content; a channel strip's label button sits closer to the top. */
private val StripTopPadding = 10.dp
private val ChannelStripTopPadding = 6.dp

/**
 * A channel strip's fader. The label button is 44 dp tall where the label was one line of about
 * 17 dp, and it sits 4 dp closer to the top, so the fader gives up the remaining 24 dp of its usual
 * 236 dp and the strip, and with it the Mix screen, is no taller than before.
 */
private val ChannelFaderHeight = 212.dp

/** The fill colour of a strip's fader for [fill]. */
@Composable
internal fun stripColor(fill: StripFill): Color {
    val colors = MixLinkTheme.colors
    return when (fill) {
        StripFill.ACCENT -> colors.accent
        StripFill.IDLE -> colors.faderIdle
        StripFill.MUTED -> colors.faderMuted
    }
}

/**
 * One strip per channel, scrolling sideways; the musician's own strip stands out. A strip's label
 * opens that channel's sheet through [onOpen].
 */
@Composable
internal fun ChannelRow(
    channels: List<ChannelUiState>,
    moreOfMe: MoreOfMeUiState,
    onLevel: (channel: Int, level: Int) -> Unit,
    onMuted: (channel: Int, muted: Boolean) -> Unit,
    onSoloed: (channel: Int, soloed: Boolean) -> Unit,
    onOpen: (channel: Int) -> Unit,
) {
    if (channels.isEmpty()) {
        EmptyRow("Waiting for the server to announce its channels.")
        return
    }
    LazyRow(
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(channels, key = { it.index }) { channel ->
            ChannelStrip(
                channel = channel,
                mine = channel.index == moreOfMe.musicianChannel,
                moreOfMeOn = moreOfMe.enabled,
                onLevel = { onLevel(channel.index, it) },
                onMuted = { onMuted(channel.index, it) },
                onSoloed = { onSoloed(channel.index, it) },
                onOpen = { onOpen(channel.index) },
            )
        }
    }
}

/** One fader per group the server announced, scrolling sideways. */
@Composable
internal fun GroupRow(groups: List<GroupUiState>, onLevel: (group: Int, level: Int) -> Unit) {
    if (groups.isEmpty()) {
        EmptyRow("The server has no groups.")
        return
    }
    LazyRow(
        contentPadding = PaddingValues(horizontal = 16.dp),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        items(groups, key = { it.index }) { group ->
            GroupStrip(group = group, onLevel = { onLevel(group.index, it) })
        }
    }
}

@Composable
private fun ChannelStrip(
    channel: ChannelUiState,
    mine: Boolean,
    moreOfMeOn: Boolean,
    onLevel: (Int) -> Unit,
    onMuted: (Boolean) -> Unit,
    onSoloed: (Boolean) -> Unit,
    onOpen: () -> Unit,
) {
    val colors = MixLinkTheme.colors
    val highlighted = mine && moreOfMeOn
    StripPanel(
        background = if (mine) colors.surfaceSelected else colors.surface,
        border = if (highlighted) colors.accent else colors.border,
        top = ChannelStripTopPadding,
    ) {
        StripLabelButton(channel = channel, onOpen = onOpen)
        val group = channel.group
        if (group != null) {
            GroupChip(group.name, groupToneFor(group.index))
        } else {
            Text(
                text = "No group",
                modifier = Modifier.padding(vertical = 2.dp),
                style = MaterialTheme.typography.labelSmall,
                color = colors.textSecondary,
            )
        }
        VerticalFader(
            value = channel.level,
            onValueChange = onLevel,
            accent = stripColor(stripFillFor(channel.muted, mine, moreOfMeOn)),
            contentDescription = stripFaderDescription(channel.label, mine, channel.muted),
            height = ChannelFaderHeight,
        )
        Text(
            text = stripValueText(channel.level, channel.muted),
            style = MixLinkType.Number,
            color = if (channel.muted) colors.textSecondary else colors.text,
        )
        MuteSoloButtons(
            muted = channel.muted,
            soloed = channel.soloed,
            onMute = onMuted,
            onSolo = onSoloed,
            channelLabel = channel.label,
        )
    }
}

@Composable
private fun GroupStrip(group: GroupUiState, onLevel: (Int) -> Unit) {
    val colors = MixLinkTheme.colors
    StripPanel(background = colors.surface, border = colors.border) {
        GroupChip(group.name, groupToneFor(group.index))
        VerticalFader(
            value = group.level,
            onValueChange = onLevel,
            accent = colors.faderIdle,
            contentDescription = "${group.name} group level",
        )
        Text(stripValueText(group.level, muted = false), style = MixLinkType.Number, color = colors.text)
        Text(
            text = groupMembersText(group.channels),
            modifier = Modifier.heightIn(min = 44.dp),
            style = MaterialTheme.typography.labelSmall,
            color = colors.textSecondary,
            textAlign = TextAlign.Center,
            maxLines = 3,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/**
 * The channel's label as a button that opens its sheet, with the channel's pan written under it
 * ("L30", "C", "R20") so the pan shows without opening anything. A full 44 dp tall and as wide as
 * the strip.
 */
@Composable
private fun StripLabelButton(channel: ChannelUiState, onOpen: () -> Unit) {
    val colors = MixLinkTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 44.dp)
            .clip(MixLinkShapes.Key)
            .background(colors.surfaceRaised)
            .border(1.dp, colors.border, MixLinkShapes.Key)
            .clickable(role = Role.Button, onClickLabel = "Open the channel's settings", onClick = onOpen)
            .semantics {
                contentDescription = channelSettingsLabel(channel.index)
                stateDescription = channelPanState(channel.pan)
            },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(channel.label, style = MixLinkType.Number, color = colors.text)
        Text(panText(channel.pan), style = MixLinkType.NumberSmall.copy(fontSize = 11.sp), color = colors.textSecondary)
    }
}

@Composable
private fun StripPanel(
    background: Color,
    border: Color,
    top: Dp = StripTopPadding,
    content: @Composable () -> Unit,
) {
    Column(
        modifier = Modifier
            .width(StripWidth)
            .clip(MixLinkShapes.Panel)
            .background(background)
            .border(1.dp, border, MixLinkShapes.Panel)
            .padding(start = 6.dp, end = 6.dp, top = top, bottom = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        content()
    }
}

/** Takes the row's place while it has nothing to show, and says why. */
@Composable
private fun EmptyRow(message: String) {
    val colors = MixLinkTheme.colors
    Box(
        modifier = Modifier
            .padding(horizontal = 16.dp)
            .fillMaxWidth()
            .heightIn(min = 120.dp)
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .padding(16.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(message, style = MaterialTheme.typography.bodyMedium, color = colors.textSecondary, textAlign = TextAlign.Center)
    }
}

/** The master level and its Mute, and the level cap that protects the musician's ears. */
@Composable
internal fun MasterCard(
    mix: MixUiState,
    onMasterVolume: (Int) -> Unit,
    onMasterMuted: (Boolean) -> Unit,
    onLevelCap: (Int) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MixLinkTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(MixLinkShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .padding(horizontal = 14.dp, vertical = 12.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("MASTER", style = MixLinkType.Caps, color = colors.textSecondary)
            Text(
                text = masterValueText(mix.masterVolume, mix.masterMuted),
                style = MixLinkType.Number.copy(fontSize = 15.sp),
                color = if (mix.masterMuted) colors.textSecondary else colors.text,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            HorizontalFader(
                value = mix.masterVolume,
                onValueChange = onMasterVolume,
                accent = if (mix.masterMuted) colors.faderMuted else colors.accent,
                contentDescription = if (mix.masterMuted) "Master level, muted" else "Master level",
                modifier = Modifier.weight(1f),
            )
            MasterMuteKey(muted = mix.masterMuted, onMuted = onMasterMuted)
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text("Level cap ", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
            Text("${mix.levelCap}%", style = MixLinkType.NumberSmall.copy(fontSize = 13.sp), color = colors.text)
            Text(" · protects your ears", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
        }
        HorizontalFader(
            value = mix.levelCap,
            onValueChange = onLevelCap,
            accent = colors.faderIdle,
            contentDescription = "Level cap, the loudest this phone plays",
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/** The master Mute: filled red and reading "Muted" while on, so it is not told by colour alone. */
@Composable
private fun MasterMuteKey(muted: Boolean, onMuted: (Boolean) -> Unit) {
    val colors = MixLinkTheme.colors
    Box(
        modifier = Modifier
            .widthIn(min = 64.dp)
            .height(44.dp)
            .clip(MixLinkShapes.Control)
            .background(if (muted) colors.red else colors.surfaceRaised)
            .border(1.dp, if (muted) colors.red else colors.border, MixLinkShapes.Control)
            .toggleable(value = muted, role = Role.Switch, onValueChange = onMuted)
            .semantics { contentDescription = "Mute master" }
            .padding(horizontal = 12.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = masterMuteText(muted),
            style = MaterialTheme.typography.labelLarge,
            color = if (muted) colors.onAccent else colors.text,
        )
    }
}
