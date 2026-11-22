package com.aoheyra.ble.server

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GattServerHostTest {
    private class TestEvents : ServerEvents {
        var acceptedLink: LinkId = LinkId(1UL)
        val accepted = mutableListOf<Pair<PeerKey, Int>>()
        val bytes = mutableListOf<Pair<LinkId, ByteArray>>()
        val ready = mutableListOf<LinkId>()
        val closed = mutableListOf<Pair<LinkId, BleError>>()
        val misbehaved = mutableListOf<PeerKey>()
        val registrationFailures = mutableListOf<Pair<PeerKey, BleError>>()

        override fun onAccepted(peer: PeerKey, mtu: Int): LinkId {
            accepted.add(peer to mtu)
            return acceptedLink
        }

        override fun onBytes(link: LinkId, bytes: ByteArray) {
            this.bytes.add(link to bytes.copyOf())
        }

        override fun onWriteReady(link: LinkId) {
            ready.add(link)
        }

        override fun onClosed(link: LinkId, status: BleError) {
            closed.add(link to status)
        }

        override fun onPeerMisbehaved(peer: PeerKey) {
            misbehaved.add(peer)
        }

        override fun onRegistrationFailed(peer: PeerKey, error: BleError) {
            registrationFailures.add(peer to error)
        }
    }

    private fun peer(): PeerKey = PeerKey.from(ByteArray(PeerKey.LENGTH) { 0x11 }).getOrThrow()

    @Test
    fun `cccd setup registers the Rust allocated link before RX writes`() {
        val events = TestEvents()
        val host = GattServerHost(events, LinkRegistry())
        val peer = peer()

        assertTrue(host.onCentralConnected(peer).isSuccess)
        assertTrue(host.onMtuChanged(peer, 517).isSuccess)
        assertTrue(host.onCccdWritten(peer, 1).isSuccess)
        assertEquals(listOf(peer to 517), events.accepted)

        val frame = byteArrayOf(9, 8, 7)
        assertTrue(host.onRxWrite(peer, frame, false, 0).isSuccess)
        assertTrue(events.bytes[0].first == LinkId(1UL))
        assertTrue(events.bytes[0].second.contentEquals(frame))
    }

    @Test
    fun `RX write before setup is a protocol violation`() {
        val events = TestEvents()
        val host = GattServerHost(events, LinkRegistry())
        val peer = peer()
        host.onCentralConnected(peer)

        assertTrue(host.onRxWrite(peer, byteArrayOf(1), false, 0).isFailure)
        assertEquals(listOf(peer), events.misbehaved)
        assertTrue(events.bytes.isEmpty())
    }

    @Test
    fun `notifications are paced and raise write ready after each callback`() {
        val events = TestEvents()
        val host = GattServerHost(events, LinkRegistry())
        val peer = peer()
        host.onCentralConnected(peer)
        host.onCccdWritten(peer, 1)

        val first = host.enqueueNotification(LinkId(1UL), byteArrayOf(1))
        assertTrue(first is NotificationSend.Send)
        assertTrue(first.bytes.contentEquals(byteArrayOf(1)))
        assertTrue(host.enqueueNotification(LinkId(1UL), byteArrayOf(2)).isSuccess)
        assertTrue(events.ready.isEmpty())

        val next = host.onNotificationSent(peer, true, 0)
        assertTrue(next is NotificationSend.Send)
        assertTrue(next.bytes.contentEquals(byteArrayOf(2)))
        assertEquals(listOf(LinkId(1UL)), events.ready)
        assertEquals(NotificationSend.None, host.onNotificationSent(peer, true, 0))
    }

    @Test
    fun `disconnect closes the link exactly once`() {
        val events = TestEvents()
        val host = GattServerHost(events, LinkRegistry())
        val peer = peer()
        host.onCentralConnected(peer)
        host.onCccdWritten(peer, 1)

        assertTrue(host.onCentralDisconnected(peer, BleError.Requested).isSuccess)
        assertEquals(listOf(LinkId(1UL) to BleError.Requested), events.closed)
        assertTrue(host.onCentralDisconnected(peer, BleError.Requested).isFailure)
        assertEquals(1, events.closed.size)
    }

    @Test
    fun `registry failure cancels the inbound central without a link event`() {
        val events = TestEvents()
        val registry = LinkRegistry()
        val host = GattServerHost(events, registry)
        val existing = peer()
        val rejected = PeerKey.from(ByteArray(PeerKey.LENGTH) { 0x22 }).getOrThrow()
        registry.registerCentral(LinkId(9UL), existing)
        events.acceptedLink = LinkId(9UL)

        assertTrue(host.onCentralConnected(rejected).isSuccess)
        assertTrue(host.onCccdWritten(rejected, 1).isFailure)
        assertEquals(1, events.registrationFailures.size)
        assertTrue(events.closed.isEmpty())
    }
}
