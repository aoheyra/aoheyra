package com.aoheyra.ble.scan

import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BoundedTaskExecutor

/** Platform side of scanning; all calls happen on the serial BLE thread. */
interface ScanController {
    /** Starts scanning with the fixed service-UUID filter. */
    fun start(mode: ScanPower): BleResult<Unit>
    /** Stops scanning. */
    fun stop(): BleResult<Unit>
}

/** NativeSink-facing scanner facade with immediate return. */
class Scanner(
    private val planner: ScanPlanner,
    private val controller: ScanController,
    private val executor: BoundedTaskExecutor,
) {
    /** Starts, changes or stops scanning from a Rust profile. */
    fun setScanning(mode: ScanPower?): BleResult<Unit> = apply(planner.setScanning(mode))

    /** Applies a radio state change. */
    fun setRadioReady(ready: Boolean): BleResult<Unit> = apply(planner.setRadioReady(ready))

    /** Reports a platform failure to Rust. */
    fun onScanFailed(code: Int) {
        planner.onScanFailed(code)
    }

    private fun apply(command: ScanCommand): BleResult<Unit> = when (command) {
        ScanCommand.Hold -> Result.success(Unit)
        ScanCommand.Stop -> executor.execute(Runnable { controller.stop() })
        is ScanCommand.Start -> executor.execute(Runnable { controller.start(command.mode) })
    }
}
