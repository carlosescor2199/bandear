package com.bandear.android

import org.junit.Assert.assertEquals
import org.junit.Test

class ControlStatusTest {
    @Test
    fun connectedAfterAcknowledgementReportsServerOwnership() {
        assertEquals(
            "connected - mix applied on the server",
            controlStatusLabel(ControlState.CONNECTED, remoteMixAcknowledged = true),
        )
    }

    @Test
    fun connectedWithoutAcknowledgementWaitsForTheServer() {
        assertEquals(
            "connected - waiting for the server to take over",
            controlStatusLabel(ControlState.CONNECTED, remoteMixAcknowledged = false),
        )
    }

    @Test
    fun lostChannelAfterAcknowledgementKeepsServerOwnership() {
        val expected = "channel lost - mix held at the server's last setting"

        assertEquals(expected, controlStatusLabel(ControlState.UNAVAILABLE, remoteMixAcknowledged = true))
        assertEquals(expected, controlStatusLabel(ControlState.CLOSED, remoteMixAcknowledged = true))
    }

    @Test
    fun lostChannelWithoutAcknowledgementFallsBackToLocalProcessing() {
        val expected = "unavailable - mixing on this device"

        assertEquals(expected, controlStatusLabel(ControlState.UNAVAILABLE, remoteMixAcknowledged = false))
        assertEquals(expected, controlStatusLabel(ControlState.CLOSED, remoteMixAcknowledged = false))
    }

    @Test
    fun reconnectingAfterAcknowledgementKeepsServerOwnership() {
        assertEquals(
            "reconnecting - mix held at the server",
            controlStatusLabel(ControlState.RECONNECTING, remoteMixAcknowledged = true),
        )
    }

    @Test
    fun reconnectingWithoutAcknowledgementMixesOnThisDevice() {
        assertEquals(
            "reconnecting - mixing on this device",
            controlStatusLabel(ControlState.RECONNECTING, remoteMixAcknowledged = false),
        )
    }

    @Test
    fun idleAndConnectingNeverClaimOwnership() {
        assertEquals("not connected", controlStatusLabel(ControlState.IDLE, remoteMixAcknowledged = false))
        assertEquals("not connected", controlStatusLabel(ControlState.IDLE, remoteMixAcknowledged = true))
        assertEquals("connecting...", controlStatusLabel(ControlState.CONNECTING, remoteMixAcknowledged = false))
    }
}
