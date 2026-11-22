package com.aoheyra.ble.core

/**
 * The 16-byte radio key from NET-03: six address bytes, then ten zero bytes.
 * Kotlin never interprets or deduplicates it beyond the fixed length check.
 */
class PeerKey private constructor(val bytes: ByteArray) {
    override fun equals(other: Any?): Boolean =
        other is PeerKey && bytes.contentEquals(other.bytes)

    override fun hashCode(): Int = bytes.contentHashCode()

    /** Returns a defensive copy for passing to platform APIs. */
    fun copy(): ByteArray = bytes.copyOf()

    companion object {
        /** Required length of a peer key. */
        const val LENGTH: Int = 16

        /** Validates a key received from Rust or from a scan record. */
        fun from(bytes: ByteArray): BleResult<PeerKey> =
            if (bytes.size == LENGTH) {
                Result.success(PeerKey(bytes.copyOf()))
            } else {
                Result.failure(BleError.InvalidPeer)
            }

        /** Builds a key from a six-byte LE address, most significant byte first. */
        fun fromAddress(address: ByteArray): BleResult<PeerKey> {
            if (address.size != 6) return Result.failure(BleError.InvalidPeer)
            val key = ByteArray(LENGTH)
            address.copyInto(key, 0, 0, 6)
            return Result.success(PeerKey(key))
        }
    }
}
