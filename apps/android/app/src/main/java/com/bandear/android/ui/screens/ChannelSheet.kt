package com.bandear.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.BottomSheetDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.GroupChip
import com.bandear.android.ui.components.HorizontalFader
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.PAN_CENTRE
import com.bandear.android.ui.components.PAN_LEFT
import com.bandear.android.ui.components.PAN_RIGHT
import com.bandear.android.ui.components.PanControl
import com.bandear.android.ui.components.SecondaryButton
import com.bandear.android.ui.components.groupToneFor
import com.bandear.android.ui.components.kindIcon
import com.bandear.android.ui.components.panText
import com.bandear.android.ui.state.ChannelUiState
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType
import kotlinx.coroutines.launch

/** The sheet's top corners; its bottom sits on the screen's edge. */
private val SheetShape = RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp)

/** A locked channel's keys fade as the strip's do. */
private const val DISABLED_KEY_ALPHA = 0.4f

/**
 * One channel's sheet, opened from its strip's label: the pan, which the strips have no room for,
 * then the channel's level, Mute and Solo, so everything about the channel is in one place.
 *
 * It slides up over the Mix screen and closes with Done, a swipe down, a tap outside it or the back
 * key, each of which ends in [onDismiss]. It always shows [channel] as it is now, so a change made
 * on the strip shows here at once.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ChannelSheet(
    channel: ChannelUiState,
    mine: Boolean,
    moreOfMeOn: Boolean,
    onDismiss: () -> Unit,
    onPan: (Int) -> Unit,
    onLevel: (Int) -> Unit,
    onMuted: (Boolean) -> Unit,
    onSoloed: (Boolean) -> Unit,
) {
    val colors = BandEarTheme.colors
    val sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = sheetState,
        shape = SheetShape,
        containerColor = colors.surface,
        contentColor = colors.text,
        tonalElevation = 0.dp,
        scrimColor = Color.Black.copy(alpha = 0.6f),
        dragHandle = { BottomSheetDefaults.DragHandle(color = colors.textSecondary) },
    ) {
        ChannelSheetContent(
            channel = channel,
            mine = mine,
            moreOfMeOn = moreOfMeOn,
            onPan = onPan,
            onLevel = onLevel,
            onMuted = onMuted,
            onSoloed = onSoloed,
            onDone = {
                // Slide the sheet away first, then forget it, as a swipe down does.
                scope.launch { sheetState.hide() }.invokeOnCompletion {
                    if (!sheetState.isVisible) onDismiss()
                }
            },
        )
    }
}

/** What the channel sheet holds, apart from the sheet itself, so previews can show it. */
@Composable
internal fun ChannelSheetContent(
    channel: ChannelUiState,
    mine: Boolean,
    moreOfMeOn: Boolean,
    onPan: (Int) -> Unit,
    onLevel: (Int) -> Unit,
    onMuted: (Boolean) -> Unit,
    onSoloed: (Boolean) -> Unit,
    onDone: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        SheetHeader(channel, mine)
        // A held channel's controls fade and take no input; the notes say why in words, both when
        // the channel is locked and the mix follows another's.
        val following = channel.following
        val open = !channel.locked && following == null
        if (following != null) HeldNote(LineIcons.Follow, followingTitle(following))
        if (channel.locked) HeldNote(LineIcons.Lock, LOCKED_NOTE)
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            // A stereo pair's pan is its balance: the same control, written as both pans.
            ValueRow(title = panTitle(channel.stereo), value = panText(channel.pan), dim = false)
            PanControl(
                pan = channel.pan,
                onPanChange = onPan,
                contentDescription = lockedLabel(channelPanDescription(channel.label, channel.stereo), channel.locked, following),
                modifier = Modifier.fillMaxWidth(),
                enabled = open,
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            ValueRow(title = "LEVEL", value = stripValueText(channel.level, channel.muted), dim = channel.muted)
            HorizontalFader(
                value = channel.level,
                onValueChange = onLevel,
                accent = stripColor(stripFillFor(channel.muted, mine, moreOfMeOn)),
                contentDescription = stripFaderDescription(channel.label, mine, channel.muted, channel.locked, following),
                modifier = Modifier.fillMaxWidth(),
                enabled = open,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            SheetToggleKey(
                text = muteKeyText(channel.muted),
                description = "Mute ${lockedLabel(channel.label, channel.locked, following)}",
                on = channel.muted,
                onColor = colors.red,
                onToggle = onMuted,
                enabled = open,
                modifier = Modifier.weight(1f),
            )
            SheetToggleKey(
                text = soloKeyText(channel.soloed),
                description = "Solo ${lockedLabel(channel.label, channel.locked, following)}",
                on = channel.soloed,
                onColor = colors.accent,
                onToggle = onSoloed,
                enabled = open,
                modifier = Modifier.weight(1f),
            )
        }
        SecondaryButton(text = "Done", onClick = onDone, modifier = Modifier.fillMaxWidth())
    }
}

/**
 * The channel with the icon of what it carries, its group, whether it is a stereo pair (in words,
 * with the stereo rings) and whether it is the musician's own. A long name takes what room is left
 * and wraps to a second line rather than pushing the rest away.
 */
@Composable
private fun SheetHeader(channel: ChannelUiState, mine: Boolean) {
    val colors = BandEarTheme.colors
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp), verticalAlignment = Alignment.CenterVertically) {
        val kind = channel.kind
        if (kind != null) {
            Icon(kindIcon(kind), contentDescription = kind.label, tint = colors.textSecondary, modifier = Modifier.size(20.dp))
        }
        Text(
            text = channel.label,
            modifier = Modifier
                .weight(1f, fill = false)
                .semantics { heading() },
            style = BandEarType.NumberLarge.copy(fontSize = 20.sp),
            color = colors.text,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        val group = channel.group
        if (group != null) {
            GroupChip(group.name, groupToneFor(group.index))
        } else {
            Text("No group", style = MaterialTheme.typography.labelSmall, color = colors.textSecondary)
        }
        if (channel.stereo) {
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(LineIcons.Stereo, contentDescription = null, tint = colors.textSecondary, modifier = Modifier.size(14.dp))
                Text(STEREO_TAG, style = MaterialTheme.typography.labelMedium, color = colors.textSecondary)
            }
        }
        if (mine) {
            Text("Your channel", style = MaterialTheme.typography.labelMedium, color = colors.accent)
        }
    }
}

/**
 * A control's caption and its value. The control under it tells a screen reader both, so the row
 * is hidden from it rather than read twice.
 */
@Composable
private fun ValueRow(title: String, value: String, dim: Boolean) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clearAndSetSemantics {},
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = BandEarType.Caps, color = colors.textSecondary)
        Text(
            text = value,
            style = BandEarType.Number.copy(fontSize = 15.sp),
            color = if (dim) colors.textSecondary else colors.text,
        )
    }
}

/**
 * Why a channel's controls take no input, over them, as an [icon] and [text] in words ("Locked by
 * the engineer", "Following Ana's mix").
 */
@Composable
private fun HeldNote(icon: ImageVector, text: String) {
    val colors = BandEarTheme.colors
    Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, tint = colors.text, modifier = Modifier.size(18.dp))
        Text(text, style = MaterialTheme.typography.labelLarge, color = colors.text)
    }
}

/**
 * A wide Mute or Solo key: filled and reading "Muted" or "Soloed" while on, not only coloured. A
 * disabled key fades but keeps its colours, so it still shows whether it is on.
 */
@Composable
private fun SheetToggleKey(
    text: String,
    description: String,
    on: Boolean,
    onColor: Color,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
) {
    val colors = BandEarTheme.colors
    Box(
        modifier = modifier
            .height(48.dp)
            .alpha(if (enabled) 1f else DISABLED_KEY_ALPHA)
            .clip(BandEarShapes.Control)
            .background(if (on) onColor else colors.surfaceRaised)
            .border(1.dp, if (on) onColor else colors.border, BandEarShapes.Control)
            .toggleable(value = on, enabled = enabled, role = Role.Switch, onValueChange = onToggle)
            .semantics { contentDescription = description },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = text,
            style = MaterialTheme.typography.labelLarge,
            color = if (on) colors.onAccent else colors.text,
        )
    }
}

/** The sheet as it rests over the Mix screen; a preview cannot show the real sheet's window. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun SheetPreview(channel: ChannelUiState, mine: Boolean = false) {
    BandEarTheme {
        val colors = BandEarTheme.colors
        Box(
            modifier = Modifier
                .fillMaxSize()
                .background(colors.ground),
            contentAlignment = Alignment.BottomCenter,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clip(SheetShape)
                    .background(colors.surface),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                BottomSheetDefaults.DragHandle(color = colors.textSecondary)
                ChannelSheetContent(
                    channel = channel,
                    mine = mine,
                    moreOfMeOn = true,
                    onPan = {},
                    onLevel = {},
                    onMuted = {},
                    onSoloed = {},
                    onDone = {},
                )
            }
        }
    }
}

private const val SHEET_PREVIEW_HEIGHT_DP = 480

@Preview(
    name = "Channel sheet · centre, your channel",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = SHEET_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ChannelSheetCentrePreview() =
    SheetPreview(PreviewSamples.connected.mix.channels[2].copy(pan = PAN_CENTRE), mine = true)

@Preview(
    name = "Channel sheet · hard left",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = SHEET_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ChannelSheetHardLeftPreview() = SheetPreview(PreviewSamples.connected.mix.channels[0].copy(pan = PAN_LEFT))

@Preview(
    name = "Channel sheet · hard right, muted",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = SHEET_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ChannelSheetHardRightPreview() = SheetPreview(PreviewSamples.connected.mix.channels[3].copy(pan = PAN_RIGHT))

@Preview(
    name = "Channel sheet · locked by the engineer",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = SHEET_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ChannelSheetLockedPreview() = SheetPreview(PreviewSamples.engineerControls.mix.channels[0])
