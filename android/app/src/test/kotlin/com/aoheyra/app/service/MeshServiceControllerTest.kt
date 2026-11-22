package com.aoheyra.app.service

import android.app.Service
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class MeshServiceControllerTest {
    private class FakeRuntime : MeshRuntimeLifecycle {
        var starts = 0
        var stops = 0
        var failStart = false
        var failStop = false

        override fun start(): Result<Unit> {
            starts += 1
            return if (failStart) Result.failure(IllegalStateException("start")) else Result.success(Unit)
        }

        override fun stop(): Result<Unit> {
            stops += 1
            return if (failStop) Result.failure(IllegalStateException("stop")) else Result.success(Unit)
        }
    }

    @Test
    fun `duplicate start requests start the runtime once`() {
        val runtime = FakeRuntime()
        val controller = MeshServiceController(runtime)

        assertTrue(controller.start() is MeshStartOutcome.Started)
        assertTrue(controller.start() is MeshStartOutcome.AlreadyStarted)
        assertTrue(controller.start() is MeshStartOutcome.AlreadyStarted)

        assertEquals(1, runtime.starts)
        assertEquals(MeshRuntimeState.RUNNING, controller.currentState())
    }

    @Test
    fun `duplicate stop requests stop the runtime once`() {
        val runtime = FakeRuntime()
        val controller = MeshServiceController(runtime)
        controller.start()

        assertTrue(controller.stop() is MeshStopOutcome.Stopped)
        assertTrue(controller.stop() is MeshStopOutcome.AlreadyStopped)

        assertEquals(1, runtime.stops)
        assertEquals(MeshRuntimeState.STOPPED, controller.currentState())
    }

    @Test
    fun `failed start leaves the controller retryable`() {
        val runtime = FakeRuntime().apply { failStart = true }
        val controller = MeshServiceController(runtime)

        val failure = controller.start()
        assertTrue(failure is MeshStartOutcome.Failed)
        assertEquals(MeshRuntimeState.STOPPED, controller.currentState())

        runtime.failStart = false
        assertTrue(controller.start() is MeshStartOutcome.Started)
        assertEquals(2, runtime.starts)
    }

    @Test
    fun `failed stop keeps the running state for the next stop`() {
        val runtime = FakeRuntime().apply { failStop = true }
        val controller = MeshServiceController(runtime)
        controller.start()

        assertTrue(controller.stop() is MeshStopOutcome.Failed)
        assertEquals(MeshRuntimeState.RUNNING, controller.currentState())

        runtime.failStop = false
        assertTrue(controller.stop() is MeshStopOutcome.Stopped)
        assertEquals(2, runtime.stops)
    }

    @Test
    fun `missing runtime is reported without calling anything`() {
        val controller = MeshServiceController(null)

        assertEquals(MeshStartOutcome.Unavailable, controller.start())
        assertEquals(MeshRuntimeState.UNAVAILABLE, controller.currentState())
    }

    @Test
    fun `restart policy keeps sticky start only until an explicit stop`() {
        assertEquals(Service.START_STICKY, MeshRestartPolicy.forStart(false))
        assertEquals(Service.START_NOT_STICKY, MeshRestartPolicy.forStart(true))
        assertEquals(Service.START_NOT_STICKY, MeshRestartPolicy.FOR_STOP)
    }
}
