package com.aoheyra.ble.scan

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.Clock
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ScannerTest {
    private class ManualBleThread : BleThread {
        private val tasks = ArrayDeque<Runnable>()
        override fun post(task: Runnable): Boolean {
            tasks.addLast(task)
            return true
        }

        override fun gattCallbackHandler(): Any? = null
        override fun close() = Unit

        fun drainOne() {
            tasks.removeFirstOrNull()?.run()
        }
    }

    private class RecordingController : ScanController {
        val actions = mutableListOf<String>()
        override fun start(mode: ScanPower): BleResult<Unit> {
            actions.add("start:$mode")
            return Result.success(Unit)
        }

        override fun stop(): BleResult<Unit> {
            actions.add("stop")
            return Result.success(Unit)
        }
    }

    private class TestClock : Clock {
        override fun nowMillis(): Long = 1_000L
    }

    @Test
    fun `scan call returns before the platform call`() {
        val thread = ManualBleThread()
        val controller = RecordingController()
        val scanner = Scanner(
            ScanPlanner(object : ScanEvents {
                override fun onDiscoveryFailed(code: Int) = Unit
                override fun onScanResult(
                    peer: com.aoheyra.ble.core.PeerKey,
                    rssi: Short,
                    beacon: ByteArray,
                ) = Unit
            }, TestClock()),
            controller,
            BoundedTaskExecutor(thread, capacity = 2),
        )

        assertTrue(scanner.setScanning(ScanPower.BALANCED).isSuccess)
        assertTrue(controller.actions.isEmpty())
        thread.drainOne()
        assertEquals(listOf("start:BALANCED"), controller.actions)
    }

    @Test
    fun `executor overflow is returned instead of buffering without a bound`() {
        val thread = ManualBleThread()
        val controller = RecordingController()
        val scanner = Scanner(
            ScanPlanner(object : ScanEvents {
                override fun onDiscoveryFailed(code: Int) = Unit
                override fun onScanResult(
                    peer: com.aoheyra.ble.core.PeerKey,
                    rssi: Short,
                    beacon: ByteArray,
                ) = Unit
            }, TestClock()),
            controller,
            BoundedTaskExecutor(thread, capacity = 1),
        )

        scanner.setScanning(ScanPower.BALANCED)
        assertEquals(
            BleError.QueueFull,
            scanner.setScanning(null).exceptionOrNull(),
        )
    }
}
