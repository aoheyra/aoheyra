package com.aoheyra.app.service

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import androidx.core.app.ServiceCompat
import com.aoheyra.app.MainActivity
import com.aoheyra.app.R

/**
 * Keeps the mesh process alive with the connected-device foreground type.
 *
 * The runtime decides when to start and stop; this service only owns the
 * notification and translates lifecycle commands into runtime calls. Doze is
 * handled by the connected-device type plus the BLE scanner's hardware filter:
 * no wake lock or battery-optimization exemption is requested.
 */
class ForegroundMeshService : Service() {
    private val stopFlag: StopFlagStore by lazy { SharedPreferencesStopFlagStore(this) }
    private var controller: MeshServiceController? = null
    private var controllerLifecycle: MeshRuntimeLifecycle? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val action = intent?.action ?: ACTION_RESTART
        return when (action) {
            MeshServiceLauncher.ACTION_STOP -> stopMesh()
            MeshServiceLauncher.ACTION_STATE -> {
                stateFrom(intent)?.let(::show)
                MeshRestartPolicy.forStart(stopFlag.isStopped())
            }

            MeshServiceLauncher.ACTION_START,
            ACTION_RESTART,
            -> startMesh(stopFlag.isStopped())

            else -> startMesh(stopFlag.isStopped())
        }
    }

    override fun onBind(intent: Intent?): Nothing? = null

    /**
     * Does not stop the runtime here. An unexpected kill must be reconstructed
     * by `START_STICKY`; an explicit stop already went through [stopMesh].
     */
    override fun onDestroy() {
        super.onDestroy()
    }

    private fun startMesh(explicitlyStopped: Boolean): Int {
        if (explicitlyStopped) {
            stopSelf()
            return MeshRestartPolicy.forStart(true)
        }
        val lifecycle = MeshServiceRuntime.current()
        if (lifecycle == null) {
            show(MeshRuntimeState.UNAVAILABLE)
            stopForeground(STOP_FOREGROUND_REMOVE)
            stopSelf()
            return Service.START_NOT_STICKY
        }
        show(MeshRuntimeState.STARTING)
        val controller = controllerFor(lifecycle)
        when (val outcome = controller.start()) {
            MeshStartOutcome.Started -> show(MeshRuntimeState.RUNNING)
            MeshStartOutcome.AlreadyStarted -> show(controller.currentState())
            MeshStartOutcome.Unavailable -> {
                show(MeshRuntimeState.UNAVAILABLE)
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return Service.START_NOT_STICKY
            }

            is MeshStartOutcome.Failed -> {
                show(MeshRuntimeState.UNAVAILABLE)
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
                return Service.START_NOT_STICKY
            }
        }
        return MeshRestartPolicy.forStart(false)
    }

    private fun stopMesh(): Int {
        stopFlag.setStopped(true)
        controller?.stop()
        show(MeshRuntimeState.STOPPING)
        stopForeground(STOP_FOREGROUND_REMOVE)
        stopSelf()
        return MeshRestartPolicy.FOR_STOP
    }

    private fun controllerFor(lifecycle: MeshRuntimeLifecycle): MeshServiceController {
        val existing = controller
        if (existing != null && controllerLifecycle === lifecycle) return existing
        val replacement = MeshServiceController(lifecycle)
        controller = replacement
        controllerLifecycle = lifecycle
        return replacement
    }

    private fun stateFrom(intent: Intent): MeshRuntimeState? {
        val value = intent.getStringExtra(MeshServiceLauncher.EXTRA_STATE) ?: return null
        return runCatching { MeshRuntimeState.valueOf(value) }.getOrNull()
    }

    private fun createChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.mesh_channel_name),
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = getString(R.string.mesh_channel_description)
        }
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    private fun show(state: MeshRuntimeState) {
        val stopIntent = PendingIntent.getService(
            this,
            STOP_REQUEST_CODE,
            Intent(this, ForegroundMeshService::class.java).setAction(MeshServiceLauncher.ACTION_STOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val openIntent = PendingIntent.getActivity(
            this,
            OPEN_REQUEST_CODE,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val notification = Notification.Builder(this, CHANNEL_ID)
            .setSmallIcon(R.drawable.ic_mesh_foreground)
            .setContentTitle(getString(R.string.mesh_notification_title))
            .setContentText(getString(state.descriptionRes()))
            .setContentIntent(openIntent)
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_SERVICE)
            .addAction(
                Notification.Action.Builder(
                    R.drawable.ic_stop,
                    getString(R.string.mesh_stop_action),
                    stopIntent,
                ).build(),
            )
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        } else {
            0
        }
        ServiceCompat.startForeground(this, NOTIFICATION_ID, notification, type)
    }

    private fun MeshRuntimeState.descriptionRes(): Int = when (this) {
        MeshRuntimeState.STOPPED -> R.string.mesh_state_stopped
        MeshRuntimeState.STARTING -> R.string.mesh_state_starting
        MeshRuntimeState.RUNNING -> R.string.mesh_state_running
        MeshRuntimeState.STOPPING -> R.string.mesh_state_stopping
        MeshRuntimeState.UNAVAILABLE -> R.string.mesh_state_unavailable
    }

    private companion object {
        const val CHANNEL_ID: String = "mesh_foreground"
        const val NOTIFICATION_ID: Int = 1001
        const val STOP_REQUEST_CODE: Int = 1
        const val OPEN_REQUEST_CODE: Int = 2
        const val ACTION_RESTART: String = "com.aoheyra.app.service.RESTART"
    }
}

/** Convenience for the runtime and tests that need a context-scoped launcher. */
fun Context.startMeshService() = MeshServiceLauncher.start(this)

/** Convenience for the runtime and tests that need a context-scoped stop. */
fun Context.stopMeshService() = MeshServiceLauncher.stop(this)
