package com.aoheyra.ble.scan

import com.aoheyra.ble.core.Clock
import org.junit.Assert.assertEquals
import org.junit.Test

class ScanPlannerTest {
    private class TestClock(var now: Long = 1_000L) : Clock {
        override fun nowMillis(): Long = now
    }

    private class TestEvents : ScanEvents {
        val failures = mutableListOf<Int>()
        val results = mutableListOf<Triple<com.aoheyra.ble.core.PeerKey, Short, ByteArray>>()

        override fun onDiscoveryFailed(code: Int) {
            failures.add(code)
        }

        override fun onScanResult(peer: com.aoheyra.ble.core.PeerKey, rssi: Short, beacon: ByteArray) {
            results.add(Triple(peer, rssi, beacon))
        }
    }

    @Test
    fun `starts with a profile and holds identical requests`() {
        val planner = ScanPlanner(TestEvents(), TestClock())
        assertEquals(ScanCommand.Start(ScanPower.BALANCED), planner.setScanning(ScanPower.BALANCED))
        assertEquals(ScanCommand.Hold, planner.setScanning(ScanPower.BALANCED))
    }

    @Test
    fun `rapid profile changes are coalesced`() {
        val clock = TestClock()
        val planner = ScanPlanner(TestEvents(), clock)
        planner.setScanning(ScanPower.LOW_POWER)
        assertEquals(ScanCommand.Hold, planner.setScanning(ScanPower.LOW_LATENCY))
        clock.now += ScanPlanner.RESTART_WINDOW_MS
        assertEquals(ScanCommand.Start(ScanPower.LOW_LATENCY), planner.setScanning(ScanPower.LOW_LATENCY))
    }

    @Test
    fun `bluetooth toggle restarts the last desired scan profile`() {
        val planner = ScanPlanner(TestEvents(), TestClock())
        planner.setScanning(ScanPower.BALANCED)
        assertEquals(ScanCommand.Hold, planner.setRadioReady(false))
        assertEquals(ScanCommand.Start(ScanPower.BALANCED), planner.setRadioReady(true))
    }

    @Test
    fun `an explicit stop is not undone by a bluetooth toggle`() {
        val planner = ScanPlanner(TestEvents(), TestClock())
        planner.setScanning(ScanPower.BALANCED)
        assertEquals(ScanCommand.Stop, planner.setScanning(null))
        assertEquals(ScanCommand.Hold, planner.setRadioReady(false))
        assertEquals(ScanCommand.Hold, planner.setRadioReady(true))
    }

    @Test
    fun `scan failure is reported and blocks a rapid restart`() {
        val clock = TestClock()
        val events = TestEvents()
        val planner = ScanPlanner(events, clock)
        planner.setScanning(ScanPower.BALANCED)

        planner.onScanFailed(2)
        assertEquals(listOf(2), events.failures)
        assertEquals(ScanCommand.Hold, planner.setScanning(ScanPower.BALANCED))
        clock.now += ScanPlanner.RESTART_WINDOW_MS
        assertEquals(ScanCommand.Start(ScanPower.BALANCED), planner.setScanning(ScanPower.BALANCED))
    }
}
