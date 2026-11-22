package com.aoheyra.ble.scan

import com.aoheyra.ble.core.PeerKey
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ScanResultParserTest {
    private class TestEvents : ScanEvents {
        val results = mutableListOf<Triple<PeerKey, Short, ByteArray>>()
        val failures = mutableListOf<Int>()

        override fun onDiscoveryFailed(code: Int) {
            failures.add(code)
        }

        override fun onScanResult(peer: PeerKey, rssi: Short, beacon: ByteArray) {
            results.add(Triple(peer, rssi, beacon))
        }
    }

    private fun peer(): PeerKey = PeerKey.from(ByteArray(PeerKey.LENGTH)).getOrThrow()

    @Test
    fun `raw manufacturer payload is forwarded byte for byte`() {
        val events = TestEvents()
        val beacon = ByteArray(16) { it.toByte() }
        val key = peer()

        assertTrue(
            ScanResultParser.forward(
                RawScan(key, -55, mapOf(ScanResultParser.COMPANY_ID to beacon)),
                events,
            ),
        )
        assertEquals(1, events.results.size)
        assertTrue(events.results[0].first == key)
        assertEquals(-55.toShort(), events.results[0].second)
        assertTrue(events.results[0].third.contentEquals(beacon))
    }

    @Test
    fun `missing or wrong company data yields an empty beacon`() {
        val events = TestEvents()
        assertTrue(ScanResultParser.forward(RawScan(peer(), -60, emptyMap()), events))
        assertTrue(events.results[0].third.isEmpty())
        assertTrue(
            ScanResultParser.forward(
                RawScan(peer(), -60, mapOf(0x0001 to ByteArray(4))),
                events,
            ),
        )
        assertTrue(events.results[1].third.isEmpty())
    }

    @Test
    fun `oversize manufacturer payload is dropped`() {
        val events = TestEvents()
        assertFalse(
            ScanResultParser.forward(
                RawScan(
                    peer(),
                    -60,
                    mapOf(ScanResultParser.COMPANY_ID to ByteArray(ScanResultParser.MAX_BEACON_LENGTH + 1)),
                ),
                events,
            ),
        )
        assertTrue(events.results.isEmpty())
    }

    @Test
    fun `address parsing is strict and returns typed results`() {
        val bytes = BluetoothAddress.parse("AA:BB:CC:DD:EE:FF").getOrThrow()
        assertEquals(0xAA.toByte(), bytes[0])
        assertEquals(0xFF.toByte(), bytes[5])
        assertEquals(16, PeerKey.fromAddress(bytes).getOrThrow().bytes.size)
        assertTrue(BluetoothAddress.parse("AA:BB:CC").isFailure)
        assertTrue(BluetoothAddress.parse("AA:BB:CC:DD:EE:ZZ").isFailure)
    }
}
