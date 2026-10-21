package com.bandear.android.ui.screens

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.bandear.android.ui.components.FieldFace
import com.bandear.android.ui.components.FieldLabel
import com.bandear.android.ui.components.LineIcons
import com.bandear.android.ui.components.LinkDot
import com.bandear.android.ui.components.BandEarTextField
import com.bandear.android.ui.components.SecondaryButton
import com.bandear.android.ui.components.StatusBanner
import com.bandear.android.ui.components.linkHealthDescription
import com.bandear.android.ui.components.pillToken
import com.bandear.android.ui.state.ConnectionUiState
import com.bandear.android.ui.state.DiagnosticsUiState
import com.bandear.android.ui.state.MAX_NAME_LENGTH
import com.bandear.android.ui.state.ManualAddress
import com.bandear.android.ui.state.MonitorUiState
import com.bandear.android.ui.theme.ColorToken
import com.bandear.android.ui.theme.BandEarShapes
import com.bandear.android.ui.theme.BandEarTheme
import com.bandear.android.ui.theme.BandEarType

/**
 * The session's server with Disconnect, the musician's name, the manual connection form and the
 * diagnostics: the phone's own link health, six key readings, and an expander with every counter
 * and the recent errors.
 *
 * Opened from Connect, with no session, it is the manual address form: [onBack] then returns to
 * Connect, and a refused Connect shows its reason under the form.
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
) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground)
            .verticalScroll(rememberScrollState())
            .padding(start = 16.dp, end = 16.dp, top = if (onBack == null) 20.dp else 8.dp, bottom = 16.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (onBack != null) BackToServers(onBack)
        Text("Settings", style = MaterialTheme.typography.titleLarge, color = colors.text)
        ServerCard(connection, onDisconnect)
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            FieldLabel("YOUR NAME")
            BandEarTextField(
                value = musicianName,
                onValueChange = onNameChange,
                label = "Your name",
                modifier = Modifier.fillMaxWidth(),
                placeholder = "Your name",
                capitalization = KeyboardCapitalization.Words,
                maxLength = MAX_NAME_LENGTH,
            )
        }
        ManualConnectionCard(connection, onManualAddressChange, onConnectManual)
        DiagnosticsCard(diagnostics, connection.statusDetail, initiallyExpanded)
    }
}

@Composable
private fun BackToServers(onBack: () -> Unit) {
    val colors = BandEarTheme.colors
    Row(
        modifier = Modifier
            .heightIn(min = 44.dp)
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
private fun ServerCard(connection: ConnectionUiState, onDisconnect: () -> Unit) {
    val colors = BandEarTheme.colors
    val token = if (connection.sessionRunning) pillToken(connection.pill) else ColorToken.NEUTRAL
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clip(BandEarShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(14.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(10.dp)
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
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                color = colors.text,
            )
            Text(serverCardDetail(connection), style = BandEarType.NumberSmall, color = colors.textSecondary)
        }
        if (connection.sessionRunning) SecondaryButton("Disconnect", onClick = onDisconnect)
    }
}

@Composable
private fun ManualConnectionCard(
    connection: ConnectionUiState,
    onManualAddressChange: (ManualAddress) -> Unit,
    onConnectManual: (host: String, controlPort: String, audioPort: String) -> Unit,
) {
    val colors = BandEarTheme.colors
    val manual = connection.manual
    val running = connection.sessionRunning
    SettingsCard {
        Text("MANUAL CONNECTION", style = BandEarType.Caps, color = colors.textSecondary)
        LabelledField(label = "Server address") {
            BandEarTextField(
                value = manual.host,
                onValueChange = { onManualAddressChange(manual.copy(host = it)) },
                label = "Server address",
                modifier = Modifier.fillMaxWidth(),
                placeholder = "192.168.1.34",
                face = FieldFace.NUMBERS,
                fill = colors.ground,
                keyboardType = KeyboardType.Uri,
                imeAction = ImeAction.Next,
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            LabelledField(label = "Audio port", modifier = Modifier.weight(1f)) {
                BandEarTextField(
                    value = manual.audioPort,
                    onValueChange = { onManualAddressChange(manual.copy(audioPort = it)) },
                    label = "Audio port",
                    modifier = Modifier.fillMaxWidth(),
                    face = FieldFace.NUMBERS,
                    fill = colors.ground,
                    keyboardType = KeyboardType.Number,
                    imeAction = ImeAction.Next,
                )
            }
            LabelledField(label = "Control port", modifier = Modifier.weight(1f)) {
                BandEarTextField(
                    value = manual.controlPort,
                    onValueChange = { onManualAddressChange(manual.copy(controlPort = it)) },
                    label = "Control port",
                    modifier = Modifier.fillMaxWidth(),
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
        SecondaryButton(
            text = "Connect to this address",
            onClick = { onConnectManual(manual.host, manual.controlPort, manual.audioPort) },
            modifier = Modifier.fillMaxWidth(),
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

@Composable
private fun DiagnosticsCard(diagnostics: DiagnosticsUiState, controlStatus: String, initiallyExpanded: Boolean) {
    val colors = BandEarTheme.colors
    var expanded by rememberSaveable { mutableStateOf(initiallyExpanded) }
    SettingsCard {
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
                    style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                    color = colors.text,
                )
            }
        }
        diagnosticsSummary(diagnostics).chunked(2).forEach { pair ->
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                pair.forEach { reading -> ReadingTile(reading, Modifier.weight(1f)) }
            }
        }
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 44.dp)
                .clickable(role = Role.Button) { expanded = !expanded }
                .semantics { stateDescription = if (expanded) "Expanded" else "Collapsed" },
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                text = if (expanded) "Hide all counters and recent errors" else "Show all counters and recent errors",
                style = MaterialTheme.typography.labelLarge,
                color = colors.accent,
            )
        }
        if (expanded) AllCounters(diagnostics, controlStatus)
    }
}

@Composable
private fun ReadingTile(reading: Reading, modifier: Modifier = Modifier) {
    val colors = BandEarTheme.colors
    Column(
        modifier = modifier
            .clip(BandEarShapes.Control)
            .background(colors.ground)
            .padding(horizontal = 10.dp, vertical = 8.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(2.dp),
    ) {
        Text(reading.label, style = MaterialTheme.typography.labelMedium, color = colors.textSecondary)
        Text(reading.value, style = BandEarType.Number.copy(fontSize = 15.sp), color = colors.text)
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

@Composable
private fun LabelledField(label: String, modifier: Modifier = Modifier, field: @Composable () -> Unit) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(6.dp)) {
        FieldLabel(label, caps = false)
        field()
    }
}

/** A Settings card: a bordered surface holding a column of rows. */
@Composable
private fun SettingsCard(content: @Composable () -> Unit) {
    val colors = BandEarTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .clip(BandEarShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, BandEarShapes.Panel)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        content()
    }
}

@Composable
private fun SettingsPreview(state: MonitorUiState, onBack: (() -> Unit)? = null, expanded: Boolean = false) {
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
