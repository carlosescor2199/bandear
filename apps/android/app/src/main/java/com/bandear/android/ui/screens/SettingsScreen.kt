package com.bandear.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.BandEarTextField
import com.bandear.android.ui.components.FieldFace
import com.bandear.android.ui.components.FieldLabel
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.LinkDot
import com.bandear.android.ui.components.PrimaryButton
import com.bandear.android.ui.components.StatusBanner
import com.bandear.android.ui.components.isLandscapeLayout
import com.bandear.android.ui.components.linkHealthDescription
import com.bandear.android.ui.components.pillToken
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.DiagnosticsUiState
import com.bandear.android.ui.state.MAX_NAME_LENGTH
import com.bandear.android.ui.state.ManualAddress
import com.bandear.android.ui.state.MonitorUiState
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType
import com.bandear.android.ui.theme.ColorToken

/** The back link's height and the gap under it, above the title of the manual address form. */
private val BackLinkSpace = 52.dp

/** The title's line and the gap under it, so the landscape Diagnostics start level with the server card. */
private val LandscapeTitleSpace = 52.dp

/**
 * The session's server with Disconnect, the musician's name, the manual connection form and the
 * diagnostics: the phone's own link health, six key readings, and an expander with every counter
 * and the recent errors.
 *
 * Opened from Connect, with no session, it is the manual address form: [onBack] then returns to
 * Connect, and a refused Connect shows its reason under the form.
 *
 * In portrait it is one column; in [landscape] the connection (server, name, manual address) sits
 * beside the diagnostics, and each side scrolls on its own when it does not fit.
 */
@Composable
fun SettingsScreen(
    musicianName: String,
    connection: ConnectionUiState,
    diagnostics: DiagnosticsUiState,
    onNameChange: (String) -> Unit,
    onManualAddressChange: (ManualAddress) -> Unit,
    onConnectManual: (host: String, controlPort: String, audioPort: String) -> Unit,
    onDisconnect: () -> Unit,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
    initiallyExpanded: Boolean = false,
    landscape: Boolean = isLandscapeLayout(),
) {
    val colors = BandEarTheme.colors
    // Kept here, above both layouts, so the counters stay open when the phone turns.
    var expanded by rememberSaveable { mutableStateOf(initiallyExpanded) }
    val topSpace = if (onBack == null) 0.dp else BackLinkSpace

    val connectionParts = @Composable {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (onBack != null) BackToServers(onBack)
            ScreenHeading("Settings", landscape)
        }
        ServerCard(connection, onDisconnect, landscape)
        NameSection(musicianName, onNameChange, landscape)
        ManualConnectionSection(connection, onManualAddressChange, onConnectManual, landscape)
    }
    val diagnosticsPart = @Composable {
        DiagnosticsSection(
            diagnostics = diagnostics,
            controlStatus = connection.statusDetail,
            expanded = expanded,
            onExpandedChange = { expanded = it },
            landscape = landscape,
        )
    }

    if (landscape) {
        val top = if (onBack == null) 22.dp else 8.dp
        Row(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .padding(horizontal = 24.dp),
            horizontalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            Column(
                modifier = Modifier
                    .weight(5f)
                    .fillMaxHeight()
                    .verticalScroll(rememberScrollState())
                    .padding(top = top, bottom = 24.dp),
                verticalArrangement = Arrangement.spacedBy(22.dp),
            ) {
                connectionParts()
            }
            Column(
                modifier = Modifier
                    .weight(4f)
                    .fillMaxHeight()
                    .verticalScroll(rememberScrollState())
                    .padding(top = top + topSpace + LandscapeTitleSpace, bottom = 24.dp),
            ) {
                diagnosticsPart()
            }
        }
    } else {
        Column(
            modifier = modifier
                .fillMaxSize()
                .background(colors.ground)
                .verticalScroll(rememberScrollState())
                .padding(start = 20.dp, end = 20.dp, top = if (onBack == null) 28.dp else 8.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            connectionParts()
            diagnosticsPart()
        }
    }
}

@Composable
private fun BackToServers(onBack: () -> Unit) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .heightIn(min = 48.dp)
            .clip(BandEarShapes.Control)
            .clickable(role = Role.Button, onClick = onBack)
            .padding(end = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(LineIcons.Back, contentDescription = null, tint = colors.accent, modifier = Modifier.size(20.dp))
        Text(
            text = "Servers",
            style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
            color = colors.accent,
        )
    }
}

/** Where the phone is connected, in words beside the coloured dot, and the way out. */
@Composable
private fun ServerCard(connection: ConnectionUiState, onDisconnect: () -> Unit, landscape: Boolean) {
    val colors = BandEarTheme.colors
    val token = if (connection.sessionRunning) pillToken(connection.pill) else ColorToken.NEUTRAL
    GroupedCard(outlined = landscape) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 68.dp)
                .padding(start = 16.dp, end = 12.dp, top = 10.dp, bottom = 10.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier
                    .size(8.dp)
                    .background(colors.solid(token), CircleShape),
            )
            Column(
                modifier = Modifier
                    .weight(1f)
                    .semantics(mergeDescendants = true) {},
                verticalArrangement = Arrangement.spacedBy(2.dp),
            ) {
                Text(
                    text = serverCardTitle(connection),
                    style = MaterialTheme.typography.bodyLarge.copy(
                        fontSize = if (landscape) 15.sp else 16.sp,
                        fontWeight = FontWeight.SemiBold,
                    ),
                    color = colors.text,
                )
                Text(
                    text = serverCardDetail(connection),
                    style = MaterialTheme.typography.bodySmall.copy(fontSize = if (landscape) 12.sp else 13.sp),
                    color = colors.textSecondary,
                )
            }
            if (connection.sessionRunning) DisconnectButton(onDisconnect)
        }
    }
}

/**
 * Ends the session, in the boards' danger look: red words on a red tint, so it reads as the way
 * out without the weight of the solid red that confirms a delete.
 */
@Composable
private fun DisconnectButton(onClick: () -> Unit) {
    val colors = BandEarTheme.colors
    Box(
        modifier = Modifier
            .heightIn(min = 48.dp)
            .clip(BandEarShapes.Control)
            .background(colors.tint(ColorToken.RED))
            .border(1.dp, colors.tintBorder(ColorToken.RED), BandEarShapes.Control)
            .clickable(role = Role.Button, onClick = onClick)
            .padding(horizontal = 14.dp),
        contentAlignment = Alignment.Center,
    ) {
        Text(text = "Disconnect", style = MaterialTheme.typography.labelLarge, color = colors.red)
    }
}

@Composable
private fun NameSection(musicianName: String, onNameChange: (String) -> Unit, landscape: Boolean) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FieldLabel("YOUR NAME")
        BandEarTextField(
            value = musicianName,
            onValueChange = onNameChange,
            label = "Your name",
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = controlHeight(landscape)),
            placeholder = "Your name",
            capitalization = KeyboardCapitalization.Words,
            maxLength = MAX_NAME_LENGTH,
        )
    }
}

@Composable
private fun ManualConnectionSection(
    connection: ConnectionUiState,
    onManualAddressChange: (ManualAddress) -> Unit,
    onConnectManual: (host: String, controlPort: String, audioPort: String) -> Unit,
    landscape: Boolean,
) {
    val colors = BandEarTheme.colors
    val manual = connection.manual
    val running = connection.sessionRunning
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("MANUAL CONNECTION", style = BandEarType.Caps, color = colors.textSecondary)
        GroupedCard(outlined = landscape) {
            AddressRow(label = "Server address") { fieldModifier ->
                BandEarTextField(
                    value = manual.host,
                    onValueChange = { onManualAddressChange(manual.copy(host = it)) },
                    label = "Server address",
                    modifier = fieldModifier,
                    placeholder = "192.168.1.34",
                    face = FieldFace.NUMBERS,
                    fill = colors.ground,
                    keyboardType = KeyboardType.Uri,
                    imeAction = ImeAction.Next,
                )
            }
            GroupedCardDivider()
            AddressRow(label = "Audio port") { fieldModifier ->
                BandEarTextField(
                    value = manual.audioPort,
                    onValueChange = { onManualAddressChange(manual.copy(audioPort = it)) },
                    label = "Audio port",
                    modifier = fieldModifier,
                    face = FieldFace.NUMBERS,
                    fill = colors.ground,
                    keyboardType = KeyboardType.Number,
                    imeAction = ImeAction.Next,
                )
            }
            GroupedCardDivider()
            AddressRow(label = "Control port") { fieldModifier ->
                BandEarTextField(
                    value = manual.controlPort,
                    onValueChange = { onManualAddressChange(manual.copy(controlPort = it)) },
                    label = "Control port",
                    modifier = fieldModifier,
                    face = FieldFace.NUMBERS,
                    fill = colors.ground,
                    keyboardType = KeyboardType.Number,
                )
            }
        }
        val problem = connection.problem
        if (problem != null && !running) {
            StatusBanner(
                token = ColorToken.RED,
                title = "Could not connect",
                detail = problem,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        PrimaryButton(
            text = "Connect to this address",
            onClick = { onConnectManual(manual.host, manual.controlPort, manual.audioPort) },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = controlHeight(landscape)),
            enabled = !running,
        )
        if (running) {
            Text(
                text = "Disconnect first to connect to another server.",
                style = MaterialTheme.typography.bodySmall,
                color = colors.textSecondary,
            )
        }
    }
}

/**
 * One row of the manual connection card: what the field holds on the left, the field on the
 * right. The field carries that name for a screen reader, so the words beside it are not read
 * twice.
 */
@Composable
private fun AddressRow(label: String, field: @Composable (Modifier) -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 16.dp, vertical = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            text = label,
            modifier = Modifier
                .weight(1f)
                .clearAndSetSemantics {},
            style = MaterialTheme.typography.bodyLarge,
            color = BandEarTheme.colors.text,
        )
        field(Modifier.weight(1.2f))
    }
}

@Composable
private fun DiagnosticsSection(
    diagnostics: DiagnosticsUiState,
    controlStatus: String,
    expanded: Boolean,
    onExpandedChange: (Boolean) -> Unit,
    landscape: Boolean,
) {
    val colors = BandEarTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("DIAGNOSTICS", style = BandEarType.Caps, color = colors.textSecondary)
            Row(
                modifier = Modifier.clearAndSetSemantics {
                    contentDescription = linkHealthDescription(diagnostics.linkHealth)
                },
                horizontalArrangement = Arrangement.spacedBy(6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                LinkDot(diagnostics.linkHealth)
                Text(
                    text = linkSummaryText(diagnostics.linkHealth),
                    style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.SemiBold),
                    color = colors.text,
                )
            }
        }
        ReadingsGrid(diagnosticsSummary(diagnostics), landscape)
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .clip(BandEarShapes.Control)
                .clickable(role = Role.Button) { onExpandedChange(!expanded) }
                .semantics { stateDescription = if (expanded) "Expanded" else "Collapsed" }
                .padding(horizontal = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (expanded) "Hide all counters and recent errors" else "Show all counters and recent errors",
                modifier = Modifier.weight(1f),
                style = MaterialTheme.typography.bodyLarge.copy(
                    fontSize = if (landscape) 14.sp else 15.sp,
                    fontWeight = FontWeight.Medium,
                ),
                color = colors.text,
            )
            Icon(
                imageVector = LineIcons.ChevronDown,
                contentDescription = null,
                tint = colors.textSecondary,
                modifier = Modifier
                    .size(16.dp)
                    .rotate(if (expanded) 180f else 0f),
            )
        }
        if (expanded) {
            GroupedCard(outlined = landscape) {
                Box(Modifier.padding(horizontal = 16.dp, vertical = 14.dp)) { AllCounters(diagnostics, controlStatus) }
            }
        }
    }
}

/** The six key readings, two to a row, split by hairlines. */
@Composable
private fun ReadingsGrid(readings: List<Reading>, landscape: Boolean) {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(BandEarShapes.Panel)
            .background(colors.border)
            .then(if (landscape) Modifier.border(1.dp, colors.border, BandEarShapes.Panel) else Modifier),
        verticalArrangement = Arrangement.spacedBy(1.dp),
    ) {
        readings.chunked(2).forEach { pair ->
            Row(
                modifier = Modifier.height(IntrinsicSize.Min),
                horizontalArrangement = Arrangement.spacedBy(1.dp),
            ) {
                pair.forEach { reading ->
                    ReadingTile(
                        reading = reading,
                        landscape = landscape,
                        modifier = Modifier
                            .weight(1f)
                            .fillMaxHeight(),
                    )
                }
            }
        }
    }
}

@Composable
private fun ReadingTile(reading: Reading, landscape: Boolean, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .background(colors.surface)
            .padding(horizontal = if (landscape) 14.dp else 16.dp, vertical = if (landscape) 12.dp else 14.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(reading.label, style = MaterialTheme.typography.labelMedium, color = colors.textSecondary)
        Text(
            text = reading.value,
            style = BandEarType.Number.copy(fontSize = if (landscape) 17.sp else 18.sp),
            color = colors.text,
        )
    }
}

/** Every counter and the recent errors; the text can be selected and copied, for a bug report. */
@Composable
private fun AllCounters(diagnostics: DiagnosticsUiState, controlStatus: String) {
    SelectionContainer { AllCountersText(diagnostics, controlStatus) }
}

@Composable
private fun AllCountersText(diagnostics: DiagnosticsUiState, controlStatus: String) {
    val colors = BandEarTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        allReadings(diagnostics, controlStatus).forEach { reading ->
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .semantics(mergeDescendants = true) {},
                horizontalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Text(
                    text = reading.label,
                    modifier = Modifier.weight(1f),
                    style = MaterialTheme.typography.bodySmall,
                    color = colors.textSecondary,
                )
                Text(
                    text = reading.value,
                    modifier = Modifier.weight(1f),
                    style = BandEarType.NumberSmall,
                    color = colors.text,
                    textAlign = TextAlign.End,
                )
            }
        }
        Text(
            text = "RECENT ERRORS",
            modifier = Modifier.padding(top = 8.dp),
            style = BandEarType.Caps,
            color = colors.textSecondary,
        )
        val errors = recentErrors(diagnostics)
        if (errors.isEmpty()) {
            Text("None", style = MaterialTheme.typography.bodySmall, color = colors.text)
        } else {
            errors.forEach { Text(it, style = MaterialTheme.typography.bodySmall, color = colors.text) }
        }
    }
}

/** The height of the name field and the Connect button: a little lower where landscape is short. */
private fun controlHeight(landscape: Boolean): Dp = if (landscape) 48.dp else 52.dp

@Composable
private fun SettingsPreview(
    state: MonitorUiState,
    onBack: (() -> Unit)? = null,
    expanded: Boolean = false,
    landscape: Boolean = false,
) {
    BandEarTheme {
        SettingsScreen(
            musicianName = state.musicianName,
            connection = state.connection,
            diagnostics = state.diagnostics,
            onNameChange = {},
            onManualAddressChange = {},
            onConnectManual = { _, _, _ -> },
            onDisconnect = {},
            onBack = onBack,
            initiallyExpanded = expanded,
            landscape = landscape,
        )
    }
}

@Preview(
    name = "Settings · connected",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SettingsConnectedPreview() = SettingsPreview(PreviewSamples.connected)

@Preview(
    name = "Settings · connected, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SettingsConnectedLandscapePreview() = SettingsPreview(PreviewSamples.connected, landscape = true)

@Preview(
    name = "Settings · lost the server, all counters",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = 1300,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SettingsExpandedPreview() = SettingsPreview(PreviewSamples.lostServer, expanded = true)

@Preview(
    name = "Settings · manual address, refused",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SettingsManualPreview() = SettingsPreview(PreviewSamples.refused, onBack = {})

@Preview(
    name = "Settings · manual address, refused, landscape",
    widthDp = PREVIEW_HEIGHT_DP,
    heightDp = PREVIEW_WIDTH_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SettingsManualLandscapePreview() = SettingsPreview(PreviewSamples.refused, onBack = {}, landscape = true)
