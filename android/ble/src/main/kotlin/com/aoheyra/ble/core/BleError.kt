package com.aoheyra.ble.core

/** Typed failures produced by the BLE shim. */
sealed class BleError(message: String) : Exception(message) {
    /** The shim or the link was closed; late work is ignored. */
    object Closed : BleError("BLE shim is closed")

    /** A bounded queue is full; the work was rejected, never dropped. */
    object QueueFull : BleError("BLE queue is full")

    /** A link id is zero or otherwise unusable. */
    object InvalidLink : BleError("link id 0 is reserved")

    /** A peer key is not the 16-byte radio key from the native contract. */
    object InvalidPeer : BleError("peer key must be 16 bytes")

    /** No registry entry exists for a link. */
    object LinkUnknown : BleError("link is unknown")

    /** Rust already registered this link id. */
    object LinkDuplicate : BleError("link is already registered")

    /** Registry memory ceiling; link policy itself stays in Rust. */
    object TooManyLinks : BleError("link registry is full")

    /** An asynchronous GATT callback reported a non-success status. */
    data class Gatt(val stage: Stage, val code: Int) :
        BleError("GATT status ${stage.name} $code")

    /** A GATT call was refused synchronously by the platform. */
    data class Refused(val stage: Stage, val code: Int) :
        BleError("GATT call refused at ${stage.name} ($code)")

    /** The adapter or permission state prevents the operation. */
    object RadioUnavailable : BleError("radio is unavailable")
}

/** The GATT step a failure came from; mirrors NET-03 raw stage codes. */
enum class Stage {
    CONNECT,
    DISCOVER,
    SUBSCRIBE,
    WRITE,
}

/** Result alias used by every internal BLE operation. */
typealias BleResult<T> = Result<T>

/** Opaque link id allocated by Rust, never invented by Kotlin. */
@JvmInline
value class LinkId(val value: ULong) {
    /** Rust reserves zero for "no link". */
    companion object {
        val INVALID: LinkId = LinkId(0UL)
    }

    /** True when this can name a live link. */
    fun isUsable(): Boolean = value != 0UL
}
