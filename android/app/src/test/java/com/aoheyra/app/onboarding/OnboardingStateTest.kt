package com.aoheyra.app.onboarding

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class OnboardingStateTest {
    private fun status(
        capability: Capability,
        granted: Boolean = false,
        requestedBefore: Boolean = true,
        rationale: Boolean = false,
    ) = CapabilityStatus(capability, granted, requestedBefore, rationale)

    private fun state(
        sdk: Int,
        capabilities: List<CapabilityStatus>,
        bluetooth: Boolean? = true,
        location: Boolean? = true,
        battery: Boolean? = true,
    ) = OnboardingState(sdk, capabilities, SystemStatus(bluetooth, location, battery))

    @Test
    fun first_ask_explains_before_requesting() {
        val state = state(
            34,
            listOf(
                status(Capability.BLUETOOTH, requestedBefore = false),
                status(Capability.NOTIFICATIONS, granted = true),
            ),
        )
        assertFalse(state.ready)
        val issue = state.issues().first { it.capability == Capability.BLUETOOTH }
        assertEquals(IssueReason.FIRST_EXPLANATION, issue.reason)
        assertEquals(RecoveryAction.RequestPermissions(Capability.BLUETOOTH), issue.action)
    }

    @Test
    fun denial_with_rationale_offers_retry() {
        val state = state(34, listOf(status(Capability.BLUETOOTH, rationale = true)))
        val issue = state.issues().first { it.capability == Capability.BLUETOOTH }
        assertEquals(IssueReason.DENIED_WITH_RATIONALE, issue.reason)
        assertEquals(RecoveryAction.RequestPermissions(Capability.BLUETOOTH), issue.action)
    }

    @Test
    fun permanent_denial_offers_app_settings() {
        val state = state(34, listOf(status(Capability.NOTIFICATIONS, rationale = false)))
        val issue = state.issues().first { it.capability == Capability.NOTIFICATIONS }
        assertEquals(IssueReason.PERMANENTLY_DENIED, issue.reason)
        assertEquals(RecoveryAction.OpenAppSettings, issue.action)
    }

    @Test
    fun bluetooth_off_or_unknown_blocks_readiness_and_offers_settings() {
        for (value in listOf(null, false)) {
            val state = state(34, listOf(status(Capability.BLUETOOTH, granted = true)), bluetooth = value)
            assertFalse(state.ready)
            assertTrue(
                state.issues().any {
                    it.reason == IssueReason.BLUETOOTH_OFF &&
                        it.action == RecoveryAction.OpenBluetoothSettings
                },
            )
        }
    }

    @Test
    fun location_services_matter_on_api29_but_not_api34() {
        val old = state(
            29,
            listOf(status(Capability.LOCATION, granted = true)),
            location = false,
        )
        assertFalse(old.ready)
        assertTrue(old.issues().any { it.reason == IssueReason.LOCATION_OFF })

        val current = state(
            34,
            listOf(
                status(Capability.BLUETOOTH, granted = true),
                status(Capability.NOTIFICATIONS, granted = true),
            ),
            location = false,
        )
        assertTrue(current.ready)
        assertFalse(current.issues().any { it.reason == IssueReason.LOCATION_OFF })
    }

    @Test
    fun battery_optimisation_is_advisory_and_never_blocks_readiness() {
        val state = state(
            34,
            listOf(
                status(Capability.BLUETOOTH, granted = true),
                status(Capability.NOTIFICATIONS, granted = true),
            ),
            battery = false,
        )
        assertTrue(state.ready)
        assertTrue(
            state.issues().any {
                it.reason == IssueReason.BATTERY_OPTIMIZED &&
                    it.action == RecoveryAction.OpenBatterySettings
            },
        )
    }

    @Test
    fun revocation_brings_the_recovery_card_back() {
        val state = state(
            34,
            listOf(
                status(Capability.BLUETOOTH, granted = false, rationale = true),
                status(Capability.NOTIFICATIONS, granted = true),
            ),
        )
        assertFalse(state.ready)
        assertEquals(1, state.issues().size)
    }

    @Test
    fun card_count_is_bounded() {
        val state = state(
            34,
            listOf(
                status(Capability.BLUETOOTH),
                status(Capability.NOTIFICATIONS),
                status(Capability.LOCATION),
            ),
            bluetooth = false,
            location = false,
            battery = false,
        )
        assertEquals(OnboardingState.MAX_ISSUES, state.issues().size)
    }
}
