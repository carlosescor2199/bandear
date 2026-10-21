package com.bandear.android.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.MoreOfMeToggle
import com.bandear.android.ui.components.SegmentedChoice
import com.bandear.android.ui.components.StatusBanner
import com.bandear.android.ui.components.StatusPill
import com.bandear.android.ui.state.ChannelChoice
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.MixUiState
import com.bandear.android.ui.state.MonitorUiState
import com.bandear.android.ui.state.MoreOfMeUiState
import com.bandear.android.ui.theme.ColorToken
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/** The "My channel" button is as wide as a channel strip, so the two columns line up. */
private val PickerWidth = 104.dp

/**
 * The main screen of a session: who and where, the connection, More of me with the musician's
 * channel, a row of strips that scrolls sideways, and the Master card.
 *
 * More of me and the strips come first, so on a 390 x 844 dp phone both are on screen without
 * scrolling; the page scrolls only to reach the rest of the Master card on smaller screens or while
 * a banner shows. Whether the row shows channels or groups is this screen's own choice; it starts
 * at [initialView].
 *
 * A strip's label opens that channel's sheet, with its pan, level, Mute and Solo; which channel's
 * sheet is open is this screen's own state too, and the sheet closes if the channel goes away.
 *
 * It reads only the sub-states it shows, so the receiver's statistics, which change about a
 * hundred times a second, never recompose it.
 */
@Composable
fun MixScreen(
    musicianName: String,
    connection: ConnectionUiState,
    mix: MixUiState,
    moreOfMe: MoreOfMeUiState,
    onMoreOfMe: (Boolean) -> Unit,
    onMusicianChannel: (Int?) -> Unit,
    onChannelLevel: (channel: Int, level: Int) -> Unit,
    onChannelPan: (channel: Int, pan: Int) -> Unit,
    onChannelMuted: (channel: Int, muted: Boolean) -> Unit,
    onChannelSoloed: (channel: Int, soloed: Boolean) -> Unit,
    onGroupLevel: (group: Int, level: Int) -> Unit,
    onMasterVolume: (Int) -> Unit,
    onMasterMuted: (Boolean) -> Unit,
    onLevelCap: (Int) -> Unit,
    modifier: Modifier = Modifier,
    initialView: MixView = MixView.CHANNELS,
) {
    val colors = BandEarTheme.colors
    var view by rememberSaveable { mutableStateOf(initialView) }
    var openChannel by rememberSaveable { mutableStateOf<Int?>(null) }
    val channels = remember(mix) { mix.channels }
    val groups = remember(mix) { mix.groups }
    val banner = connectionBanner(connection)
    val sheetChannel = sheetChannelFor(channels, openChannel)
    // A channel the server no longer announces has no sheet; it does not reopen if it comes back.
    LaunchedEffect(openChannel, sheetChannel == null) {
        if (sheetChannel == null) openChannel = null
    }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground)
            .verticalScroll(rememberScrollState()),
    ) {
        MixHeader(musicianName, connection)
        if (banner != null) {
            StatusBanner(
                token = banner.token,
                title = banner.title,
                detail = banner.detail,
                busy = banner.busy,
                modifier = Modifier
                    .padding(horizontal = 16.dp, vertical = 4.dp)
                    .fillMaxWidth(),
            )
        }
        // While the engineer has this mix follow another musician's, say whose, and what stays theirs.
        val following = mix.following
        if (following != null) {
            StatusBanner(
                token = ColorToken.BLUE,
                title = followingTitle(following),
                detail = FOLLOWING_DETAIL,
                modifier = Modifier
                    .padding(horizontal = 16.dp, vertical = 4.dp)
                    .fillMaxWidth(),
            )
        }
        MoreOfMeRow(
            moreOfMe = moreOfMe,
            channelLabel = moreOfMe.musicianChannel?.let(mix::labelOf),
            choices = mix.musicianChoices,
            onMoreOfMe = onMoreOfMe,
            onMusicianChannel = onMusicianChannel,
        )
        SectionHeader(
            view = view,
            count = if (view == MixView.CHANNELS) channels.size else groups.size,
            onView = { view = it },
        )
        when (view) {
            MixView.CHANNELS -> ChannelRow(
                channels = channels,
                moreOfMe = moreOfMe,
                onLevel = onChannelLevel,
                onMuted = onChannelMuted,
                onSoloed = onChannelSoloed,
                onOpen = { openChannel = it },
            )
            MixView.GROUPS -> GroupRow(groups = groups, onLevel = onGroupLevel)
        }
        MasterCard(
            mix = mix,
            onMasterVolume = onMasterVolume,
            onMasterMuted = onMasterMuted,
            onLevelCap = onLevelCap,
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 14.dp),
        )
        Spacer(Modifier.height(16.dp))
    }

    if (sheetChannel != null) {
        val index = sheetChannel.index
        ChannelSheet(
            channel = sheetChannel,
            mine = index == moreOfMe.musicianChannel,
            moreOfMeOn = moreOfMe.enabled,
            onDismiss = { openChannel = null },
            onPan = { onChannelPan(index, it) },
            onLevel = { onChannelLevel(index, it) },
            onMuted = { onChannelMuted(index, it) },
            onSoloed = { onChannelSoloed(index, it) },
        )
    }
}

@Composable
private fun MixHeader(musicianName: String, connection: ConnectionUiState) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 16.dp, top = 16.dp, bottom = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(
                    Modifier
                        .size(10.dp)
                        .background(colors.accent, BandEarShapes.Tag),
                )
                Text(
                    text = headerName(musicianName),
                    style = MaterialTheme.typography.titleMedium,
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Text(
                text = headerServerText(connection.server),
                style = BandEarType.NumberSmall,
                color = colors.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        StatusPill(connection.pill)
    }
}

/** More of me and the musician's channel, named [channelLabel] as its strip names it. */
@Composable
private fun MoreOfMeRow(
    moreOfMe: MoreOfMeUiState,
    channelLabel: String?,
    choices: List<ChannelChoice>,
    onMoreOfMe: (Boolean) -> Unit,
    onMusicianChannel: (Int?) -> Unit,
) {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 12.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .height(IntrinsicSize.Min),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            MoreOfMeToggle(
                on = moreOfMe.enabled,
                channelLabel = channelLabel.orEmpty(),
                onToggle = onMoreOfMe,
                modifier = Modifier
                    .weight(1f)
                    .fillMaxHeight(),
            )
            MyChannelPicker(
                selected = moreOfMe.musicianChannel,
                selectedLabel = channelLabel,
                choices = choices,
                onSelect = onMusicianChannel,
                modifier = Modifier
                    .width(PickerWidth)
                    .fillMaxHeight(),
            )
        }
        val notice = moreOfMe.notice
        if (notice != null) {
            Text(
                text = notice.message,
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                color = colors.accent,
            )
        }
    }
}

/**
 * The musician's channel, and a menu of the announced channels to change it: one entry per strip,
 * by name, so a hidden channel is not offered and a stereo pair is offered once.
 */
@Composable
private fun MyChannelPicker(
    selected: Int?,
    selectedLabel: String?,
    choices: List<ChannelChoice>,
    onSelect: (Int?) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = BandEarTheme.colors
    var open by remember { mutableStateOf(false) }
    val label = selectedLabel

    fun choose(channel: Int?) {
        open = false
        onSelect(channel)
    }

    Box(modifier = modifier) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .heightIn(min = 64.dp)
                .clip(BandEarShapes.Panel)
                .background(colors.surface)
                .border(1.dp, colors.border, BandEarShapes.Panel)
                .clickable(role = Role.Button, onClickLabel = "Choose your channel") { open = true }
                .semantics { contentDescription = myChannelDescription(label) }
                .padding(horizontal = 10.dp),
            verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
        ) {
            // Tighter than other caps labels so it stays on one line in a strip-wide button.
            Text(
                text = "MY CHANNEL",
                style = BandEarType.Caps.copy(fontSize = 11.sp, letterSpacing = 0.08.em),
                color = colors.textSecondary,
                maxLines = 1,
            )
            Row(verticalAlignment = Alignment.CenterVertically) {
                // A long name is cut short here; the menu and the button's spoken name carry it whole.
                Text(
                    text = myChannelText(label),
                    modifier = Modifier.weight(1f, fill = false),
                    style = BandEarType.NumberLarge,
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Icon(LineIcons.ChevronDown, contentDescription = null, tint = colors.text, modifier = Modifier.size(16.dp))
            }
        }
        DropdownMenu(
            expanded = open,
            onDismissRequest = { open = false },
            containerColor = colors.surfaceRaised,
            tonalElevation = 0.dp,
            border = BorderStroke(1.dp, colors.border),
        ) {
            ChannelMenuItem(text = "None", chosen = selected == null, onClick = { choose(null) })
            if (choices.isEmpty()) {
                DropdownMenuItem(
                    text = { Text("Waiting for the server's channels") },
                    onClick = {},
                    enabled = false,
                    colors = MenuDefaults.itemColors(disabledTextColor = colors.textSecondary),
                )
            }
            choices.forEach { choice ->
                ChannelMenuItem(text = choice.label, chosen = selected == choice.index, onClick = { choose(choice.index) })
            }
        }
    }
}

/** A channel in the picker's menu; the chosen one carries a tick, not only a bolder weight. */
@Composable
private fun ChannelMenuItem(text: String, chosen: Boolean, onClick: () -> Unit) {
    val colors = BandEarTheme.colors
    DropdownMenuItem(
        text = {
            Text(
                text = text,
                style = BandEarType.Number.copy(fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Medium),
            )
        },
        onClick = onClick,
        modifier = Modifier.semantics { selected = chosen },
        trailingIcon = if (chosen) {
            { Icon(LineIcons.Check, contentDescription = "Chosen", tint = colors.accent) }
        } else {
            null
        },
        colors = MenuDefaults.itemColors(textColor = colors.text),
    )
}

@Composable
private fun SectionHeader(view: MixView, count: Int, onView: (MixView) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 10.dp),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(sectionTitle(view, count), style = BandEarType.Caps, color = BandEarTheme.colors.textSecondary)
        SegmentedChoice(
            options = MixView.entries.map { it.label },
            selected = view.ordinal,
            onSelect = { onView(MixView.entries[it]) },
        )
    }
}

@Composable
private fun MixPreview(state: MonitorUiState, initialView: MixView = MixView.CHANNELS) {
    BandEarTheme {
        MixScreen(
            musicianName = state.musicianName,
            connection = state.connection,
            mix = state.mix,
            moreOfMe = state.moreOfMe,
            onMoreOfMe = {},
            onMusicianChannel = {},
            onChannelLevel = { _, _ -> },
            onChannelPan = { _, _ -> },
            onChannelMuted = { _, _ -> },
            onChannelSoloed = { _, _ -> },
            onGroupLevel = { _, _ -> },
            onMasterVolume = {},
            onMasterMuted = {},
            onLevelCap = {},
            initialView = initialView,
        )
    }
}

@Preview(
    name = "Mix · connected, More of me on",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixConnectedPreview() = MixPreview(PreviewSamples.connected)

@Preview(
    name = "Mix · More of me off",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixMoreOfMeOffPreview() = MixPreview(PreviewSamples.moreOfMeOff)

@Preview(
    name = "Mix · older server, channels by number",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixOlderServerPreview() = MixPreview(PreviewSamples.olderServer)

@Preview(
    name = "Mix · drums locked, limited by the engineer",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixEngineerControlsPreview() = MixPreview(PreviewSamples.engineerControls)

@Preview(
    name = "Mix · following Lead vocal's mix",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixFollowingPreview() = MixPreview(PreviewSamples.following)

@Preview(
    name = "Mix · following, groups",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixFollowingGroupsPreview() = MixPreview(PreviewSamples.following, initialView = MixView.GROUPS)

@Preview(
    name = "Mix · no channel chosen",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixNoChannelPreview() = MixPreview(PreviewSamples.noChannelChosen)

@Preview(
    name = "Mix · reconnecting",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixReconnectingPreview() = MixPreview(PreviewSamples.reconnecting)

@Preview(
    name = "Mix · lost the server",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixLostServerPreview() = MixPreview(PreviewSamples.lostServer)

@Preview(
    name = "Mix · groups",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixGroupsPreview() = MixPreview(PreviewSamples.connected, initialView = MixView.GROUPS)

@Preview(
    name = "Mix · waiting for channels",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixWaitingPreview() = MixPreview(PreviewSamples.waitingForChannels)
