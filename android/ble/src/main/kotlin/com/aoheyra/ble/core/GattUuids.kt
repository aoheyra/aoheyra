package com.aoheyra.ble.core

import java.util.UUID

/** GATT layout from aoNetwork's native contract (NET-03). */
object GattUuids {
    /** Primary service in advertising data and both GATT roles. */
    val SERVICE: UUID = UUID.fromString("4c5b0001-108f-4105-a600-5a82e0fd7470")

    /** Peripheral RX: central writes without response. */
    val RX: UUID = UUID.fromString("4c5b0002-108f-4105-a600-5a82e0fd7470")

    /** Peripheral TX: notifications to the central. */
    val TX: UUID = UUID.fromString("4c5b0003-108f-4105-a600-5a82e0fd7470")

    /** Client characteristic configuration descriptor. */
    val CCCD: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
}
