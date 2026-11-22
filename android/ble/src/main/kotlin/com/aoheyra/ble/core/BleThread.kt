package com.aoheyra.ble.core

import java.util.concurrent.atomic.AtomicInteger

/**
 * The single serial thread used by every GATT callback and operation.
 *
 * The handler is exposed only to platform code that needs a Looper; pure
 * tests provide a fake whose handler is null.
 */
interface BleThread {
    /** Posts work to the serial thread. False means the thread is closing. */
    fun post(task: Runnable): Boolean

    /** Platform Handler for `connectGatt` and `openGattServer`, or null in tests. */
    fun gattCallbackHandler(): Any?

    /** Stops accepting work and releases the thread. */
    fun close()
}

/** Caps outstanding tasks before they reach the platform thread. */
class BoundedTaskExecutor(
    private val thread: BleThread,
    private val capacity: Int = DEFAULT_CAPACITY,
) {
    private val outstanding = AtomicInteger(0)

    init {
        if (capacity <= 0) throw IllegalArgumentException("capacity must be positive")
    }

    /** Posts one task, rejecting it with a typed error when the cap is reached. */
    fun execute(task: Runnable): BleResult<Unit> {
        if (outstanding.incrementAndGet() > capacity) {
            outstanding.decrementAndGet()
            return Result.failure(BleError.QueueFull)
        }
        val wrapped = Runnable {
            try {
                task.run()
            } finally {
                outstanding.decrementAndGet()
            }
        }
        if (!thread.post(wrapped)) {
            outstanding.decrementAndGet()
            return Result.failure(BleError.Closed)
        }
        return Result.success(Unit)
    }

    /** Number of posted tasks that have not finished. */
    fun outstanding(): Int = outstanding.get()

    companion object {
        /** Bounds main-thread requests to the BLE thread. */
        const val DEFAULT_CAPACITY: Int = 64
    }
}
