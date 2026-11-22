package com.aoheyra.app.service

/**
 * Process-local binding between the foreground service and the runtime.
 *
 * Android constructs the service itself, so the runtime registers its
 * lifecycle here. An old runtime cannot unbind a newer one.
 */
object MeshServiceRuntime {
    @Volatile
    private var lifecycle: MeshRuntimeLifecycle? = null

    /** Makes [lifecycle] the current runtime owner. */
    fun bind(lifecycle: MeshRuntimeLifecycle) {
        synchronized(this) { this.lifecycle = lifecycle }
    }

    /** Removes [lifecycle] if it is still the current owner. */
    fun unbind(lifecycle: MeshRuntimeLifecycle) {
        synchronized(this) {
            if (this.lifecycle === lifecycle) this.lifecycle = null
        }
    }

    /** Current runtime owner, or null before APP-03 wires one. */
    fun current(): MeshRuntimeLifecycle? = lifecycle
}
