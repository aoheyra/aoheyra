package com.aoheyra.ble.advertise

/** Advertiser duty cycle, mapped one-to-one onto Android advertise modes. */
enum class AdvertisePower {
    LOW_POWER,
    BALANCED,
    LOW_LATENCY,
}

/** A single platform action chosen by the planner. */
sealed class AdvertiseCommand {
    /** Start or replace an advertisement with a 16-byte beacon. */
    data class Start(val beacon: ByteArray, val mode: AdvertisePower, val connectable: Boolean) :
        AdvertiseCommand()

    /** Stop advertising. */
    object Stop : AdvertiseCommand()

    /** No platform call is allowed yet; the desired state is retained. */
    object Hold : AdvertiseCommand()
}
