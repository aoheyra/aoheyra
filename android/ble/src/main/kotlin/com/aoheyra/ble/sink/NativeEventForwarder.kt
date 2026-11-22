package com.aoheyra.ble.sink

import com.aoheyra.ble.advertise.AdvertiseEvents
import com.aoheyra.ble.client.GattClientEvents
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.scan.ScanEvents
import com.aoheyra.ble.server.ServerEvents
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Forwards every platform callback to Rust exactly once and in stack order.
 *
 * The GATT facades call these methods on the single BLE thread. No radio work
 * happens here: a callback may not re-enter a `NativeSink` method, and Rust
 * guarantees these calls return immediately. After [close], every callback is
 * ignored, which is how a restarted Rust runtime avoids late events.
 */
class NativeEventForwarder(
    private val links: LinkRegistry,
    private val peers: PeerDirectory,
    private val events: NativeEvents,
) : AdvertiseEvents,
    ScanEvents,
    GattClientEvents,
    ServerEvents {
    private val active = AtomicBoolean(true)

    /** False once the owning sink has been closed. */
    fun isActive(): Boolean = active.get()

    /** Stops forwarding; cleanup still happens locally. */
    fun close() {
        active.set(false)
    }

    override fun onStartFailure(code: Int) {
        if (active.get()) events.onDiscoveryFailed(DiscoveryKind.ADVERTISE, code)
    }

    override fun onDiscoveryFailed(code: Int) {
        if (active.get()) events.onDiscoveryFailed(DiscoveryKind.SCAN, code)
    }

    override fun onScanResult(peer: PeerKey, rssi: Short, beacon: ByteArray) {
        if (active.get()) events.onScanResult(peer, rssi, beacon.copyOf())
    }

    override fun onConnected(link: LinkId, mtu: Int) {
        if (active.get()) events.onConnected(link, mtu)
    }

    override fun onAccepted(peer: PeerKey, mtu: Int): LinkId {
        if (!active.get()) return LinkId.INVALID
        return events.onAccepted(peer, mtu)
    }

    override fun onMtuChanged(link: LinkId, mtu: Int) {
        if (active.get()) events.onMtuChanged(link, mtu)
    }

    override fun onBytes(link: LinkId, bytes: ByteArray) {
        if (active.get()) events.onBytes(link, bytes.copyOf())
    }

    override fun onWriteReady(link: LinkId) {
        if (active.get()) events.onWriteReady(link)
    }

    override fun onClosed(link: LinkId, status: BleError) {
        peers.forget(link)
        links.remove(link)
        if (active.get()) events.onClosed(link, status.toNativeStatus())
    }

    override fun onPeerMisbehaved(peer: PeerKey) {
        // No link id exists before CCCD setup; the platform cancels the central
        // and the host reports the terminal event through onClosed when one exists.
    }

    override fun onRegistrationFailed(peer: PeerKey, error: BleError) {
        // Registry bounds are a Kotlin safety limit; Rust owns link policy and
        // has no link id to close here. The central is cancelled by the host.
    }

    /** Reports adapter state after the sink has closed every affected link. */
    fun onRadioState(state: RadioState) {
        if (active.get()) events.onRadioState(state)
    }
}
