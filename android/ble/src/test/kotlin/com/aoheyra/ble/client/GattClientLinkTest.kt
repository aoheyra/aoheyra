package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class GattClientLinkTest {
    private class FakeHandle : GattHandle {
        val calls = mutableListOf<String>()
        var closes = 0
        var mtuAccepted = true
        var descriptorAccepted = true
        var writeAccepted = true

        override fun requestMtu(mtu: Int): Boolean {
            calls.add("mtu:$mtu")
            return mtuAccepted
        }

        override fun discoverServices(): Boolean {
            calls.add("discover")
            return true
        }

        override fun setCharacteristicNotification(enabled: Boolean): Boolean {
            calls.add("notify:$enabled")
            return true
        }

        override fun writeDescriptor(value: ByteArray): Boolean {
            calls.add("cccd")
            return descriptorAccepted
        }

        override fun writeCharacteristic(value: ByteArray): Boolean {
            calls.add("write:${value.size}")
            return writeAccepted
        }

        override fun disconnect(): Boolean {
            calls.add("disconnect")
            return true
        }

        override fun close() {
            closes += 1
            calls.add("close")
        }

        override fun requestConnectionPriority(priority: Int): Boolean {
            calls.add("priority:$priority")
            return true
        }

        override fun setPreferredPhy(tx: Int, rx: Int, options: Int) {
            calls.add("phy:$tx:$rx")
        }
    }

    private class FakeEvents : GattClientEvents {
        val connected = mutableListOf<Pair<LinkId, Int>>()
        val ready = mutableListOf<LinkId>()
        val bytes = mutableListOf<Pair<LinkId, ByteArray>>()
        val closed = mutableListOf<Pair<LinkId, BleError>>()

        override fun onConnected(link: LinkId, mtu: Int) {
            connected.add(link to mtu)
        }

        override fun onMtuChanged(link: LinkId, mtu: Int) = Unit

        override fun onBytes(link: LinkId, bytes: ByteArray) {
            this.bytes.add(link to bytes.copyOf())
        }

        override fun onWriteReady(link: LinkId) {
            ready.add(link)
        }

        override fun onClosed(link: LinkId, status: BleError) {
            closed.add(link to status)
        }
    }

    private fun peer(): PeerKey = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

    private fun connectedLink(handle: FakeHandle, events: FakeEvents): GattClientLink {
        val link = GattClientLink(LinkId(1UL), peer(), handle, events)
        link.start()
        link.onConnected(0)
        link.onServicesDiscovered(0, rxFound = true, txFound = true)
        link.onMtuChanged(0, 517)
        link.onDescriptorWrite(0)
        return link
    }

    @Test
    fun `setup follows connect discover mtu subscribe order`() {
        val handle = FakeHandle()
        val events = FakeEvents()
        val link = connectedLink(handle, events)

        assertEquals(ClientPhase.CONNECTED, link.phase())
        assertEquals(517, link.mtu())
        assertEquals(
            listOf(
                "phy:2:2",
                "discover",
                "mtu:517",
                "notify:true",
                "cccd",
            ),
            handle.calls,
        )
        assertEquals(listOf(LinkId(1UL) to 517), events.connected)
    }

    @Test
    fun `a single write credit is returned only by the callback`() {
        val handle = FakeHandle()
        val events = FakeEvents()
        val link = connectedLink(handle, events)

        assertTrue(link.write(byteArrayOf(1, 2)).isSuccess)
        assertEquals(BleError.QueueFull, link.write(byteArrayOf(3)).exceptionOrNull())
        link.onCharacteristicWrite(0)
        assertEquals(listOf(LinkId(1UL)), events.ready)
        assertTrue(link.write(byteArrayOf(4)).isSuccess)
    }

    @Test
    fun `failed mtu request still subscribes with the default`() {
        val handle = FakeHandle().apply { mtuAccepted = false }
        val events = FakeEvents()
        val link = GattClientLink(LinkId(1UL), peer(), handle, events)
        link.start()
        link.onConnected(0)
        link.onServicesDiscovered(0, rxFound = true, txFound = true)

        assertEquals(23, link.mtu())
        assertEquals(listOf("notify:true", "cccd"), handle.calls.takeLast(2))
        link.onDescriptorWrite(0)
        assertEquals(listOf(LinkId(1UL) to 23), events.connected)
        assertEquals(23, link.mtu())
    }

    @Test
    fun `disconnect closes the handle exactly once`() {
        val handle = FakeHandle()
        val events = FakeEvents()
        val link = connectedLink(handle, events)

        link.disconnect()
        link.onDisconnected(133)
        link.onDisconnected(0)

        assertEquals(1, handle.closes)
        assertEquals(1, events.closed.size)
        assertEquals(BleError.Gatt(Stage.CONNECT, 133), events.closed[0].second)
    }

    @Test
    fun `100 connect and disconnect cycles leak no handles`() {
        var closes = 0
        repeat(100) { index ->
            val handle = FakeHandle()
            val link = connectedLink(handle, FakeEvents())
            assertTrue(link.write(byteArrayOf(index.toByte())).isSuccess)
            link.disconnect()
            link.onDisconnected(0)
            closes += handle.closes
        }
        assertEquals(100, closes)
    }

    @Test
    fun `sync priority is raised and lowered only while connected`() {
        val handle = FakeHandle()
        val link = connectedLink(handle, FakeEvents())
        link.setSyncActive(true)
        link.setSyncActive(false)
        assertTrue(handle.calls.contains("priority:1"))
        assertTrue(handle.calls.contains("priority:0"))
        link.disconnect()
        link.onDisconnected(0)
        val count = handle.calls.count { it.startsWith("priority:") }
        link.setSyncActive(true)
        assertEquals(count, handle.calls.count { it.startsWith("priority:") })
        assertFalse(link.write(byteArrayOf(1)).isSuccess)
    }
}
