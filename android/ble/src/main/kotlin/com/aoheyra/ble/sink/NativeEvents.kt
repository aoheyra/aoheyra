package com.aoheyra.ble.sink

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage

/** Adapter state reported to Rust before and after every change. */
enum class RadioState {
    /** BLE is available. */
    READY,

    /** The adapter is off. */
    OFF,

    /** A required BLE permission was revoked. */
    PERMISSION_DENIED,
}

/** Which discovery callback failed. */
enum class DiscoveryKind {
    /** `AdvertiseCallback.onStartFailure`. */
    ADVERTISE,

    /** `ScanCallback.onScanFailed`. */
    SCAN,
}

/**
 * Why Kotlin closed a link, mirroring aoNetwork's `NativeStatus` (NET-03).
 *
 * Kotlin passes these typed values, never raw strings or payload bytes.
 */
sealed class NativeStatus {
    /** Rust called `disconnect`. */
    object Requested : NativeStatus()

    /** A non-success asynchronous GATT callback. */
    data class Gatt(val stage: Stage, val code: Int) : NativeStatus()

    /** A GATT call was refused synchronously. */
    data class Refused(val stage: Stage, val code: Int) : NativeStatus()

    /** `connect` named a peer with no known device. */
    object PeerUnknown : NativeStatus()

    /** Discovery did not find RX and TX. */
    object ServiceMissing : NativeStatus()

    /** The peer broke the peripheral procedure. */
    object PeerMisbehaved : NativeStatus()

    /** The adapter turned off. */
    object RadioOff : NativeStatus()

    /** BLE permission was revoked. */
    object PermissionDenied : NativeStatus()
}

/** Maps the shim's typed errors onto the normative native status values. */
fun BleError.toNativeStatus(): NativeStatus = when (this) {
    BleError.Requested -> NativeStatus.Requested
    is BleError.Gatt -> NativeStatus.Gatt(stage, code)
    is BleError.Refused -> NativeStatus.Refused(stage, code)
    BleError.ServiceMissing -> NativeStatus.ServiceMissing
    BleError.PeerMisbehaved -> NativeStatus.PeerMisbehaved
    BleError.RadioOff -> NativeStatus.RadioOff
    BleError.PermissionDenied -> NativeStatus.PermissionDenied
    BleError.RadioUnavailable -> NativeStatus.RadioOff
    else -> NativeStatus.Requested
}

/**
 * Kotlin-to-Rust callbacks from aoNetwork's `NativeEvents` contract (NET-03).
 *
 * PR #54 exposes only Rust-to-Kotlin `NativeSink`; an APP-03 follow-up must
 * export this interface through UniFFI. Every method must return immediately:
 * Rust owns the bounded queues and allocates inbound link ids.
 */
interface NativeEvents {
    /** Adapter state at start and on every change. */
    fun onRadioState(state: RadioState)

    /** A platform discovery start failed with a raw Android error code. */
    fun onDiscoveryFailed(kind: DiscoveryKind, code: Int)

    /** One raw scan record; the beacon is never decoded in Kotlin. */
    fun onScanResult(peer: PeerKey, rssi: Short, beacon: ByteArray)

    /** Outbound central setup finished. */
    fun onConnected(link: LinkId, mtu: Int)

    /** Inbound peripheral setup finished; Rust allocates the link id. */
    fun onAccepted(peer: PeerKey, mtu: Int): LinkId

    /** MTU changed after setup. */
    fun onMtuChanged(link: LinkId, mtu: Int)

    /** One unmodified ATT value arrived. */
    fun onBytes(link: LinkId, bytes: ByteArray)

    /** The stack accepted the previous write. */
    fun onWriteReady(link: LinkId)

    /** The link is gone; this is always its last event. */
    fun onClosed(link: LinkId, status: NativeStatus)
}
