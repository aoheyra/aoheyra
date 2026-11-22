package com.aoheyra.ble.advertise

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.Clock
import com.aoheyra.ble.core.SystemClock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AdvertiserTest {
    private class ManualBleThread : BleThread {
        private val tasks = ArrayDeque<Runnable>()
        override fun post(task: Runnable): Boolean {
            tasks.addLast(task)
            return true
        }

        override fun gattCallbackHandler(): Any? = null
        override fun close() = Unit

        fun drainOne(): Boolean {
            val task = tasks.removeFirstOrNull() ?: return false
            task.run()
            return true
        }
    }

    private class RecordingController : AdvertiseController {
        val actions = mutableListOf<String>()
        var lastBeacon: ByteArray? = null

        override fun start(beacon: ByteArray, mode: AdvertisePower, connectable: Boolean): BleResult<Unit> {
            lastBeacon = beacon.copyOf()
            actions.add("start:$mode:$connectable")
            return Result.success(Unit)
        }

        override fun stop(): BleResult<Unit> {
            actions.add("stop")
            return Result.success(Unit)
        }
    }

    private class ManualClock : Clock {
        var now: Long = 1_000L
        override fun nowMillis(): Long = now
    }

    @Test
    fun `calls return before platform work and pass bytes through unchanged`() {
        val thread = ManualBleThread()
        val controller = RecordingController()
        val planner = AdvertisePlanner(object : AdvertiseEvents {
            override fun onStartFailure(code: Int) = Unit
        }, ManualClock())
        val advertiser = Advertiser(planner, controller, BoundedTaskExecutor(thread, capacity = 4))
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH) { it.toByte() }

        assertTrue(advertiser.setAdvertising(beacon, AdvertisePower.BALANCED).isSuccess)
        assertTrue(controller.actions.isEmpty())
        assertTrue(thread.drainOne())
        assertEquals(listOf("start:BALANCED:true"), controller.actions)
        assertTrue(controller.lastBeacon?.contentEquals(beacon) == true)
    }

    @Test
    fun `bounded executor overflow is returned to the caller`() {
        val thread = ManualBleThread()
        val controller = RecordingController()
        val planner = AdvertisePlanner(object : AdvertiseEvents {
            override fun onStartFailure(code: Int) = Unit
        }, ManualClock())
        val advertiser = Advertiser(planner, controller, BoundedTaskExecutor(thread, capacity = 1))
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH)

        assertTrue(advertiser.setAdvertising(beacon, AdvertisePower.BALANCED).isSuccess)
        assertEquals(
            BleError.QueueFull,
            advertiser.setAdvertising(null, AdvertisePower.BALANCED).exceptionOrNull(),
        )
    }

    @Test
    fun `production clock is injectable and never parses bytes`() {
        val planner = AdvertisePlanner(object : AdvertiseEvents {
            override fun onStartFailure(code: Int) = Unit
        }, SystemClock())
        val beacon = ByteArray(AdvertisePlanner.BEACON_LENGTH) { 0x42 }
        assertTrue(
            planner.setAdvertising(beacon, AdvertisePower.LOW_POWER).getOrThrow()
                is AdvertiseCommand.Start,
        )
    }
}
