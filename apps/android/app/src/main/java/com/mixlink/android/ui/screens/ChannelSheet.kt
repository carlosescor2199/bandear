package com.mixlink.android.ui.screens

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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.components.GroupChip
import com.mixlink.android.ui.components.HorizontalFader
import com.mixlink.android.ui.components.LineIcons
import com.mixlink.android.ui.components.PAN_CENTRE
import com.mixlink.android.ui.components.PAN_LEFT
import com.mixlink.android.ui.components.PAN_RIGHT
import com.mixlink.android.ui.components.PanControl
import com.mixlink.android.ui.components.SecondaryButton
import com.mixlink.android.ui.components.groupToneFor
import com.mixlink.android.ui.components.kindIcon
import com.mixlink.android.ui.components.panText
import com.mixlink.android.ui.state.ChannelUiState
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme
import com.mixlink.android.ui.theme.MixLinkType
import kotlinx.coroutines.launch

/** The sheet's top corners; its bottom sits on the screen's edge. */
private val SheetShape = RoundedCornerShape(topStart = 22.dp, topEnd = 22.dp)

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
    val colors = MixLinkTheme.colors
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
    val colors = MixLinkTheme.colors
    Column(
        modifier = modifier
            .fillMaxWidth()
            .padding(start = 20.dp, end = 20.dp, top = 4.dp, bottom = 20.dp),
        verticalArrangement = Arrangement.spacedBy(18.dp),
    ) {
        SheetHeader(channel, mine)
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            // A stereo pair's pan is its balance: the same control, written as both pans.
            ValueRow(title = panTitle(channel.stereo), value = panText(channel.pan), dim = false)
            PanControl(
                pan = channel.pan,
                onPanChange = onPan,
                contentDescription = channelPanDescription(channel.label, channel.stereo),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
            ValueRow(title = "LEVEL", value = stripValueText(channel.level, channel.muted), dim = channel.muted)
            HorizontalFader(
                value = channel.level,
                onValueChange = onLevel,
                accent = stripColor(stripFillFor(channel.muted, mine, moreOfMeOn)),
                contentDescription = stripFaderDescription(channel.label, mine, channel.muted),
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            SheetToggleKey(
                text = muteKeyText(channel.muted),
                description = "Mute ${channel.label}",
                on = channel.muted,
                onColor = colors.red,
                onToggle = onMuted,
                modifier = Modifier.weight(1f),
            )
            SheetToggleKey(
                text = soloKeyText(channel.soloed),
                description = "Solo ${channel.label}",
                on = channel.soloed,
                onColor = colors.accent,
                onToggle = onSoloed,
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
    val colors = MixLinkTheme.colors
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
            style = MixLinkType.NumberLarge.copy(fontSize = 20.sp),
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
    val colors = MixLinkTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clearAndSetSemantics {},
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(title, style = MixLinkType.Caps, color = colors.textSecondary)
        Text(
            text = value,
            style = MixLinkType.Number.copy(fontSize = 15.sp),
            color = if (dim) colors.textSecondary else colors.text,
        )
    }
}

/** A wide Mute or Solo key: filled and reading "Muted" or "Soloed" while on, not only coloured. */
@Composable
private fun SheetToggleKey(
    text: String,
    description: String,
    on: Boolean,
    onColor: Color,
    onToggle: (Boolean) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MixLinkTheme.colors
    Box(
        modifier = modifier
            .height(48.dp)
            .clip(MixLinkShapes.Control)
            .background(if (on) onColor else colors.surfaceRaised)
            .border(1.dp, if (on) onColor else colors.border, MixLinkShapes.Control)
            .toggleable(value = on, role = Role.Switch, onValueChange = onToggle)
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
    MixLinkTheme {
        val colors = MixLinkTheme.colors
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
