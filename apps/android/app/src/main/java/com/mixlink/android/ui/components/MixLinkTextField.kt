package com.mixlink.android.ui.components

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.tooling.preview.Preview
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.mixlink.android.ui.theme.MixLinkShapes
import com.mixlink.android.ui.theme.MixLinkTheme
import com.mixlink.android.ui.theme.MixLinkType

/**
 * Whether a text field should replace what it shows ([shown]) with the text it was given from
 * outside ([outside]).
 *
 * While the musician types, the outside text is the echo of earlier keys and can lag behind them,
 * so it is ignored until the field loses focus; otherwise a late echo would undo the last key. An
 * outside change while the field is not being edited (a restored name, a form filled by a
 * session) is shown.
 */
fun adoptOutsideText(outside: String, shown: String, focused: Boolean): Boolean = !focused && outside != shown

/**
 * What a field limited to [maxLength] characters keeps when an edit turns [shown] into [proposed].
 *
 * An edit that fits, or that does not make the text longer, is kept as it is, so a text that is
 * already too long (given from outside) can still be shortened. At the limit, further keys do
 * nothing. A paste that would pass the limit keeps its first characters up to it, one fewer when
 * the cut would split a character made of two UTF-16 units (an emoji, say).
 */
fun keptEdit(shown: String, proposed: String, maxLength: Int): String = when {
    proposed.length <= maxLength || proposed.length <= shown.length -> proposed
    shown.length >= maxLength -> shown
    else -> {
        val splitsPair = maxLength > 0 && Character.isHighSurrogate(proposed[maxLength - 1])
        proposed.substring(0, if (splitsPair) maxLength - 1 else maxLength)
    }
}

/** [proposed] as [keptEdit] keeps it, with the cursor kept inside the text. */
private fun keptEdit(shown: TextFieldValue, proposed: TextFieldValue, maxLength: Int): TextFieldValue {
    val text = keptEdit(shown.text, proposed.text, maxLength)
    return when (text) {
        proposed.text -> proposed
        shown.text -> shown
        else -> TextFieldValue(text, TextRange(proposed.selection.end.coerceAtMost(text.length)))
    }
}

/** How a text field's text is set: in the text face, or in the number face for addresses and ports. */
enum class FieldFace { TEXT, NUMBERS }

/**
 * A single-line text field in the MixLink look: a bordered box that turns amber while it is being
 * edited. [label] is what a screen reader calls it; a visible label goes above it as a [FieldLabel].
 *
 * It keeps what is typed itself and reports every change through [onValueChange], so a caller that
 * echoes the text back later (through a ViewModel's state) never moves the cursor or loses a key.
 * With [maxLength] it takes no more characters than that (see [keptEdit]).
 */
@Composable
fun MixLinkTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    modifier: Modifier = Modifier,
    placeholder: String = "",
    face: FieldFace = FieldFace.TEXT,
    large: Boolean = false,
    fill: Color = MixLinkTheme.colors.surface,
    keyboardType: KeyboardType = KeyboardType.Text,
    capitalization: KeyboardCapitalization = KeyboardCapitalization.None,
    imeAction: ImeAction = ImeAction.Done,
    onDone: () -> Unit = {},
    enabled: Boolean = true,
    maxLength: Int = Int.MAX_VALUE,
) {
    val colors = MixLinkTheme.colors
    var shown by remember { mutableStateOf(TextFieldValue(value, TextRange(value.length))) }
    var focused by remember { mutableStateOf(false) }
    LaunchedEffect(value, focused) {
        if (adoptOutsideText(value, shown.text, focused)) shown = TextFieldValue(value, TextRange(value.length))
    }
    val textStyle = fieldTextStyle(face, large).copy(color = if (enabled) colors.text else colors.textSecondary)
    val shape = if (large) MixLinkShapes.Card else MixLinkShapes.Control

    BasicTextField(
        value = shown,
        onValueChange = { proposed ->
            val next = keptEdit(shown, proposed, maxLength)
            val changed = next.text != shown.text
            shown = next
            if (changed) onValueChange(next.text)
        },
        modifier = modifier
            .heightIn(min = if (large) 52.dp else 48.dp)
            .onFocusChanged { focused = it.isFocused }
            .semantics { contentDescription = label },
        enabled = enabled,
        singleLine = true,
        textStyle = textStyle,
        keyboardOptions = KeyboardOptions(
            capitalization = capitalization,
            keyboardType = keyboardType,
            imeAction = imeAction,
        ),
        keyboardActions = KeyboardActions(onDone = { onDone() }),
        cursorBrush = SolidColor(colors.accent),
        decorationBox = { innerField ->
            Box(
                modifier = Modifier
                    .background(fill, shape)
                    .border(1.dp, if (focused) colors.accent else colors.border, shape)
                    .padding(horizontal = if (large) 16.dp else 14.dp),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (shown.text.isEmpty() && placeholder.isNotEmpty()) {
                    Text(text = placeholder, style = textStyle, color = colors.textSecondary)
                }
                innerField()
            }
        },
    )
}

@Composable
private fun fieldTextStyle(face: FieldFace, large: Boolean): TextStyle = when (face) {
    FieldFace.TEXT -> MaterialTheme.typography.bodyLarge.copy(fontSize = if (large) 18.sp else 16.sp)
    FieldFace.NUMBERS -> MixLinkType.Number.copy(fontWeight = FontWeight.Medium, fontSize = 15.sp)
}

/**
 * The visible label over a field: small capitals ("YOUR NAME") or, inside a card, plain secondary
 * text ("Audio port"). The field itself carries the name for a screen reader, so this is hidden
 * from it rather than read twice.
 */
@Composable
fun FieldLabel(text: String, modifier: Modifier = Modifier, caps: Boolean = true) {
    Text(
        text = text,
        modifier = modifier.clearAndSetSemantics {},
        style = if (caps) MixLinkType.Caps else MaterialTheme.typography.bodySmall,
        color = MixLinkTheme.colors.textSecondary,
    )
}

@Preview(showBackground = true, backgroundColor = 0xFF0D1117)
@Composable
private fun MixLinkTextFieldPreview() {
    MixLinkTheme {
        var name by remember { mutableStateOf("Carlos D") }
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            FieldLabel("YOUR NAME")
            MixLinkTextField(name, { name = it }, "Your name", Modifier.fillMaxWidth(), large = true)
            FieldLabel("Server address", caps = false)
            MixLinkTextField(
                value = "",
                onValueChange = {},
                label = "Server address",
                modifier = Modifier.fillMaxWidth(),
                placeholder = "192.168.1.34",
                face = FieldFace.NUMBERS,
                fill = MixLinkTheme.colors.ground,
            )
        }
    }
}
