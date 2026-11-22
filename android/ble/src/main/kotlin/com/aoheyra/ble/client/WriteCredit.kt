package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult

/**
 * The single write credit from NET-03.
 *
 * Rust is called only while it holds the credit; Kotlin never queues more
 * than one write per link. A second write is rejected rather than buffered.
 */
class WriteCredit {
    private var available = true

    /** Consumes the credit for one ATT value. */
    fun acquire(): BleResult<Unit> {
        if (!available) return Result.failure(BleError.QueueFull)
        available = false
        return Result.success(Unit)
    }

    /** Returns the credit from a successful write callback. */
    fun release() {
        available = true
    }

    /** Drops the credit after a failed write or close. */
    fun clear() {
        available = false
    }
}
