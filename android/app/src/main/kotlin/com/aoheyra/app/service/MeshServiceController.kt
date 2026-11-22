package com.aoheyra.app.service

import android.app.Service

/** The application runtime that owns the BLE module and Rust bridge. */
interface MeshRuntimeLifecycle {
    /** Starts the runtime; must return promptly. */
    fun start(): Result<Unit>

    /** Stops the runtime; must be idempotent and return promptly. */
    fun stop(): Result<Unit>
}

/** Visible state of the mesh, used for the ongoing notification only. */
enum class MeshRuntimeState {
    /** No runtime is running. */
    STOPPED,

    /** The runtime is being created. */
    STARTING,

    /** Radio and Rust bridge are running. */
    RUNNING,

    /** The runtime is shutting down. */
    STOPPING,

    /** No runtime implementation is bound; APP-03 must supply one. */
    UNAVAILABLE,
}

/** Result of a start request; repeated requests are not treated as failures. */
sealed class MeshStartOutcome {
    /** The runtime was started. */
    object Started : MeshStartOutcome()

    /** An active or pending runtime already owns the service. */
    object AlreadyStarted : MeshStartOutcome()

    /** No runtime implementation is bound. */
    object Unavailable : MeshStartOutcome()

    /** The runtime refused to start. */
    data class Failed(val error: Throwable) : MeshStartOutcome()
}

/** Result of a stop request; repeated requests are not treated as failures. */
sealed class MeshStopOutcome {
    /** The runtime was stopped. */
    object Stopped : MeshStopOutcome()

    /** No runtime was active. */
    object AlreadyStopped : MeshStopOutcome()

    /** The runtime refused to stop. */
    data class Failed(val error: Throwable) : MeshStopOutcome()
}

/**
 * Pure, idempotent start/stop state machine for the foreground service.
 *
 * Android can deliver duplicate start commands and deliveries after a process
 * restart. This keeps runtime calls to exactly one start and one stop per
 * transition without relying on intents or platform classes.
 */
class MeshServiceController(private val lifecycle: MeshRuntimeLifecycle?) {
    private val lock = Any()
    private var state: MeshRuntimeState = MeshRuntimeState.STOPPED

    /** Current observed runtime state. */
    fun currentState(): MeshRuntimeState = synchronized(lock) { state }

    /** Starts the runtime once. */
    fun start(): MeshStartOutcome = synchronized(lock) {
        when (state) {
            MeshRuntimeState.STARTING,
            MeshRuntimeState.RUNNING,
            -> MeshStartOutcome.AlreadyStarted

            MeshRuntimeState.UNAVAILABLE -> MeshStartOutcome.Unavailable
            MeshRuntimeState.STOPPED,
            MeshRuntimeState.STOPPING,
            -> {
                val runtime = lifecycle ?: run {
                    state = MeshRuntimeState.UNAVAILABLE
                    return@synchronized MeshStartOutcome.Unavailable
                }
                state = MeshRuntimeState.STARTING
                val result = runtime.start()
                result.fold(
                    onSuccess = {
                        state = MeshRuntimeState.RUNNING
                        MeshStartOutcome.Started
                    },
                    onFailure = { error ->
                        state = MeshRuntimeState.STOPPED
                        MeshStartOutcome.Failed(error)
                    },
                )
            }
        }
    }

    /** Stops the runtime once. */
    fun stop(): MeshStopOutcome = synchronized(lock) {
        if (state == MeshRuntimeState.STOPPED) return@synchronized MeshStopOutcome.AlreadyStopped
        val previous = state
        state = MeshRuntimeState.STOPPING
        val result = lifecycle?.stop() ?: Result.success(Unit)
        result.fold(
            onSuccess = {
                state = MeshRuntimeState.STOPPED
                MeshStopOutcome.Stopped
            },
            onFailure = { error ->
                state = previous
                MeshStopOutcome.Failed(error)
            },
        )
    }

    /** Records that no runtime implementation is bound any more. */
    fun markUnavailable() = synchronized(lock) {
        state = MeshRuntimeState.UNAVAILABLE
        Unit
    }

    /** Applies a state reported by the runtime for the notification. */
    fun update(state: MeshRuntimeState) = synchronized(lock) {
        if (this.state != MeshRuntimeState.UNAVAILABLE) this.state = state
        Unit
    }
}

/** Restart flags chosen from the action, never from a timer or timer heuristic. */
object MeshRestartPolicy {
    /** A running mesh is recreated by the system after an unexpected kill. */
    fun forStart(explicitlyStopped: Boolean): Int =
        if (explicitlyStopped) Service.START_NOT_STICKY else Service.START_STICKY

    /** An explicit stop is never recreated. */
    const val FOR_STOP: Int = Service.START_NOT_STICKY
}
