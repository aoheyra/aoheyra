package com.aoheyra.app.onboarding

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The API-level matrix from docs/android-permissions.md, checked on the JVM. */
class PermissionPolicyTest {
    @Test
    fun api29_requires_location_only() {
        assertEquals(listOf(Capability.LOCATION), PermissionPolicy.requiredCapabilities(29))
        assertEquals(
            listOf(
                PlatformPermissions.ACCESS_FINE_LOCATION,
                PlatformPermissions.ACCESS_COARSE_LOCATION,
            ),
            PermissionPolicy.runtimePermissions(29),
        )
        assertFalse(PlatformPermissions.BLUETOOTH_SCAN in PermissionPolicy.runtimePermissions(29))
        assertFalse(PlatformPermissions.POST_NOTIFICATIONS in PermissionPolicy.runtimePermissions(29))
    }

    @Test
    fun api31_requires_nearby_devices_without_location() {
        assertEquals(listOf(Capability.BLUETOOTH), PermissionPolicy.requiredCapabilities(31))
        assertEquals(
            listOf(
                PlatformPermissions.BLUETOOTH_SCAN,
                PlatformPermissions.BLUETOOTH_ADVERTISE,
                PlatformPermissions.BLUETOOTH_CONNECT,
            ),
            PermissionPolicy.runtimePermissions(31),
        )
        assertFalse(PlatformPermissions.ACCESS_FINE_LOCATION in PermissionPolicy.runtimePermissions(31))
    }

    @Test
    fun api34_adds_notifications() {
        assertEquals(
            listOf(Capability.BLUETOOTH, Capability.NOTIFICATIONS),
            PermissionPolicy.requiredCapabilities(34),
        )
        assertTrue(PlatformPermissions.POST_NOTIFICATIONS in PermissionPolicy.runtimePermissions(34))
        assertEquals(4, PermissionPolicy.runtimePermissions(34).size)
    }

    @Test
    fun partial_bluetooth_grant_does_not_unlock_the_radio() {
        val grants = mapOf(
            PlatformPermissions.BLUETOOTH_SCAN to true,
            PlatformPermissions.BLUETOOTH_ADVERTISE to true,
            PlatformPermissions.BLUETOOTH_CONNECT to false,
            PlatformPermissions.POST_NOTIFICATIONS to true,
        )
        assertEquals(
            setOf(Capability.NOTIFICATIONS),
            PermissionPolicy.grantedCapabilities(34, grants),
        )
    }

    @Test
    fun all_permissions_granted_unlocks_every_capability() {
        val grants = PermissionPolicy.runtimePermissions(34).associateWith { true }
        assertEquals(
            PermissionPolicy.requiredCapabilities(34).toSet(),
            PermissionPolicy.grantedCapabilities(34, grants),
        )
    }
}
