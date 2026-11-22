package com.aoheyra.ble.core

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.util.concurrent.atomic.AtomicInteger

class OperationQueueTest {
    private class ManualBleThread : BleThread {
        private val tasks = ArrayDeque<Runnable>()
        private var accepting = true

        override fun post(task: Runnable): Boolean {
            if (!accepting) return false
            tasks.addLast(task)
            return true
        }

        override fun gattCallbackHandler(): Any? = null

        override fun close() {
            accepting = false
            tasks.clear()
        }

        fun drainOne(): Boolean {
            val task = tasks.removeFirstOrNull() ?: return false
            task.run()
            return true
        }
    }

    private class RecordingOperation(
        override val kind: OperationKind,
        private val id: Int,
        private val log: MutableList<String>,
        private val active: AtomicInteger,
        private val maxActive: AtomicInteger,
        private val onStart: () -> Unit,
    ) : GattOperation {
        override fun start() {
            val current = active.incrementAndGet()
            maxActive.updateAndGet { previous -> maxOf(previous, current) }
            log.add("start-$id")
            active.decrementAndGet()
            onStart()
        }

        override fun failed(error: BleError) {
            log.add("failed-$id-${error.javaClass.simpleName}")
        }
    }

    @Test
    fun `parallel requests never run two operations on one link`() {
        val thread = ManualBleThread()
        val executor = BoundedTaskExecutor(thread)
        lateinit var queue: OperationQueue
        queue = OperationQueue(executor)
        val log = mutableListOf<String>()
        val active = AtomicInteger(0)
        val maxActive = AtomicInteger(0)

        repeat(3) { index ->
            assertTrue(
                queue.submit(
                    RecordingOperation(
                        kind = OperationKind.WRITE,
                        id = index,
                        log = log,
                        active = active,
                        maxActive = maxActive,
                        onStart = { queue.complete() },
                    ),
                ).isSuccess,
            )
        }
        repeat(3) { assertTrue(thread.drainOne()) }

        assertEquals(1, maxActive.get())
        assertEquals(listOf("start-0", "start-1", "start-2"), log)
        assertFalse(queue.isBusy())
    }

    @Test
    fun `queue rejects work beyond its bound`() {
        val queue = OperationQueue(BoundedTaskExecutor(ManualBleThread()), capacity = 2)
        val log = mutableListOf<String>()
        val noop = { Unit }

        fun operation(id: Int) = RecordingOperation(
            kind = OperationKind.DISCOVER,
            id = id,
            log = log,
            active = AtomicInteger(),
            maxActive = AtomicInteger(),
            onStart = noop,
        )

        assertTrue(queue.submit(operation(0)).isSuccess)
        assertTrue(queue.submit(operation(1)).isSuccess)
        assertTrue(queue.submit(operation(2)).isSuccess)
        assertEquals(BleError.QueueFull, queue.submit(operation(3)).exceptionOrNull())
        assertEquals(2, queue.pendingSize())
    }

    @Test
    fun `failure closes the queue and fails every waiter`() {
        val queue = OperationQueue(BoundedTaskExecutor(ManualBleThread()))
        val log = mutableListOf<String>()
        val failure = BleError.Gatt(Stage.WRITE, 133)

        fun operation(id: Int) = RecordingOperation(
            kind = OperationKind.NOTIFY,
            id = id,
            log = log,
            active = AtomicInteger(),
            maxActive = AtomicInteger(),
            onStart = { Unit },
        )

        repeat(3) { assertTrue(queue.submit(operation(it)).isSuccess) }
        queue.failCurrent(failure)

        assertEquals(
            listOf("failed-0-Gatt", "failed-1-Gatt", "failed-2-Gatt"),
            log,
        )
        assertEquals(0, queue.pendingSize())
        assertEquals(BleError.Closed, queue.submit(operation(3)).exceptionOrNull())
    }

    @Test
    fun `bounded executor rejects overflow and recovers after a task finishes`() {
        val thread = ManualBleThread()
        val executor = BoundedTaskExecutor(thread, capacity = 1)

        assertTrue(executor.execute(Runnable { }).isSuccess)
        assertEquals(BleError.QueueFull, executor.execute(Runnable { }).exceptionOrNull())
        assertTrue(thread.drainOne())
        assertEquals(0, executor.outstanding())
        assertTrue(executor.execute(Runnable { }).isSuccess)
    }

    @Test
    fun `closed thread is reported instead of losing work`() {
        val thread = ManualBleThread()
        thread.close()
        assertEquals(
            BleError.Closed,
            BoundedTaskExecutor(thread).execute(Runnable { }).exceptionOrNull(),
        )
    }
}
