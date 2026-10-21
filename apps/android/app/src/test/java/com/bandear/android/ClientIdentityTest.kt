package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.UUID

/**
 * Pure JVM coverage for the client id: what counts as a valid one (the same rule the server
 * applies), how a new one is made, and when a stored one is kept or replaced.
 */
class ClientIdentityTest {
    @Test
    fun aNewClientIdIsARandomUuidTheServerAccepts() {
        val first = newClientId()
        val second = newClientId()

        assertEquals(first, UUID.fromString(first).toString())
        assertEquals(first, validClientId(first))
        assertNotEquals(first, second)
    }

    @Test
    fun anIdOfUpTo64LettersDigitsAndHyphensIsValid() {
        val longest = "A-9".repeat(21) + "z"
        assertEquals(MAX_CLIENT_ID_LENGTH, longest.length)

        for (id in listOf("a", "phone-1", "ABC-def-123", longest)) {
            assertEquals(id, validClientId(id))
        }
    }

    @Test
    fun anythingElseIsNotAClientId() {
        val invalid = listOf("", "has space", "under_score", "slash/id", "acentuación", "a".repeat(MAX_CLIENT_ID_LENGTH + 1))

        for (id in invalid) {
            assertNull("'$id' must not be a client id", validClientId(id))
        }
        assertNull(validClientId(null))
    }

    @Test
    fun aStoredValidIdIsKeptAndNothingIsSaved() {
        val saved = mutableListOf<String>()

        val id = resolveClientId(stored = "phone-1", generate = { "unused" }, save = { saved += it })

        assertEquals("phone-1", id)
        assertTrue("nothing new is stored", saved.isEmpty())
    }

    @Test
    fun withNoStoredIdANewOneIsGeneratedAndSaved() {
        val saved = mutableListOf<String>()

        val id = resolveClientId(stored = null, generate = { "fresh-id" }, save = { saved += it })

        assertEquals("fresh-id", id)
        assertEquals(listOf("fresh-id"), saved)
    }

    @Test
    fun aCorruptedStoredIdIsReplacedAndSaved() {
        val saved = mutableListOf<String>()

        val id = resolveClientId(stored = "not valid!", generate = { "fresh-id" }, save = { saved += it })

        assertEquals("fresh-id", id)
        assertEquals(listOf("fresh-id"), saved)
    }
}
