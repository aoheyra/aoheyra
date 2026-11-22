package com.aoheyra.ble.core

/** The GATT steps that must never overlap on one link. */
enum class OperationKind {
    MTU,
    DISCOVER,
    DESCRIPTOR_WRITE,
    WRITE,
    NOTIFY,
    DISCONNECT,
}

/** OneGATT step; asynchronous steps call [OperationQueue.complete] from their callback. */
interface GattOperation {
    /** Used only for diagnostics; never carries payload bytes. */
    val kind: OperationKind

    /** Starts the platform call. Called on the serial BLE thread. */
    fun start()

    /** Called when the link fails while this step is outstanding. */
    fun failed(error: BleError)
}

/**
 * Per-link single-flight queue.
 *
 * The first operation starts immediately on the serial executor. Later ones
 * wait until the outstanding operation reports completion. A failure closes
 * the queue and fails every waiter with the same typed error; no operation is
 * silently dropped.
 */
class OperationQueue(
    private val executor: BoundedTaskExecutor,
    private val capacity: Int = DEFAULT_CAPACITY,
) {
    private val lock = Any()
    private val pending = ArrayDeque<GattOperation>()
    private var current: GattOperation? = null
    private var closed = false

    init {
        if (capacity <= 0) throw IllegalArgumentException("capacity must be positive")
    }

    /** Adds one operation, rejecting it rather than growing past the cap. */
    fun submit(operation: GattOperation): BleResult<Unit> = synchronized(lock) {
        if (closed) return Result.failure(BleError.Closed)
        if (pending.size >= capacity) return Result.failure(BleError.QueueFull)
        pending.addLast(operation)
        pumpLocked()
        Result.success(Unit)
    }

    /** Signals that the outstanding operation finished; starts the next one. */
    fun complete() = synchronized(lock) {
        if (current == null) return@synchronized
        current = null
        pumpLocked()
    }

    /**
     * Fails the outstanding operation and every waiter.
     *
     * A failed GATT operation ends its link under NET-03, so the queue is
     * closed rather than left with a half-finished procedure.
     */
    fun failCurrent(error: BleError) = synchronized(lock) {
        val operation = current
        current = null
        operation?.failed(error)
        closeLocked(error)
    }

    /** Closes the queue and fails anything still outstanding. */
    fun close(error: BleError = BleError.Closed) = synchronized(lock) {
        closeLocked(error)
    }

    /** Pending count, for resource-bound tests. */
    fun pendingSize(): Int = synchronized(lock) { pending.size }

    /** True while an operation is outstanding. */
    fun isBusy(): Boolean = synchronized(lock) { current != null }

    private fun pumpLocked() {
        if (closed || current != null || pending.isEmpty()) return
        val operation = pending.removeFirst()
        current = operation
        val posted = executor.execute(Runnable { operation.start() })
        if (posted.isFailure) {
            current = null
            operation.failed(posted.exceptionOrNull().asBleError())
            closeLocked(BleError.Closed)
        }
    }

    private fun closeLocked(error: BleError) {
        closed = true
        current?.failed(error)
        current = null
        while (pending.isNotEmpty()) {
            pending.removeFirst().failed(error)
        }
    }

    private fun Throwable?.asBleError(): BleError =
        (this as? BleError) ?: BleError.Closed

    companion object {
        /** Caps queued GATT steps per link. */
        const val DEFAULT_CAPACITY: Int = 8
    }
}
