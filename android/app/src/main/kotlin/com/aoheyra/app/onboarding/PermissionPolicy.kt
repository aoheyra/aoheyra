package com.aoheyra.app.onboarding

/**
 * A capability the mesh must have before any radio work may start.
 *
 * The policy is parameterised by SDK level instead of reading
 * `Build.VERSION.SDK_INT` so that the API 29, 31 and 34 rules can be tested on
 * the JVM without an emulator.
 */
enum class Capability {
    /** Scanning, advertising and connecting over Android's nearby-device permissions. */
    BLUETOOTH,

    /** Legacy BLE scanning permission on Android versions before 12. */
    LOCATION,

    /** Ongoing notification permission on Android 13 and newer. */
    NOTIFICATIONS,
}

/** Permission names shared by the manifest and the request launcher. */
object PlatformPermissions {
    const val BLUETOOTH_SCAN = "android.permission.BLUETOOTH_SCAN"
    const val BLUETOOTH_ADVERTISE = "android.permission.BLUETOOTH_ADVERTISE"
    const val BLUETOOTH_CONNECT = "android.permission.BLUETOOTH_CONNECT"
    const val ACCESS_FINE_LOCATION = "android.permission.ACCESS_FINE_LOCATION"
    const val ACCESS_COARSE_LOCATION = "android.permission.ACCESS_COARSE_LOCATION"
    const val POST_NOTIFICATIONS = "android.permission.POST_NOTIFICATIONS"
}

/** Which runtime permissions each Android version needs, and why. */
object PermissionPolicy {
    /** Android 12 introduced the nearby-device permissions. */
    const val BLUETOOTH_PERMISSIONS_SDK: Int = 31

    /** Android 13 introduced a runtime permission for notifications. */
    const val NOTIFICATION_PERMISSION_SDK: Int = 33

    /**
     * Capabilities required at [sdk]. Android 12+ uses nearby-device
     * permissions instead of location for BLE scanning; scans declare
     * `neverForLocation`, so no location permission is requested there.
     */
    fun requiredCapabilities(sdk: Int): List<Capability> {
        val capabilities = ArrayList<Capability>(2)
        if (sdk >= BLUETOOTH_PERMISSIONS_SDK) {
            capabilities.add(Capability.BLUETOOTH)
        } else {
            capabilities.add(Capability.LOCATION)
        }
        if (sdk >= NOTIFICATION_PERMISSION_SDK) {
            capabilities.add(Capability.NOTIFICATIONS)
        }
        return capabilities
    }

    /**
     * Permissions that make up [capability] at [sdk], or an empty list when
     * that capability does not need a runtime permission on this version.
     */
    fun permissionsFor(capability: Capability, sdk: Int): List<String> = when (capability) {
        Capability.BLUETOOTH ->
            if (sdk >= BLUETOOTH_PERMISSIONS_SDK) {
                listOf(
                    PlatformPermissions.BLUETOOTH_SCAN,
                    PlatformPermissions.BLUETOOTH_ADVERTISE,
                    PlatformPermissions.BLUETOOTH_CONNECT,
                )
            } else {
                emptyList()
            }

        Capability.LOCATION ->
            if (sdk < BLUETOOTH_PERMISSIONS_SDK) {
                listOf(
                    PlatformPermissions.ACCESS_FINE_LOCATION,
                    PlatformPermissions.ACCESS_COARSE_LOCATION,
                )
            } else {
                emptyList()
            }

        Capability.NOTIFICATIONS ->
            if (sdk >= NOTIFICATION_PERMISSION_SDK) {
                listOf(PlatformPermissions.POST_NOTIFICATIONS)
            } else {
                emptyList()
            }
    }

    /** Every runtime permission the onboarding flow may request at [sdk]. */
    fun runtimePermissions(sdk: Int): List<String> =
        requiredCapabilities(sdk).flatMap { permissionsFor(it, sdk) }

    /**
     * Capabilities fully granted by [grants]. A capability counts as granted
     * only when all of its permissions are granted, so a partial grant never
     * unlocks the radio.
     */
    fun grantedCapabilities(sdk: Int, grants: Map<String, Boolean>): Set<Capability> =
        requiredCapabilities(sdk).filterTo(HashSet()) { capability ->
            val permissions = permissionsFor(capability, sdk)
            permissions.isNotEmpty() && permissions.all { grants[it] == true }
        }
}
