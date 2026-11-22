package com.aoheyra.ble.scan

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult

/** Parses a colon-separated LE address into NET-03's six most-significant-byte-first bytes. */
object BluetoothAddress {
    /** Returns six bytes or a typed error for any malformed address. */
    fun parse(address: String): BleResult<ByteArray> {
        val parts = address.split(':')
        if (parts.size != 6) return Result.failure(BleError.InvalidPeer)
        val bytes = ByteArray(6)
        for (index in parts.indices) {
            val part = parts[index]
            if (part.length != 2) return Result.failure(BleError.InvalidPeer)
            val value = part.toIntOrNull(16) ?: return Result.failure(BleError.InvalidPeer)
            bytes[index] = value.toByte()
        }
        return Result.success(bytes)
    }
}
