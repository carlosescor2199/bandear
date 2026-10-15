package com.mixlink.android.ui.screens

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
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.key
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.components.FieldLabel
import com.mixlink.android.ui.components.LineIcons
import com.mixlink.android.ui.components.MixLinkTextField
import com.mixlink.android.ui.components.StatusBanner
import com.mixlink.android.ui.state.ConnectionUiState
import com.mixlink.android.ui.state.ServerEndpoint
import com.mixlink.android.ui.theme.ColorToken
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme
import com.mixlink.android.ui.theme.MixLinkType

/**
 * The first screen, shown whenever no session runs: the musician's name and the servers found on
 * the network, each joined with one tap. The manual address is one tap away, in Settings.
 *
 * [connection] is only read for why the last Connect was refused.
 */
@Composable
fun ConnectScreen(
    musicianName: String,
    servers: List<ServerEndpoint>,
    connection: ConnectionUiState,
    onNameChange: (String) -> Unit,
    onConnect: (ServerEndpoint) -> Unit,
    onManualAddress: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MixLinkTheme.colors
    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground)
            .padding(horizontal = 20.dp),
    ) {
        Column(
            modifier = Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState())
                .padding(top = 28.dp, bottom = 20.dp),
            verticalArrangement = Arrangement.spacedBy(28.dp),
        ) {
            BrandHeader()
            NameSection(musicianName, onNameChange)
            ServersSection(servers, connection.problem, onConnect)
        }
        ManualAddressLink(onManualAddress, Modifier.padding(bottom = 24.dp))
    }
}

@Composable
private fun BrandHeader() {
    val colors = MixLinkTheme.colors
    Row(
        modifier = Modifier.padding(top = 8.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier
                .size(14.dp)
                .background(colors.accent, MixLinkShapes.Tag),
        )
        Text(
            text = "MixLink",
            style = MaterialTheme.typography.titleLarge.copy(fontSize = 26.sp, letterSpacing = (-0.01).em),
            color = colors.text,
        )
        Text(
            text = "MONITOR",
            modifier = Modifier.padding(top = 6.dp),
            style = MixLinkType.Caps.copy(letterSpacing = 0.18.em),
            color = colors.textSecondary,
        )
    }
}

@Composable
private fun NameSection(musicianName: String, onNameChange: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
        FieldLabel("YOUR NAME")
        MixLinkTextField(
            value = musicianName,
            onValueChange = onNameChange,
            label = "Your name",
            modifier = Modifier.fillMaxWidth(),
            placeholder = "Your name",
            large = true,
            capitalization = KeyboardCapitalization.Words,
        )
        Text(
            text = "The engineer sees this name on the desktop.",
            style = MaterialTheme.typography.bodySmall,
            color = MixLinkTheme.colors.textSecondary,
        )
    }
}

@Composable
private fun ServersSection(servers: List<ServerEndpoint>, problem: String?, onConnect: (ServerEndpoint) -> Unit) {
    val colors = MixLinkTheme.colors
    Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
        Row(
            modifier = Modifier.fillMaxWidth(),
            horizontalArrangement = Arrangement.SpaceBetween,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("SERVERS ON THIS NETWORK", style = MixLinkType.Caps, color = colors.textSecondary)
            Searching()
        }
        if (problem != null) {
            StatusBanner(
                token = ColorToken.RED,
                title = "Could not connect",
                detail = problem,
                modifier = Modifier.fillMaxWidth(),
            )
        }
        if (servers.isEmpty()) {
            NoServersYet()
        } else {
            servers.forEach { server ->
                key(server) { ServerCard(server, onConnect = { onConnect(server) }) }
            }
        }
        Text(
            text = "Servers appear here on their own. Tap one to join; the phone stays connected and " +
                "reconnects on its own if the Wi-Fi drops.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

/** Discovery keeps listening while this screen shows, found servers or not. */
@Composable
private fun Searching() {
    val colors = MixLinkTheme.colors
    Row(
        modifier = Modifier.semantics(mergeDescendants = true) {},
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        CircularProgressIndicator(
            modifier = Modifier.size(14.dp),
            color = colors.textSecondary,
            strokeWidth = 2.dp,
            trackColor = Color.Transparent,
        )
        Text("Searching", style = MaterialTheme.typography.bodySmall, color = colors.textSecondary)
    }
}

/** One found server: the whole card is its single Connect action. */
@Composable
private fun ServerCard(server: ServerEndpoint, onConnect: () -> Unit) {
    val colors = MixLinkTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 76.dp)
            .clip(MixLinkShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .clickable(role = Role.Button, onClickLabel = "Connect", onClick = onConnect)
            .padding(horizontal = 16.dp, vertical = 14.dp),
        horizontalArrangement = Arrangement.spacedBy(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            modifier = Modifier
                .size(44.dp)
                .background(colors.surfaceRaised, MixLinkShapes.Card),
            contentAlignment = Alignment.Center,
        ) {
            Icon(LineIcons.Server, contentDescription = null, tint = colors.accent, modifier = Modifier.size(22.dp))
        }
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text("MixLink server", style = MaterialTheme.typography.titleSmall, color = colors.text)
            Text(
                text = server.label,
                style = MixLinkType.NumberSmall.copy(fontSize = 13.sp),
                color = colors.textSecondary,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Box(
            modifier = Modifier
                .background(colors.accent, MixLinkShapes.Control)
                .padding(horizontal = 16.dp, vertical = 12.dp),
        ) {
            Text("Connect", style = MaterialTheme.typography.labelLarge.copy(fontSize = 15.sp), color = colors.onAccent)
        }
    }
}

@Composable
private fun NoServersYet() {
    val colors = MixLinkTheme.colors
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 76.dp)
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .padding(horizontal = 16.dp, vertical = 14.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(2.dp, Alignment.CenterVertically),
    ) {
        Text("No servers found yet", style = MaterialTheme.typography.titleSmall, color = colors.text)
        Text(
            text = "Check that this phone is on the same Wi-Fi as the MixLink desktop.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

@Composable
private fun ManualAddressLink(onClick: () -> Unit, modifier: Modifier = Modifier) {
    val colors = MixLinkTheme.colors
    Row(
        modifier = modifier
            .fillMaxWidth()
            .heightIn(min = 48.dp)
            .clip(MixLinkShapes.Card)
            .border(1.dp, colors.border, MixLinkShapes.Card)
            .clickable(role = Role.Button, onClick = onClick),
        horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.CenterHorizontally),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(LineIcons.Form, contentDescription = null, tint = colors.text, modifier = Modifier.size(18.dp))
        Text(
            text = "Enter the server address manually",
            style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
            color = colors.text,
        )
    }
}

@Preview(
    name = "Connect · server found",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenFoundPreview() {
    ConnectPreview(PreviewSamples.notConnected.musicianName, PreviewSamples.notConnected.servers, ConnectionUiState())
}

@Preview(
    name = "Connect · searching",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenSearchingPreview() {
    ConnectPreview(PreviewSamples.searching.musicianName, PreviewSamples.searching.servers, ConnectionUiState())
}

@Preview(
    name = "Connect · refused",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun ConnectScreenRefusedPreview() {
    val state = PreviewSamples.refused
    ConnectPreview(state.musicianName, state.servers, state.connection)
}

@Composable
private fun ConnectPreview(name: String, servers: List<ServerEndpoint>, connection: ConnectionUiState) {
    MixLinkTheme {
        ConnectScreen(
            musicianName = name,
            servers = servers,
            connection = connection,
            onNameChange = {},
            onConnect = {},
            onManualAddress = {},
        )
    }
}
