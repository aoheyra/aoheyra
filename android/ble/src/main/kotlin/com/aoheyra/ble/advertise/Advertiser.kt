package com.aoheyra.ble.advertise

import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BoundedTaskExecutor

/** Platform side of advertising; all calls happen on the serial BLE thread. */
interface AdvertiseController {
    /** Starts or replaces the advertisement with the raw 16-byte beacon. */
    fun start(beacon: ByteArray, mode: AdvertisePower, connectable: Boolean): BleResult<Unit>

    /** Stops advertising. */
    fun stop(): BleResult<Unit>
}

/**
 * NativeSink-facing advertiser facade.
 *
 * Every entry point returns immediately: state is planned on the caller's
 * thread and the platform call is posted to the bounded BLE executor.
 */
class Advertiser(
    private val planner: AdvertisePlanner,
    private val controller: AdvertiseController,
    private val executor: BoundedTaskExecutor,
) {
    /** Starts, replaces or stops advertising from a Rust profile. */
    fun setAdvertising(beacon: ByteArray?, mode: AdvertisePower): BleResult<Unit> =
        planner.setAdvertising(beacon, mode).fold(
            onSuccess = { apply(it) },
            onFailure = { Result.failure(it) },
        )

    /** Applies a Rust-driven connectable toggle. */
    fun setConnectable(connectable: Boolean): BleResult<Unit> =
        apply(planner.setConnectable(connectable))

    /** Restarts after Bluetooth or permission state returns to ready. */
    fun setRadioReady(ready: Boolean): BleResult<Unit> =
        apply(planner.setRadioReady(ready))

    /** Forwards a platform start failure to Rust without retrying locally. */
    fun onStartFailure(code: Int) {
        planner.onStartFailure(code)
    }

    private fun apply(command: AdvertiseCommand): BleResult<Unit> = when (command) {
        AdvertiseCommand.Hold -> Result.success(Unit)
        AdvertiseCommand.Stop -> executor.execute(Runnable { controller.stop() })
        is AdvertiseCommand.Start -> executor.execute(
            Runnable { controller.start(command.beacon, command.mode, command.connectable) },
        )
    }
}
