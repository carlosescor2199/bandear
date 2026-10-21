package com.bandear.android

import android.content.Context
import android.content.SharedPreferences
import java.util.UUID

/** The longest client id the server keeps; a UUID string (36 characters) fits. */
internal const val MAX_CLIENT_ID_LENGTH = 64

/**
 * Returns [raw] when it is a client id the server keeps (1 to [MAX_CLIENT_ID_LENGTH] ASCII
 * letters, digits and hyphens), otherwise null. The server applies the same rule and reads any
 * other value as no id at all.
 */
internal fun validClientId(raw: String?): String? =
    raw?.takeIf { id ->
        id.length in 1..MAX_CLIENT_ID_LENGTH &&
            id.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '-' }
    }

/** A new random client id. A UUID is not guessable on the LAN and passes [validClientId]. */
internal fun newClientId(): String = UUID.randomUUID().toString()

/**
 * Returns the [stored] id when it is valid. Otherwise (first run, or a value damaged on the
 * device) makes a new one with [generate], hands it to [save] and returns it.
 */
internal fun resolveClientId(
    stored: String?,
    generate: () -> String = ::newClientId,
    save: (String) -> Unit,
): String {
    validClientId(stored)?.let { return it }
    val created = generate()
    save(created)
    return created
}

/**
 * This installation's client id, generated on first use and kept in its own `SharedPreferences`
 * file from then on.
 *
 * The phone announces it in every registration. The server keys musicians by IP, and a phone that
 * comes back from a new IP (DHCP after a Wi-Fi blip) would otherwise appear as a second musician;
 * with the id the server moves the musician's row, mix included, to the new address. It names this
 * installation only: it is random, carries nothing about the device or the musician, and is never
 * shown.
 */
class ClientIdentity(context: Context) {
    private val preferences: SharedPreferences =
        context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    val clientId: String by lazy {
        resolveClientId(preferences.getString(KEY_CLIENT_ID, null)) { created ->
            preferences.edit().putString(KEY_CLIENT_ID, created).apply()
        }
    }

    private companion object {
        const val PREFERENCES_NAME = "bandear_identity"
        const val KEY_CLIENT_ID = "client_id"
    }
}
