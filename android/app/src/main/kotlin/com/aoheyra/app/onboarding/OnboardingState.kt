package com.aoheyra.app.onboarding

/** Permission state for one capability, derived from the OS on every resume. */
data class CapabilityStatus(
    val capability: Capability,
    val granted: Boolean,
    /** True once the app has asked for this capability, to tell a first ask from a block. */
    val requestedBefore: Boolean,
    /** Whether Android will show a rationale instead of the system dialog. */
    val canShowRationale: Boolean,
)

/**
 * System settings that affect the radio. `null` means "could not be read",
 * which is rendered as an actionable unknown rather than as a grant.
 */
data class SystemStatus(
    val bluetoothEnabled: Boolean? = null,
    val locationEnabled: Boolean? = null,
    val batteryExempt: Boolean? = null,
)

/** A bounded, one-off message; no OS or user data is kept in it. */
enum class Notice {
    SETTINGS_UNAVAILABLE,
}

/** What the UI offers to do about an issue. */
sealed interface RecoveryAction {
    data class RequestPermissions(val capability: Capability) : RecoveryAction
    data object OpenAppSettings : RecoveryAction
    data object OpenBluetoothSettings : RecoveryAction
    data object OpenLocationSettings : RecoveryAction
    data object OpenBatterySettings : RecoveryAction
}

/** Why a recovery card is on screen. */
enum class IssueReason {
    FIRST_EXPLANATION,
    DENIED_WITH_RATIONALE,
    PERMANENTLY_DENIED,
    BLUETOOTH_OFF,
    LOCATION_OFF,
    BATTERY_OPTIMIZED,
}

/** One bounded recovery card. */
data class RecoveryIssue(
    val reason: IssueReason,
    val action: RecoveryAction,
    /** Capability the card is about, or null for a system-level card. */
    val capability: Capability? = null,
)

/**
 * Immutable projection of OS state. The list is derived from a fixed policy
 * and is never accumulated: at most three capabilities and three system
 * checks can produce a card.
 */
data class OnboardingState(
    val sdk: Int,
    val capabilities: List<CapabilityStatus>,
    val system: SystemStatus,
    val notice: Notice? = null,
) {
    /** Whether the radio may be started. An unknown system state is not a grant. */
    val ready: Boolean
        get() {
            if (capabilities.any { !it.granted }) return false
            if (system.bluetoothEnabled != true) return false
            if (Capability.LOCATION in capabilities.map { it.capability } &&
                system.locationEnabled != true
            ) {
                return false
            }
            return true
        }

    /**
     * Recovery cards in a stable order. A capability that has never been
     * asked for gets an explanation first; a denial with a rationale gets a
     * retry; a permanent denial gets a path into Android app settings.
     */
    fun issues(): List<RecoveryIssue> {
        val issues = ArrayList<RecoveryIssue>(MAX_ISSUES)
        for (status in capabilities) {
            if (status.granted) continue
            val reason = when {
                !status.requestedBefore -> IssueReason.FIRST_EXPLANATION
                status.canShowRationale -> IssueReason.DENIED_WITH_RATIONALE
                else -> IssueReason.PERMANENTLY_DENIED
            }
            val action = if (reason == IssueReason.PERMANENTLY_DENIED) {
                RecoveryAction.OpenAppSettings
            } else {
                RecoveryAction.RequestPermissions(status.capability)
            }
            issues.add(RecoveryIssue(reason, action, status.capability))
        }
        if (system.bluetoothEnabled != true) {
            issues.add(
                RecoveryIssue(
                    IssueReason.BLUETOOTH_OFF,
                    RecoveryAction.OpenBluetoothSettings,
                    Capability.BLUETOOTH,
                ),
            )
        }
        if (Capability.LOCATION in capabilities.map { it.capability } &&
            system.locationEnabled != true
        ) {
            issues.add(
                RecoveryIssue(
                    IssueReason.LOCATION_OFF,
                    RecoveryAction.OpenLocationSettings,
                    Capability.LOCATION,
                ),
            )
        }
        // Battery optimisation is guidance, not a permission: it never blocks ready.
        if (system.batteryExempt == false) {
            issues.add(RecoveryIssue(IssueReason.BATTERY_OPTIMIZED, RecoveryAction.OpenBatterySettings))
        }
        return issues
    }

    companion object {
        /** Three capabilities plus three system checks. */
        const val MAX_ISSUES: Int = 6
    }
}
