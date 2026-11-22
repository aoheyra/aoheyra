package com.aoheyra.app.power

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** AND-19: profiles must stay bounded and the default must be conservative. */
class BatteryProfilesTest {
    @Test
    fun allProfilesRespectTheBounds() {
        BatteryProfiles.all.forEach { profile ->
            assertEquals("${profile.key}: ${profile.validate()}", emptyList<String>(), profile.validate())
        }
    }

    @Test
    fun lowPowerIsNoMoreActiveThanBalanced() {
        val low = BatteryProfiles.lowPower
        val balanced = BatteryProfiles.balanced
        assertTrue(low.scanWindowMs <= balanced.scanWindowMs)
        assertTrue(low.scanDutyCycle <= balanced.scanDutyCycle)
        assertTrue(low.advertiseIntervalMs >= balanced.advertiseIntervalMs)
        assertTrue(low.syncDwellMs <= balanced.syncDwellMs)
        assertTrue(low.maxContinuousWakeMs <= balanced.maxContinuousWakeMs)
    }

    @Test
    fun invalidProfilesAreRejected() {
        val unbounded = BatteryProfiles.balanced.copy(
            scanWindowMs = 30_000L,
            scanIntervalMs = 1_000L,
            maxContinuousWakeMs = 120_000L,
        )
        assertTrue(unbounded.validate().size >= 3)
    }

    @Test
    fun unknownKeyFallsBackToDefault() {
        assertEquals(BatteryProfiles.default, BatteryProfiles.byKey(null))
        assertEquals(BatteryProfiles.default, BatteryProfiles.byKey("not-a-profile"))
        assertEquals(BatteryProfiles.performance, BatteryProfiles.byKey("performance"))
    }
}
