package com.aoheyra.app

import android.bluetooth.BluetoothManager
import android.content.ActivityNotFoundException
import android.content.Intent
import android.content.pm.PackageManager
import android.location.LocationManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.Surface
import androidx.compose.runtime.MutableState
import androidx.compose.runtime.mutableStateOf
import androidx.compose.ui.Modifier
import com.aoheyra.app.onboarding.Capability
import com.aoheyra.app.onboarding.CapabilityStatus
import com.aoheyra.app.onboarding.Notice
import com.aoheyra.app.onboarding.OnboardingScreen
import com.aoheyra.app.onboarding.OnboardingState
import com.aoheyra.app.onboarding.PermissionPolicy
import com.aoheyra.app.onboarding.RecoveryAction
import com.aoheyra.app.onboarding.SystemStatus
import com.aoheyra.app.ui.AoHeyraTheme
import com.aoheyra.app.ui.CoreScreen

/**
 * Hosts the permission onboarding gate. Every permission and system setting is
 * re-read on resume, so a revocation while the app was in the background
 * immediately puts the recovery card back on screen.
 */
class MainActivity : ComponentActivity() {
    private val requestedCapabilities = mutableSetOf<Capability>()
    private val notice = mutableStateOf<Notice?>(null)

    private val permissionLauncher =
        registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
            // Grants are re-read from the OS in refresh(); the result itself is
            // not cached, so it cannot disagree with a later revocation.
            refresh()
        }

    private lateinit var state: MutableState<OnboardingState>

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        state = mutableStateOf(snapshot())
        setContent {
            AoHeyraTheme {
                Surface(modifier = Modifier.fillMaxSize()) {
                    val current = state.value
                    if (current.ready) {
                        CoreScreen()
                    } else {
                        OnboardingScreen(current, ::handleAction)
                    }
                }
            }
        }
    }

    override fun onResume() {
        super.onResume()
        refresh()
    }

    private fun refresh() {
        state.value = snapshot()
    }

    /** Rebuilds the whole screen state from the OS; nothing is accumulated. */
    private fun snapshot(): OnboardingState {
        val sdk = Build.VERSION.SDK_INT
        val permissions = PermissionPolicy.runtimePermissions(sdk)
        val grants = HashMap<String, Boolean>(permissions.size)
        for (permission in permissions) {
            grants[permission] = checkSelfPermission(permission) == PackageManager.PERMISSION_GRANTED
        }

        val capabilities = PermissionPolicy.requiredCapabilities(sdk).map { capability ->
            CapabilityStatus(
                capability = capability,
                granted = isGranted(capability, sdk, grants),
                requestedBefore = capability in requestedCapabilities,
                canShowRationale = PermissionPolicy.permissionsFor(capability, sdk)
                    .any { shouldShowRequestPermissionRationale(it) },
            )
        }

        return OnboardingState(
            sdk = sdk,
            capabilities = capabilities,
            system = SystemStatus(
                bluetoothEnabled = bluetoothEnabled(
                    sdk,
                    isGranted(Capability.BLUETOOTH, sdk, grants),
                ),
                locationEnabled = locationEnabled(
                    isGranted(Capability.LOCATION, sdk, grants),
                ),
                batteryExempt = batteryExempt(),
            ),
            notice = notice.value,
        )
    }

    private fun isGranted(
        capability: Capability,
        sdk: Int,
        grants: Map<String, Boolean>,
    ): Boolean {
        val permissions = PermissionPolicy.permissionsFor(capability, sdk)
        return permissions.isNotEmpty() && permissions.all { grants[it] == true }
    }

    private fun bluetoothEnabled(sdk: Int, granted: Boolean): Boolean? {
        // isEnabled() itself needs BLUETOOTH_CONNECT on Android 12+.
        if (sdk >= PermissionPolicy.BLUETOOTH_PERMISSIONS_SDK && !granted) return null
        val manager = getSystemService(BluetoothManager::class.java) ?: return null
        val adapter = manager.adapter ?: return null
        return try {
            adapter.isEnabled
        } catch (_: SecurityException) {
            null
        }
    }

    private fun locationEnabled(granted: Boolean): Boolean? {
        if (!granted) return null
        val manager = getSystemService(LocationManager::class.java) ?: return null
        return try {
            manager.isProviderEnabled(LocationManager.GPS_PROVIDER)
        } catch (_: SecurityException) {
            null
        }
    }

    private fun batteryExempt(): Boolean? {
        val manager = getSystemService(PowerManager::class.java) ?: return null
        return manager.isIgnoringBatteryOptimizations(packageName)
    }

    private fun handleAction(action: RecoveryAction) {
        notice.value = null
        when (action) {
            is RecoveryAction.RequestPermissions -> {
                requestedCapabilities.add(action.capability)
                val permissions = PermissionPolicy.permissionsFor(action.capability, Build.VERSION.SDK_INT)
                permissionLauncher.launch(permissions.toTypedArray())
            }

            RecoveryAction.OpenAppSettings ->
                openSettings(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, appUri())

            RecoveryAction.OpenBluetoothSettings ->
                openSettings(Settings.ACTION_BLUETOOTH_SETTINGS, null)

            RecoveryAction.OpenLocationSettings ->
                openSettings(Settings.ACTION_LOCATION_SOURCE_SETTINGS, null)

            RecoveryAction.OpenBatterySettings ->
                openSettings(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, appUri())
        }
    }

    private fun appUri(): Uri = Uri.fromParts("package", packageName, null)

    private fun openSettings(action: String, data: Uri?) {
        val intent = Intent(action)
        if (data != null) intent.data = data
        try {
            startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            notice.value = Notice.SETTINGS_UNAVAILABLE
            refresh()
        }
    }
}
