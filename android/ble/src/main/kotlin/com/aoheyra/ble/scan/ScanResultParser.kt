package com.aoheyra.ble.scan

import com.aoheyra.ble.core.PeerKey

/** Platform-independent view of one scan record. */
data class RawScan(
    val peer: PeerKey,
    val rssi: Short,
    val manufacturer: Map<Int, ByteArray>,
)

/**
 * Passes the raw manufacturer payload to Rust without decoding it.
 *
 * NET-03 allows an empty beacon when a record has no `0xFFFF` data and
 * requires records longer than the legacy advertising limit to be dropped.
 */
object ScanResultParser {
    /** Unregistered company id used by the native contract. */
    const val COMPANY_ID: Int = 0xFFFF

    /** Longest manufacturer payload legacy advertising can carry. */
    const val MAX_BEACON_LENGTH: Int = 31

    /** Forwards one scan result; returns false when the record is unusable. */
    fun forward(raw: RawScan, events: ScanEvents): Boolean {
        val beacon = raw.manufacturer[COMPANY_ID] ?: ByteArray(0)
        if (beacon.size > MAX_BEACON_LENGTH) return false
        events.onScanResult(raw.peer, raw.rssi, beacon.copyOf())
        return true
    }
}
