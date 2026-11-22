package com.aoheyra.app.service

import android.content.Context
import android.content.Intent

/** Runtime-facing API for the foreground service; no BLE radio calls here. */
object MeshServiceLauncher {
    /** Action used when the runtime asks the system to start the mesh. */
    const val ACTION_START: String = "com.aoheyra.app.service.START"

    /** Action used by the notification stop button and the runtime. */
    const val ACTION_STOP: String = "com.aoheyra.app.service.STOP"

    /** Action used to refresh the ongoing notification's state. */
    const val ACTION_STATE: String = "com.aoheyra.app.service.STATE"

    /** Extra carrying the [MeshRuntimeState] name. */
    const val EXTRA_STATE: String = "com.aoheyra.app.service.STATE_VALUE"

    /** Starts or reconnects to the service; the runtime decides when to call. */
    fun start(context: Context) {
        SharedPreferencesStopFlagStore(context).setStopped(false)
        context.startForegroundService(intent(context, ACTION_START))
    }

    /** Stops the service and records the explicit stop. */
    fun stop(context: Context) {
        context.startService(intent(context, ACTION_STOP))
    }

    /** Updates the visible notification without restarting the runtime. */
    fun setState(context: Context, state: MeshRuntimeState) {
        context.startService(intent(context, ACTION_STATE).putExtra(EXTRA_STATE, state.name))
    }

    private fun intent(context: Context, action: String): Intent =
        Intent(context, ForegroundMeshService::class.java).setAction(action)
}
