package com.aoheyra.ble.scan

/** Scanner duty cycle, mapped one-to-one onto Android scan modes. */
enum class ScanPower {
    LOW_POWER,
    BALANCED,
    LOW_LATENCY,
}

/** A single platform action chosen by the scan planner. */
sealed class ScanCommand {
    /** Starts or replaces scanning with a service-UUID filter. */
    data class Start(val mode: ScanPower) : ScanCommand()

    /** Stops scanning. */
    object Stop : ScanCommand()

    /** No platform call is allowed yet; the desired state is retained. */
    object Hold : ScanCommand()
}
