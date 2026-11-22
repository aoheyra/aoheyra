package com.aoheyra.ble.sink

import com.aoheyra.ble.advertise.AdvertisePower
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.scan.ScanPower

/**
 * The `:ble` side of Rust's `NativeSink` (NET-03, PR #54).
 *
 * The generated UniFFI interface has the same five methods with `Unit` return
 * types; an APP-03 adapter maps its enums to [AdvertisePower]/[ScanPower] and
 * turns a failed [BleResult] into the generated typed exception. Keeping that
 * adapter outside `:ble` avoids a dependency from the radio module on generated
 * bindings while preserving the exact call order and argument shapes.
 */
interface NativeSink {
    /** Starts or replaces advertising, or stops it with `null`. */
    fun setAdvertising(beacon: ByteArray?, mode: AdvertisePower): BleResult<Unit>

    /** Starts, changes or stops scanning. */
    fun setScanning(mode: ScanPower?): BleResult<Unit>

    /** Opens one outbound link for a Rust-allocated id and 16-byte peer key. */
    fun connect(link: ULong, peer: ByteArray): BleResult<Unit>

    /** Sends one raw ATT value while Rust holds the link's write credit. */
    fun write(link: ULong, bytes: ByteArray): BleResult<Unit>

    /** Tears one link down in any state. */
    fun disconnect(link: ULong): BleResult<Unit>

    /** Opens the GATT server before advertising is allowed. */
    fun start(): BleResult<Unit>

    /** Stops discovery and closes links; idempotent and safe after Rust restart. */
    fun close()
}

/** The platform operations the sink needs from the GATT client factory. */
interface PeerDirectory {
    /** Records the peer for a Rust-allocated outbound link. */
    fun remember(link: com.aoheyra.ble.core.LinkId, peer: com.aoheyra.ble.core.PeerKey)

    /** Forgets a closed link so late callbacks find no peer. */
    fun forget(link: com.aoheyra.ble.core.LinkId)
}

/** The platform operations the sink needs from the GATT server. */
interface GattServerController {
    /** Opens the server and installs the NET-03 service once. */
    fun start(): BleResult<Unit>

    /** Sends one raw notification to an inbound peer. */
    fun send(peer: com.aoheyra.ble.core.PeerKey, bytes: ByteArray): BleResult<Unit>

    /** Cancels an inbound central with the final close cause. */
    fun cancel(
        peer: com.aoheyra.ble.core.PeerKey,
        status: com.aoheyra.ble.core.BleError,
    ): BleResult<Unit>

    /** Closes the server once. */
    fun stop()
}
