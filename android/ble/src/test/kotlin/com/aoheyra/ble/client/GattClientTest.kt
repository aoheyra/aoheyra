package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.Clock
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class GattClientTest {
    private class TestClock(var now: Long = 1_000L) : Clock {
        override fun nowMillis(): Long = now
    }

    private class CountingHandle : GattHandle {
        var closes = 0
        override fun requestMtu(mtu: Int): Boolean = true
        override fun discoverServices(): Boolean = true
        override fun setCharacteristicNotification(enabled: Boolean): Boolean = true
        override fun writeDescriptor(value: ByteArray): Boolean = true
        override fun writeCharacteristic(value: ByteArray): Boolean = true
        override fun disconnect(): Boolean = true
        override fun close() {
            closes += 1
        }

        override fun requestConnectionPriority(priority: Int): Boolean = true
        override fun setPreferredPhy(tx: Int, rx: Int, options: Int) = Unit
    }

    private class TestEvents : GattClientEvents {
        val closed = mutableListOf<Pair<LinkId, BleError>>()
        override fun onConnected(link: LinkId, mtu: Int) = Unit
        override fun onMtuChanged(link: LinkId, mtu: Int) = Unit
        override fun onBytes(link: LinkId, bytes: ByteArray) = Unit
        override fun onWriteReady(link: LinkId) = Unit
        override fun onClosed(link: LinkId, status: BleError) {
            closed.add(link to status)
        }
    }

    @Test
    fun `status 133 removes the link and blocks an immediate reconnect`() {
        val clock = TestClock()
        val handle = CountingHandle()
        val factory = object : GattHandleFactory {
            override fun open(linkId: LinkId): GattHandle = handle
        }
        val events = TestEvents()
        val client = GattClient(factory, events, LinkRegistry(), ConnectBackoff(clock))
        val link = LinkId(4UL)
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

        assertTrue(client.connect(link, peer).isSuccess)
        client.onConnected(link, ConnectBackoff.STATUS_133)

        assertEquals(1, handle.closes)
        assertEquals(0, client.size())
        assertEquals(1, events.closed.size)
        assertEquals(
            BleError.Backoff,
            client.connect(link, peer).exceptionOrNull(),
        )

        clock.now += ConnectBackoff.COOLDOWN_MS
        assertTrue(client.connect(link, peer).isSuccess)
    }

    @Test
    fun `force close reports the explicit radio-state cause once`() {
        val handle = CountingHandle()
        val factory = object : GattHandleFactory {
            override fun open(linkId: LinkId): GattHandle = handle
        }
        val events = TestEvents()
        val client = GattClient(factory, events, LinkRegistry(), ConnectBackoff(TestClock()))
        val link = LinkId(6UL)
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

        assertTrue(client.connect(link, peer).isSuccess)
        assertTrue(client.forceClose(link, BleError.RadioUnavailable).isSuccess)
        assertTrue(client.forceClose(link, BleError.RadioUnavailable).isFailure)

        assertEquals(1, handle.closes)
        assertEquals(listOf(link to BleError.RadioUnavailable), events.closed)
    }

    @Test
    fun `missing device is a typed refusal and leaves no link`() {
        val client = GattClient(
            object : GattHandleFactory {
                override fun open(linkId: LinkId): GattHandle? = null
            },
            TestEvents(),
            LinkRegistry(),
            ConnectBackoff(TestClock()),
        )
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()
        assertEquals(
            BleError.RadioUnavailable,
            client.connect(LinkId(5UL), peer).exceptionOrNull(),
        )
        assertEquals(0, client.size())
    }
}
