package com.aoheyra.ble.core

/** Injected time source for BLE throttles; never used for protocol logic. */
interface Clock {
    /** Current monotonic-ish time in milliseconds. */
    fun nowMillis(): Long
}

/** Production clock from the system wall time. */
class SystemClock : Clock {
    override fun nowMillis(): Long = System.currentTimeMillis()
}
