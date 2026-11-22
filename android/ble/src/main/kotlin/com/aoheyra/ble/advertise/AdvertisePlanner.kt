package com.aoheyra.ble.advertise

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.Clock

/** Where start failures go; the caller forwards them to Rust unchanged. */
interface AdvertiseEvents {
    /** Raw Android `AdvertiseCallback` error code. */
    fun onStartFailure(code: Int)
}

/**
 * Pure advertiser state machine.
 *
 * Rust controls the beacon and duty cycle. Android's start path is throttled
 * with a long hold window so repeated profile changes cannot trip the start
 * limit. Beacon bytes are copied but never decoded here.
 */
class AdvertisePlanner(
    private val events: AdvertiseEvents,
    private val clock: Clock,
) {
    private val lock = Any()
    private var desiredBeacon: ByteArray? = null
    private var desiredMode: AdvertisePower = AdvertisePower.BALANCED
    private var desiredConnectable: Boolean = true
    private var runningBeacon: ByteArray? = null
    private var runningMode: AdvertisePower? = null
    private var runningConnectable: Boolean? = null
    private var radioReady: Boolean = true
    private var forceRestart: Boolean = false
    private var blockedUntil: Long = Long.MIN_VALUE
    private var lastStart: Long = Long.MIN_VALUE

    /** Sets or stops advertising; only the latest Rust request is kept. */
    fun setAdvertising(beacon: ByteArray?, mode: AdvertisePower): BleResult<AdvertiseCommand> {
        if (beacon != null && beacon.size != BEACON_LENGTH) {
            return Result.failure(BleError.InvalidBeacon)
        }
        return synchronized(lock) {
            desiredBeacon = beacon?.copyOf()
            desiredMode = mode
            planLocked(clock.nowMillis())
        }
    }

    /** Rust-driven connectable toggle; applied by restarting the advertiser. */
    fun setConnectable(connectable: Boolean): AdvertiseCommand = synchronized(lock) {
        desiredConnectable = connectable
        planLocked(clock.nowMillis())
    }

    /** Adapter or permission state changed. */
    fun setRadioReady(ready: Boolean): AdvertiseCommand = synchronized(lock) {
        if (radioReady && !ready) {
            runningBeacon = null
            runningMode = null
            runningConnectable = null
        }
        radioReady = ready
        if (ready) forceRestart = true
        planLocked(clock.nowMillis())
    }

    /** Reports a platform start failure and blocks another start briefly. */
    fun onStartFailure(code: Int) = synchronized(lock) {
        runningBeacon = null
        runningMode = null
        runningConnectable = null
        blockedUntil = clock.nowMillis() + RESTART_WINDOW_MS
        events.onStartFailure(code)
        Unit
    }

    private fun planLocked(now: Long): AdvertiseCommand {
        if (!radioReady) return AdvertiseCommand.Hold
        val beacon = desiredBeacon ?: run {
            if (runningBeacon != null) {
                runningBeacon = null
                runningMode = null
                runningConnectable = null
                return AdvertiseCommand.Stop
            }
            return AdvertiseCommand.Hold
        }
        if (now < blockedUntil) return AdvertiseCommand.Hold

        val sameAdvertisement =
            runningBeacon?.contentEquals(beacon) == true &&
                runningMode == desiredMode &&
                runningConnectable == desiredConnectable
        if (sameAdvertisement && !forceRestart) return AdvertiseCommand.Hold
        if (!forceRestart && runningBeacon != null && now - lastStart < RESTART_WINDOW_MS) {
            return AdvertiseCommand.Hold
        }

        runningBeacon = beacon.copyOf()
        runningMode = desiredMode
        runningConnectable = desiredConnectable
        lastStart = now
        forceRestart = false
        return AdvertiseCommand.Start(beacon.copyOf(), desiredMode, desiredConnectable)
    }

    companion object {
        /** NET-03 beacon length. */
        const val BEACON_LENGTH: Int = 16

        /** Long window shared with Rust's discovery interval to avoid start throttling. */
        const val RESTART_WINDOW_MS: Long = 10_000L
    }
}
