package com.mixlink.android.ui.screens

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.MenuDefaults
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.components.DangerButton
import com.mixlink.android.ui.components.FieldLabel
import com.mixlink.android.ui.components.LineIcons
import com.mixlink.android.ui.components.MixLinkTextField
import com.mixlink.android.ui.components.PrimaryButton
import com.mixlink.android.ui.components.SecondaryButton
import com.mixlink.android.ui.state.SavedMixUiState
import com.mixlink.android.ui.state.SavedMixesUiState
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme
import com.mixlink.android.ui.theme.MixLinkType

/**
 * The saved mixes: save the current mix under a name, bring one back with Recall. Rename and
 * delete sit behind each mix's menu, and delete asks first, so a stray tap on stage never loses a
 * mix. With nothing saved the list says so, and only then.
 */
@Composable
fun SavedMixesScreen(
    savedMixes: SavedMixesUiState,
    onSave: (String) -> Boolean,
    onRecall: (String) -> Unit,
    onRename: (from: String, to: String) -> Unit,
    onDelete: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    val colors = MixLinkTheme.colors
    var renaming by rememberSaveable { mutableStateOf<String?>(null) }
    var deleting by rememberSaveable { mutableStateOf<String?>(null) }
    val mixes = remember(savedMixes) { savedMixes.mixes }

    Column(
        modifier = modifier
            .fillMaxSize()
            .background(colors.ground)
            .verticalScroll(rememberScrollState()),
    ) {
        Column(
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 8.dp),
            verticalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            Text("Saved mixes", style = MaterialTheme.typography.titleLarge, color = colors.text)
            Text(
                text = "Store your mix for a song or a set and bring it back with one tap.",
                style = MaterialTheme.typography.bodyMedium,
                color = colors.textSecondary,
            )
        }
        SaveCard(onSave, Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp))
        val notice = savedMixes.notice
        if (notice != null) {
            Text(
                text = notice,
                modifier = Modifier
                    .padding(start = 16.dp, end = 16.dp, top = 10.dp)
                    .semantics { liveRegion = LiveRegionMode.Polite },
                style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                color = colors.text,
            )
        }
        Text(
            text = savedMixesTitle(mixes.size),
            modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 20.dp, bottom = 8.dp),
            style = MixLinkType.Caps,
            color = colors.textSecondary,
        )
        val emptyMessage = savedMixes.emptyMessage
        if (emptyMessage != null) {
            NoMixesYet(emptyMessage)
        } else {
            Column(
                modifier = Modifier.padding(horizontal = 16.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                mixes.forEach { mix ->
                    key(mix.name) {
                        SavedMixRow(
                            mix = mix,
                            onRecall = { onRecall(mix.name) },
                            onRename = { renaming = mix.name },
                            onDelete = { deleting = mix.name },
                        )
                    }
                }
            }
            Text(
                text = "Rename and delete are under ⋮, so a stray tap on stage never deletes a mix.",
                modifier = Modifier.padding(start = 16.dp, end = 16.dp, top = 12.dp),
                style = MaterialTheme.typography.bodySmall,
                color = colors.textSecondary,
            )
        }
        Spacer(Modifier.height(16.dp))
    }

    renaming?.let { from ->
        RenameDialog(
            from = from,
            onRename = { to ->
                renaming = null
                onRename(from, to)
            },
            onDismiss = { renaming = null },
        )
    }
    deleting?.let { name ->
        DeleteDialog(
            name = name,
            onDelete = {
                deleting = null
                onDelete(name)
            },
            onDismiss = { deleting = null },
        )
    }
}

/**
 * The name field and Save. [onSave] says whether the mix was saved: only then is the field
 * cleared, so a refused name stays to be corrected.
 */
@Composable
private fun SaveCard(onSave: (String) -> Boolean, modifier: Modifier = Modifier) {
    val colors = MixLinkTheme.colors
    val focusManager = LocalFocusManager.current
    var name by rememberSaveable { mutableStateOf("") }

    fun save() {
        val saved = onSave(name)
        // Leave the field first, so it shows the cleared text instead of keeping what was typed.
        if (saved) focusManager.clearFocus()
        name = nameAfterSave(name, saved)
    }

    Column(
        modifier = modifier
            .fillMaxWidth()
            .clip(MixLinkShapes.Panel)
            .background(colors.surface)
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .padding(14.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        FieldLabel("SAVE THE CURRENT MIX")
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            MixLinkTextField(
                value = name,
                onValueChange = { name = it },
                label = "Name for the current mix",
                modifier = Modifier.weight(1f),
                placeholder = "Name, e.g. Ballads",
                fill = colors.ground,
                capitalization = KeyboardCapitalization.Sentences,
                onDone = ::save,
            )
            PrimaryButton("Save", onClick = ::save)
        }
    }
}

@Composable
private fun SavedMixRow(mix: SavedMixUiState, onRecall: () -> Unit, onRename: () -> Unit, onDelete: () -> Unit) {
    val colors = MixLinkTheme.colors
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = 68.dp)
            .clip(MixLinkShapes.Panel)
            .background(colors.surface)
            .border(1.dp, if (mix.active) colors.accent else colors.border, MixLinkShapes.Panel)
            .padding(start = 14.dp, end = 10.dp, top = 10.dp, bottom = 10.dp),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Text(
                text = mix.name,
                style = MaterialTheme.typography.bodyLarge.copy(fontSize = 16.sp, fontWeight = FontWeight.SemiBold),
                color = colors.text,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            val detail = savedMixDetail(mix.active)
            if (detail != null) {
                Text(
                    text = detail,
                    style = MaterialTheme.typography.bodySmall.copy(fontWeight = FontWeight.Medium),
                    color = colors.accent,
                )
            }
        }
        SecondaryButton(
            text = "Recall",
            onClick = onRecall,
            modifier = Modifier.semantics { contentDescription = "Recall ${mix.name}" },
        )
        MixMenu(name = mix.name, onRename = onRename, onDelete = onDelete)
    }
}

/** The ⋮ key of a saved mix, with Rename and Delete; neither acts before its dialog is confirmed. */
@Composable
private fun MixMenu(name: String, onRename: () -> Unit, onDelete: () -> Unit) {
    val colors = MixLinkTheme.colors
    var open by remember { mutableStateOf(false) }
    Box {
        Box(
            modifier = Modifier
                .size(44.dp)
                .clip(MixLinkShapes.Control)
                .clickable(role = Role.Button) { open = true }
                .semantics { contentDescription = "More actions for $name" },
            contentAlignment = Alignment.Center,
        ) {
            Icon(LineIcons.MoreActions, contentDescription = null, tint = colors.textSecondary, modifier = Modifier.size(20.dp))
        }
        DropdownMenu(
            expanded = open,
            onDismissRequest = { open = false },
            containerColor = colors.surfaceRaised,
            tonalElevation = 0.dp,
            border = BorderStroke(1.dp, colors.border),
        ) {
            DropdownMenuItem(
                text = { Text("Rename") },
                onClick = {
                    open = false
                    onRename()
                },
                colors = MenuDefaults.itemColors(textColor = colors.text),
            )
            DropdownMenuItem(
                text = { Text("Delete") },
                onClick = {
                    open = false
                    onDelete()
                },
                colors = MenuDefaults.itemColors(textColor = colors.red),
            )
        }
    }
}

/** Asks for the new name; [onRename] gets it trimmed, and only once it can be confirmed. */
@Composable
private fun RenameDialog(from: String, onRename: (String) -> Unit, onDismiss: () -> Unit) {
    val colors = MixLinkTheme.colors
    var to by rememberSaveable(from) { mutableStateOf(from) }

    fun confirm() {
        renameTarget(from, to)?.let(onRename)
    }

    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = {
            PrimaryButton("Rename", onClick = ::confirm, enabled = canRename(from, to))
        },
        dismissButton = { SecondaryButton("Cancel", onClick = onDismiss) },
        title = { Text("Rename \"$from\"") },
        text = {
            MixLinkTextField(
                value = to,
                onValueChange = { to = it },
                label = "New name",
                modifier = Modifier.fillMaxWidth(),
                fill = colors.ground,
                capitalization = KeyboardCapitalization.Sentences,
                onDone = ::confirm,
            )
        },
        containerColor = colors.surface,
        titleContentColor = colors.text,
        textContentColor = colors.text,
        tonalElevation = 0.dp,
    )
}

@Composable
private fun DeleteDialog(name: String, onDelete: () -> Unit, onDismiss: () -> Unit) {
    val colors = MixLinkTheme.colors
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { DangerButton("Delete", onClick = onDelete) },
        dismissButton = { SecondaryButton("Cancel", onClick = onDismiss) },
        title = { Text("Delete \"$name\"?") },
        text = { Text("The mix is removed from this phone. This cannot be undone.") },
        containerColor = colors.surface,
        titleContentColor = colors.text,
        textContentColor = colors.textSecondary,
        tonalElevation = 0.dp,
    )
}

@Composable
private fun NoMixesYet(message: String) {
    val colors = MixLinkTheme.colors
    Column(
        modifier = Modifier
            .padding(horizontal = 16.dp)
            .fillMaxWidth()
            .border(1.dp, colors.border, MixLinkShapes.Panel)
            .padding(16.dp)
            .semantics(mergeDescendants = true) {},
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Text(message, style = MaterialTheme.typography.titleSmall, color = colors.text)
        Text(
            text = "Save the current mix above, then bring it back here with one tap.",
            style = MaterialTheme.typography.bodySmall,
            color = colors.textSecondary,
        )
    }
}

@Preview(
    name = "Saved mixes · three mixes",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesFilledPreview() {
    MixLinkTheme {
        SavedMixesScreen(PreviewSamples.connected.savedMixes, { false }, {}, { _, _ -> }, {})
    }
}

@Preview(
    name = "Saved mixes · empty",
    widthDp = PREVIEW_WIDTH_DP,
    heightDp = PREVIEW_HEIGHT_DP,
    showBackground = true,
    backgroundColor = PREVIEW_BACKGROUND,
)
@Composable
private fun SavedMixesEmptyPreview() {
    MixLinkTheme {
        SavedMixesScreen(PreviewSamples.emptyMixes.savedMixes, { false }, {}, { _, _ -> }, {})
    }
}
