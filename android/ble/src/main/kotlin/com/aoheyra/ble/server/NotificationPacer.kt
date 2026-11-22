package com.aoheyra.ble.server

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.Stage

/** What the serial thread should do after a notification send callback. */
sealed class PacerOutcome {
    /** No notification is in flight. */
    object Idle : PacerOutcome()

    /** The stack accepted the previous write; send the next value if present. */
    data class SendNext(val bytes: ByteArray, val writeReady: Boolean) : PacerOutcome()

    /** The stack accepted the previous write and the queue is empty. */
    object WriteReady : PacerOutcome()

    /** The send failed; the link must be closed. */
    data class Failed(val error: BleError) : PacerOutcome()
}

/**
 * One in-flight notification per inbound central.
 *
 * The next notification is released only from the send callback, then
 * `WriteReady` is raised for Rust. Values beyond the cap are rejected with a
 * typed error rather than buffered without a bound.
 */
class NotificationPacer(private val capacity: Int = DEFAULT_CAPACITY) {
    private val lock = Any()
    private val queue = ArrayDeque<ByteArray>()
    private var inFlight = false

    init {
        if (capacity <= 0) throw IllegalArgumentException("capacity must be positive")
    }

    /** Adds one raw ATT value. */
    fun enqueue(bytes: ByteArray): BleResult<Unit> = synchronized(lock) {
        if (queue.size >= capacity) return Result.failure(BleError.QueueFull)
        queue.addLast(bytes.copyOf())
        Result.success(Unit)
    }

    /** Takes the first value when no send is outstanding. */
    fun next(): ByteArray? = synchronized(lock) {
        if (inFlight) return null
        val bytes = queue.removeFirstOrNull() ?: return null
        inFlight = true
        bytes
    }

    /** Handles one `onNotificationSent` callback. */
    fun onSent(success: Boolean, code: Int): PacerOutcome = synchronized(lock) {
        if (!inFlight) return PacerOutcome.Idle
        if (!success) {
            inFlight = false
            queue.clear()
            val error = if (code == 0) {
                BleError.Refused(Stage.WRITE, -1)
            } else {
                BleError.Gatt(Stage.WRITE, code)
            }
            return PacerOutcome.Failed(error)
        }
        val next = queue.removeFirstOrNull()
        if (next == null) {
            inFlight = false
            return PacerOutcome.WriteReady
        }
        PacerOutcome.SendNext(next, writeReady = true)
    }

    /** Drops all buffered values after a link closes. */
    fun clear() = synchronized(lock) {
        queue.clear()
        inFlight = false
    }

    /** Buffered values, for resource-bound tests. */
    fun pendingSize(): Int = synchronized(lock) { queue.size }

    companion object {
        /** Notification backlog per inbound central. */
        const val DEFAULT_CAPACITY: Int = 8
    }
}
