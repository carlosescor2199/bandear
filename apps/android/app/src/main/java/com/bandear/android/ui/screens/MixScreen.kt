package com.bandear.android.ui.screens

import androidx.compose.foundation.BorderStroke
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
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.lazy.rememberLazyListState
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
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.MoreOfMeSwitch
import com.bandear.android.ui.components.SegmentedChoice
import com.bandear.android.ui.components.StatusBanner
import com.bandear.android.ui.components.StatusPill
import com.bandear.android.ui.components.isLandscapeLayout
import com.bandear.android.ui.state.ChannelChoice
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.MixUiState
import com.bandear.android.ui.state.MonitorUiState
import com.bandear.android.ui.state.MoreOfMeUiState
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType
import com.bandear.android.ui.theme.ColorToken

/** The screen's side margins: wider in portrait, tighter beside the landscape rail. */
private val PortraitGutter = 16.dp
private val LandscapeGutter = 12.dp

/**
 * The least height the console keeps for its strips: their keys, pan, value and scribble strip,
 * and a fader long enough to set by hand. A window too short to leave it scrolls instead.
 */
private val MinConsoleHeight = 260.dp

/** The "My channel" button never takes more of the header than this; a long name is cut short. */
private val PickerMaxWidth = 140.dp

/** The "My channel" button is drawn 36 dp tall, as the More of me switch beside it is. */
private val PickerMargin = 6.dp

/**
 * The main screen of a session, laid out as a mixing console: who and where, the connection, More
 * of me with the musician's channel, whether the row shows channels or groups, and the row of
 * strips filling the rest of the screen with the master strip pinned at its end.
 *
 * In portrait the header takes two lines over More of me and the section title, and the strips
 * scroll sideways while the master stays put; "Swipe for more" beside the title says so while they
 * overflow. In landscape the header, the Channels/Groups choice and More of me share one line and
 * the row shows every strip it has room for. The screen itself does not scroll, unless the window
 * is too short to leave the strips [MinConsoleHeight].
 *
 * Whether the row shows channels or groups is this screen's own choice; it starts at
 * [initialView]. A strip's scribble strip opens that channel's panel, with its pan, level, Mute and
 * Solo, and the master's level-cap button opens the master's panel; which panel is open is this
 * screen's own state too, kept across a rotation, and a channel's panel closes if the channel goes
 * away.
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
    val landscape = isLandscapeLayout()
    val gutter = if (landscape) LandscapeGutter else PortraitGutter
    var view by rememberSaveable { mutableStateOf(initialView) }
    var openChannel by rememberSaveable { mutableStateOf<Int?>(null) }
    var masterOpen by rememberSaveable { mutableStateOf(false) }
    val channelRowState = rememberLazyListState()
    val groupRowState = rememberLazyListState()
    val rowState = if (view == MixView.CHANNELS) channelRowState else groupRowState
    val channels = remember(mix) { mix.channels }
    val groups = remember(mix) { mix.groups }
    val sheetChannel = sheetChannelFor(channels, openChannel)
    // A channel the server no longer announces has no panel; it does not reopen if it comes back.
    LaunchedEffect(openChannel, sheetChannel == null) {
        if (sheetChannel == null) openChannel = null
    }
    val channelLabel = moreOfMe.musicianChannel?.let(mix::labelOf)

    ConsoleLayout(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground),
        top = {
            Column(
                modifier = Modifier.padding(start = gutter, end = gutter, top = if (landscape) 8.dp else 12.dp),
                verticalArrangement = Arrangement.spacedBy(if (landscape) 6.dp else 8.dp),
            ) {
                if (landscape) {
                    MixHeader(musicianName, connection, compact = true) {
                        SegmentedChoice(
                            options = MixView.entries.map { it.label },
                            selected = view.ordinal,
                            onSelect = { view = MixView.entries[it] },
                        )
                        MoreOfMeControls(moreOfMe, channelLabel, mix.musicianChoices, onMoreOfMe, onMusicianChannel)
                    }
                } else {
                    MixHeader(musicianName, connection, compact = false)
                }
                MixBanners(connection, mix.following)
                if (!landscape) {
                    MoreOfMeControls(moreOfMe, channelLabel, mix.musicianChoices, onMoreOfMe, onMusicianChannel, fill = true)
                }
                MoreOfMeNoticeLine(moreOfMe)
                if (!landscape) {
                    SectionHeader(
                        view = view,
                        count = if (view == MixView.CHANNELS) channels.size else groups.size,
                        rowState = rowState,
                        onView = { view = it },
                    )
                }
            }
        },
        console = {
            Row(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(start = gutter, end = gutter, top = if (landscape) 6.dp else 4.dp, bottom = 12.dp),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                val rowModifier = Modifier
                    .weight(1f)
                    .fillMaxHeight()
                when (view) {
                    MixView.CHANNELS -> ChannelRow(
                        channels = channels,
                        moreOfMe = moreOfMe,
                        state = channelRowState,
                        onLevel = onChannelLevel,
                        onPan = onChannelPan,
                        onMuted = onChannelMuted,
                        onSoloed = onChannelSoloed,
                        onOpen = { openChannel = it },
                        modifier = rowModifier,
                    )
                    MixView.GROUPS -> GroupRow(groups = groups, state = groupRowState, onLevel = onGroupLevel, modifier = rowModifier)
                }
                MasterStrip(
                    mix = mix,
                    onMasterVolume = onMasterVolume,
                    onMasterMuted = onMasterMuted,
                    onOpenPanel = { masterOpen = true },
                    modifier = Modifier
                        .width(if (landscape) MasterStripWidthLandscape else MasterStripWidth)
                        .fillMaxHeight(),
                )
            }
        },
    )

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
    if (masterOpen) {
        MasterPanel(mix = mix, onLevelCap = onLevelCap, onDismiss = { masterOpen = false })
    }
}

/**
 * The screen's two parts: [top] as tall as it needs, and [console] filling the height that is left,
 * so the faders are as long as the window allows. When that leaves the console less than
 * [MinConsoleHeight], the console keeps that height and the whole screen scrolls instead.
 */
@Composable
private fun ConsoleLayout(
    top: @Composable () -> Unit,
    console: @Composable () -> Unit,
    modifier: Modifier = Modifier,
) {
    BoxWithConstraints(modifier) {
        val viewport = constraints.maxHeight
        val least = with(LocalDensity.current) { MinConsoleHeight.roundToPx() }
        Layout(
            contents = listOf(top, console),
            modifier = Modifier.verticalScroll(rememberScrollState()),
        ) { (topParts, consoleParts), scrolled ->
            val width = scrolled.maxWidth
            val topPlaced = topParts.map { it.measure(Constraints(maxWidth = width)) }
            val topHeight = topPlaced.sumOf { it.height }
            val room = if (viewport == Constraints.Infinity) least else viewport - topHeight
            val consoleHeight = maxOf(room, least)
            val consolePlaced = consoleParts.map { it.measure(Constraints.fixed(width, consoleHeight)) }
            layout(width, topHeight + consoleHeight) {
                var y = 0
                topPlaced.forEach {
                    it.placeRelative(0, y)
                    y += it.height
                }
                consolePlaced.forEach { it.placeRelative(0, topHeight) }
            }
        }
    }
}

/**
 * Who and where: the musician's name over the server, and the connection pill at the end. The
 * [compact] landscape header puts [controls] (the Channels/Groups choice and More of me) between
 * them on the same line.
 */
@Composable
private fun MixHeader(
    musicianName: String,
    connection: ConnectionUiState,
    compact: Boolean,
    controls: @Composable () -> Unit = {},
) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(if (compact) 10.dp else 12.dp),
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
                    style = if (compact) MaterialTheme.typography.titleSmall else MaterialTheme.typography.titleLarge,
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
        controls()
        StatusPill(connection.pill)
    }
}

/**
 * The banners under the header: the connection's while the control channel is away, and while the
 * engineer has this mix follow another musician's, whose, and what stays the musician's.
 */
@Composable
private fun MixBanners(connection: ConnectionUiState, following: String?) {
    val banner = connectionBanner(connection)
    if (banner != null) {
        StatusBanner(
            token = banner.token,
            title = banner.title,
            detail = banner.detail,
            busy = banner.busy,
            modifier = Modifier.fillMaxWidth(),
        )
    }
    if (following != null) {
        StatusBanner(
            token = ColorToken.BLUE,
            title = followingTitle(following),
            detail = FOLLOWING_DETAIL,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

/**
 * The More of me switch and, beside it, the musician's channel, named [channelLabel] as its strip
 * names it. With [fill] the switch takes the room the button leaves.
 */
@Composable
private fun MoreOfMeControls(
    moreOfMe: MoreOfMeUiState,
    channelLabel: String?,
    choices: List<ChannelChoice>,
    onMoreOfMe: (Boolean) -> Unit,
    onMusicianChannel: (Int?) -> Unit,
    fill: Boolean = false,
) {
    Row(
        modifier = if (fill) Modifier.fillMaxWidth() else Modifier,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        MoreOfMeSwitch(
            on = moreOfMe.enabled,
            channelLabel = channelLabel.orEmpty(),
            onToggle = onMoreOfMe,
            modifier = if (fill) Modifier.weight(1f) else Modifier,
        )
        MyChannelPicker(
            selected = moreOfMe.musicianChannel,
            selectedLabel = channelLabel,
            choices = choices,
            onSelect = onMusicianChannel,
            modifier = Modifier.widthIn(max = PickerMaxWidth),
        )
    }
}

/** Why More of me did not do what was asked, said once to a screen reader as it appears. */
@Composable
private fun MoreOfMeNoticeLine(moreOfMe: MoreOfMeUiState) {
    val notice = moreOfMe.notice ?: return
    Text(
        text = notice.message,
        modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
        style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
        color = BandEarTheme.colors.accent,
    )
}

/**
 * The musician's channel as a compact menu button, and a menu of the announced channels to change
 * it: one entry per strip, by name, so a hidden channel is not offered and a stereo pair is offered
 * once. Drawn 36 dp tall in a 48 dp touch target; a long name is cut short here, and the menu and
 * the button's spoken name carry it whole.
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
    val interaction = remember { MutableInteractionSource() }
    val label = selectedLabel

    fun choose(channel: Int?) {
        open = false
        onSelect(channel)
    }

    Box(modifier = modifier) {
        Row(
            modifier = Modifier
                .heightIn(min = 48.dp)
                .clickable(
                    interactionSource = interaction,
                    indication = null,
                    role = Role.Button,
                    onClickLabel = "Choose your channel",
                ) { open = true }
                .semantics { contentDescription = myChannelDescription(label) }
                // The press shows on the drawn button only, not on the touch target's margin.
                .padding(vertical = PickerMargin)
                .clip(BandEarShapes.Control)
                .background(colors.surface)
                .border(1.dp, colors.border, BandEarShapes.Control)
                .indication(interaction, LocalIndication.current)
                .padding(start = 10.dp, end = 6.dp),
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(modifier = Modifier.weight(1f, fill = false)) {
                Text(
                    text = "MY CHANNEL",
                    style = BandEarType.Caps.copy(fontSize = 9.sp, lineHeight = 11.sp, letterSpacing = 0.08.em),
                    color = colors.textSecondary,
                    maxLines = 1,
                )
                Text(
                    text = myChannelText(label),
                    style = BandEarType.Number.copy(fontSize = 13.sp, lineHeight = 16.sp),
                    color = colors.text,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            Icon(LineIcons.ChevronDown, contentDescription = null, tint = colors.text, modifier = Modifier.size(16.dp))
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

/**
 * The portrait title over the row, with how many it holds, the Channels/Groups choice at its end,
 * and "Swipe for more" beside the title while the strips in [rowState] run past the screen's edge.
 */
@Composable
private fun SectionHeader(view: MixView, count: Int, rowState: LazyListState, onView: (MixView) -> Unit) {
    val colors = BandEarTheme.colors
    val overflows = rowState.canScrollForward || rowState.canScrollBackward
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            modifier = Modifier.weight(1f),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(sectionTitle(view, count), style = BandEarType.Caps, color = colors.textSecondary, maxLines = 1)
            if (overflows) {
                // Wraps to a second line rather than crowd the choice beside it on a narrow phone.
                Text(
                    text = SWIPE_FOR_MORE,
                    modifier = Modifier.weight(1f, fill = false),
                    style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight.Normal, lineHeight = 14.sp),
                    color = colors.neutral,
                    maxLines = 2,
                )
            }
        }
        SegmentedChoice(
            options = MixView.entries.map { it.label },
            selected = view.ordinal,
            onSelect = { onView(MixView.entries[it]) },
        )
    }
}

/** A preview landscape phone, as the landscape boards draw it, in dp. */
private const val LANDSCAPE_PREVIEW_WIDTH_DP = 844
private const val LANDSCAPE_PREVIEW_HEIGHT_DP = 390

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
    name = "Mix · console, portrait",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixConsolePortraitPreview() = MixPreview(PreviewSamples.console)

@Preview(
    name = "Mix · console, landscape",
    widthDp = LANDSCAPE_PREVIEW_WIDTH_DP,
    heightDp = LANDSCAPE_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixConsoleLandscapePreview() = MixPreview(PreviewSamples.console)

@Preview(
    name = "Mix · groups, landscape",
    widthDp = LANDSCAPE_PREVIEW_WIDTH_DP,
    heightDp = LANDSCAPE_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixGroupsLandscapePreview() = MixPreview(PreviewSamples.console, initialView = MixView.GROUPS)

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
    name = "Mix · following, landscape",
    widthDp = LANDSCAPE_PREVIEW_WIDTH_DP,
    heightDp = LANDSCAPE_PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun MixFollowingLandscapePreview() = MixPreview(PreviewSamples.following)

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
