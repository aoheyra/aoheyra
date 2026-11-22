package com.aoheyra.app.power

/**
 * Radio and wake bounds for one battery profile. All durations are milliseconds
 * so the values map directly onto Android scan/advertise calls. Values must stay
 * in sync with android/docs/battery-budget.json; a test checks the drift.
 */
data class BatteryProfile(
    val key: String,
    val scanWindowMs: Long,
    val scanIntervalMs: Long,
    val advertiseIntervalMs: Long,
    val syncDwellMs: Long,
    val maxContinuousWakeMs: Long,
) {
    val scanDutyCycle: Double
        get() = if (scanIntervalMs == 0L) 1.0 else scanWindowMs.toDouble() / scanIntervalMs

    /** Returns the violated bounds; an empty list means the profile is usable. */
    fun validate(): List<String> = buildList {
        if (scanWindowMs <= 0L || scanIntervalMs <= 0L || advertiseIntervalMs <= 0L ||
            syncDwellMs <= 0L || maxContinuousWakeMs <= 0L
        ) {
            add("all durations must be positive")
        }
        if (scanWindowMs > scanIntervalMs) {
            add("scan window must not exceed scan interval")
        }
        if (syncDwellMs > maxContinuousWakeMs) {
            add("sync dwell must not exceed the maximum continuous wake time")
        }
        if (maxContinuousWakeMs > MAX_WAKE_MS) {
            add("continuous wake time must not exceed $MAX_WAKE_MS ms")
        }
        if (scanDutyCycle > MAX_DUTY_CYCLE) {
            add("scan duty cycle must not exceed $MAX_DUTY_CYCLE")
        }
    }

    private companion object {
        const val MAX_WAKE_MS = 60_000L
        const val MAX_DUTY_CYCLE = 0.5
    }
}

object BatteryProfiles {
    val balanced = BatteryProfile(
        key = "balanced",
        scanWindowMs = 4_000L,
        scanIntervalMs = 12_000L,
        advertiseIntervalMs = 1_000L,
        syncDwellMs = 3_000L,
        maxContinuousWakeMs = 30_000L,
    )
    val lowPower = BatteryProfile(
        key = "low_power",
        scanWindowMs = 2_000L,
        scanIntervalMs = 20_000L,
        advertiseIntervalMs = 3_000L,
        syncDwellMs = 1_500L,
        maxContinuousWakeMs = 15_000L,
    )
    val performance = BatteryProfile(
        key = "performance",
        scanWindowMs = 4_000L,
        scanIntervalMs = 8_000L,
        advertiseIntervalMs = 1_000L,
        syncDwellMs = 5_000L,
        maxContinuousWakeMs = 60_000L,
    )

    val all: List<BatteryProfile> = listOf(balanced, lowPower, performance)
    val default: BatteryProfile = balanced

    fun byKey(key: String?): BatteryProfile = all.firstOrNull { it.key == key } ?: default
}
