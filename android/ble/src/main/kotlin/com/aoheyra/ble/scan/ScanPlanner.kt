package com.aoheyra.ble.scan

import com.aoheyra.ble.core.Clock

/** Where platform scan failures go; the caller forwards them to Rust. */
interface ScanEvents {
    /** Raw Android `ScanCallback` error code. */
    fun onDiscoveryFailed(code: Int)

    /** Raw peer key, RSSI and manufacturer beacon from a scan record. */
    fun onScanResult(peer: com.aoheyra.ble.core.PeerKey, rssi: Short, beacon: ByteArray)
}

/**
 * Pure scanning state machine.
 *
 * Rust selects the discovery profile; Kotlin only filters by service UUID
 * and guards Android's scan-start limit with a long hold window.
 */
class ScanPlanner(
    private val events: ScanEvents,
    private val clock: Clock,
) {
    private val lock = Any()
    private var desiredMode: ScanPower? = null
    private var runningMode: ScanPower? = null
    private var radioReady: Boolean = true
    private var forceRestart: Boolean = false
    private var blockedUntil: Long = Long.MIN_VALUE
    private var lastStart: Long = Long.MIN_VALUE

    /** Starts, changes or stops scanning from a Rust profile. */
    fun setScanning(mode: ScanPower?): ScanCommand = synchronized(lock) {
        desiredMode = mode
        planLocked(clock.nowMillis())
    }

    /** Restarts scanning after Bluetooth or permission state returns to ready. */
    fun setRadioReady(ready: Boolean): ScanCommand = synchronized(lock) {
        if (radioReady && !ready) runningMode = null
        radioReady = ready
        if (ready) forceRestart = true
        planLocked(clock.nowMillis())
    }

    /** Reports a platform start failure and blocks another start briefly. */
    fun onScanFailed(code: Int) = synchronized(lock) {
        runningMode = null
        blockedUntil = clock.nowMillis() + RESTART_WINDOW_MS
        events.onDiscoveryFailed(code)
        Unit
    }

    private fun planLocked(now: Long): ScanCommand {
        if (!radioReady) return ScanCommand.Hold
        val mode = desiredMode ?: run {
            if (runningMode != null) {
                runningMode = null
                return ScanCommand.Stop
            }
            return ScanCommand.Hold
        }
        if (now < blockedUntil) return ScanCommand.Hold
        if (runningMode == mode && !forceRestart) return ScanCommand.Hold
        if (!forceRestart && runningMode != null && now - lastStart < RESTART_WINDOW_MS) {
            return ScanCommand.Hold
        }

        runningMode = mode
        lastStart = now
        forceRestart = false
        return ScanCommand.Start(mode)
    }

    companion object {
        /** Long window shared with Rust's discovery interval to avoid start throttling. */
        const val RESTART_WINDOW_MS: Long = 10_000L
    }
}
