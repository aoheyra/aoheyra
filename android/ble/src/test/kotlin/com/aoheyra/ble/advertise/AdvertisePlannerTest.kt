package com.aoheyra.ble.advertise

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.Clock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AdvertisePlannerTest {
    private class TestClock(var now: Long = 1_000L) : Clock {
        override fun nowMillis(): Long = now
    }

    private class TestEvents : AdvertiseEvents {
        val codes = mutableListOf<Int>()
        override fun onStartFailure(code: Int) {
            codes.add(code)
        }
    }

    private fun assertStart(
        command: AdvertiseCommand,
        beacon: ByteArray,
        mode: AdvertisePower,
        connectable: Boolean,
    ) {
        assertTrue(command is AdvertiseCommand.Start)
        val start = command as AdvertiseCommand.Start
        assertTrue(start.beacon.contentEquals(beacon))
        assertEquals(mode, start.mode)
        assertEquals(connectable, start.connectable)
    }

    @Test
    fun `starts with raw beacon then holds identical requests`() {
        val events = TestEvents()
        val planner = AdvertisePlanner(events, TestClock())
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH) { it.toByte() }

        val first = planner.setAdvertising(beacon, AdvertisePower.BALANCED).getOrThrow()
        assertStart(first, beacon, AdvertisePower.BALANCED, true)
        assertEquals(
            AdvertiseCommand.Hold,
            planner.setAdvertising(beacon.copyOf(), AdvertisePower.BALANCED).getOrThrow(),
        )
        assertTrue(events.codes.isEmpty())
    }

    @Test
    fun `rejects a beacon with the wrong length`() {
        val planner = AdvertisePlanner(TestEvents(), TestClock())
        assertEquals(
            BleError.InvalidBeacon,
            planner.setAdvertising(ByteArray(15), AdvertisePower.LOW_POWER).exceptionOrNull(),
        )
    }

    @Test
    fun `rapid mode changes are coalesced until the hold window expires`() {
        val clock = TestClock()
        val planner = AdvertisePlanner(TestEvents(), clock)
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH)

        planner.setAdvertising(beacon, AdvertisePower.LOW_POWER)
        assertEquals(
            AdvertiseCommand.Hold,
            planner.setAdvertising(beacon, AdvertisePower.LOW_LATENCY).getOrThrow(),
        )
        clock.now += AdvertisePlanner.RESTART_WINDOW_MS
        assertStart(
            planner.setAdvertising(beacon, AdvertisePower.LOW_LATENCY).getOrThrow(),
            beacon,
            AdvertisePower.LOW_LATENCY,
            true,
        )
    }

    @Test
    fun `bluetooth toggle restarts the last desired advertisement`() {
        val planner = AdvertisePlanner(TestEvents(), TestClock())
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH)

        planner.setAdvertising(beacon, AdvertisePower.BALANCED)
        assertEquals(AdvertiseCommand.Hold, planner.setRadioReady(false))
        assertStart(planner.setRadioReady(true), beacon, AdvertisePower.BALANCED, true)
    }

    @Test
    fun `an explicit stop is not undone by a bluetooth toggle`() {
        val planner = AdvertisePlanner(TestEvents(), TestClock())
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH)

        planner.setAdvertising(beacon, AdvertisePower.BALANCED)
        assertEquals(
            AdvertiseCommand.Stop,
            planner.setAdvertising(null, AdvertisePower.BALANCED).getOrThrow(),
        )
        assertEquals(AdvertiseCommand.Hold, planner.setRadioReady(false))
        assertEquals(AdvertiseCommand.Hold, planner.setRadioReady(true))
    }

    @Test
    fun `connectable toggle restarts without decoding beacon bytes`() {
        val clock = TestClock()
        val planner = AdvertisePlanner(TestEvents(), clock)
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH) { 0x7F }

        planner.setAdvertising(beacon, AdvertisePower.BALANCED)
        clock.now += AdvertisePlanner.RESTART_WINDOW_MS
        assertStart(planner.setConnectable(false), beacon, AdvertisePower.BALANCED, false)
    }

    @Test
    fun `start failure is reported once and briefly blocks restarts`() {
        val clock = TestClock()
        val events = TestEvents()
        val planner = AdvertisePlanner(events, clock)
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH)

        planner.setAdvertising(beacon, AdvertisePower.BALANCED)
        planner.onStartFailure(4)
        assertEquals(listOf(4), events.codes)
        assertEquals(
            AdvertiseCommand.Hold,
            planner.setAdvertising(beacon, AdvertisePower.BALANCED).getOrThrow(),
        )
        clock.now += AdvertisePlanner.RESTART_WINDOW_MS
        assertTrue(
            planner.setAdvertising(beacon, AdvertisePower.BALANCED).getOrThrow()
                is AdvertiseCommand.Start,
        )
    }
}
