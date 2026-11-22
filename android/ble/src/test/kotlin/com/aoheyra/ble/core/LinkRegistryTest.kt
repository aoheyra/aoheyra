package com.aoheyra.ble.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LinkRegistryTest {
    @Test
    fun `registers both GATT roles with Rust allocated ids`() {
        val registry = LinkRegistry()
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

        assertTrue(registry.registerCentral(LinkId(1UL), peer).isSuccess)
        assertTrue(registry.registerPeripheral(LinkId(2UL), peer).isSuccess)

        assertEquals(LinkRole.CENTRAL, registry.get(LinkId(1UL))?.role)
        assertEquals(LinkRole.PERIPHERAL, registry.get(LinkId(2UL))?.role)
        assertEquals(2, registry.size())
    }

    @Test
    fun `rejects reserved duplicate and unknown links`() {
        val registry = LinkRegistry()
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

        assertEquals(
            BleError.InvalidLink,
            registry.registerCentral(LinkId.INVALID, peer).exceptionOrNull(),
        )
        assertTrue(registry.registerCentral(LinkId(7UL), peer).isSuccess)
        assertEquals(
            BleError.LinkDuplicate,
            registry.registerPeripheral(LinkId(7UL), peer).exceptionOrNull(),
        )
        assertEquals(BleError.LinkUnknown, registry.remove(LinkId(8UL)).exceptionOrNull())
    }

    @Test
    fun `remove is once only so late callbacks find no link`() {
        val registry = LinkRegistry()
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()
        registry.registerCentral(LinkId(3UL), peer)

        assertTrue(registry.remove(LinkId(3UL)).isSuccess)
        assertFalse(registry.contains(LinkId(3UL)))
        assertEquals(BleError.LinkUnknown, registry.remove(LinkId(3UL)).exceptionOrNull())
    }

    @Test
    fun `registry has a fixed safety bound`() {
        val registry = LinkRegistry()
        val peer = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

        repeat(LinkRegistry.MAX_TRACKED_LINKS) { index ->
            assertTrue(registry.registerCentral(LinkId(index.toULong() + 1UL), peer).isSuccess)
        }
        assertEquals(
            BleError.TooManyLinks,
            registry.registerPeripheral(
                LinkId(LinkRegistry.MAX_TRACKED_LINKS.toULong() + 1UL),
                peer,
            ).exceptionOrNull(),
        )
    }

    @Test
    fun `peer keys are validated and copied`() {
        assertEquals(
            BleError.InvalidPeer,
            PeerKey.from(ByteArray(15)).exceptionOrNull(),
        )
        val input = ByteArray(PeerKey.LENGTH)
        input[0] = 0x0A
        val key = PeerKey.from(input).getOrThrow()
        input[0] = 0xFF.toByte()
        assertEquals(0x0A.toByte(), key.bytes[0])
        assertEquals(BleError.InvalidPeer, PeerKey.fromAddress(ByteArray(5)).exceptionOrNull())
    }
}
