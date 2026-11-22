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

    /** A beacon is not the 16-byte value required by the native contract. */
    object InvalidBeacon : BleError("beacon must be 16 bytes")

    /** An ATT MTU is outside the 23..517 range from the native contract. */
    object InvalidMtu : BleError("MTU must be between 23 and 517")

    /** A write is longer than the 514-byte native contract limit. */
    object InvalidWrite : BleError("write must be at most 514 bytes")

    /** The remote central broke the peripheral procedure in NET-03. */
    object PeerMisbehaved : BleError("peer violated the GATT procedure")

    /** Rust or the peer requested the close. */
    object Requested : BleError("link close requested")

    /** A status 133 cooldown is active; Rust may retry later. */
    object Backoff : BleError("connect attempt is in backoff")

    /** Discovery did not find RX and TX. */
    object ServiceMissing : BleError("GATT service is missing")

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

    /** The adapter turned off; the final close cause for every radio link. */
    object RadioOff : BleError("radio is off")

    /** BLE permission was revoked; the final close cause for every radio link. */
    object PermissionDenied : BleError("BLE permission is denied")

    /** The sink must be started before Rust-driven radio calls. */
    object NotStarted : BleError("BLE sink is not started")
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
