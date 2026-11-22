package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage

/**
 * Owns all outbound links and their handles.
 *
 * `connect`, writes and disconnects are called on the serial BLE thread by
 * AND-09. Every link is removed on its terminal event, and a status 133
 * failure starts a cool-down instead of an immediate retry.
 */
class GattClient(
    private val factory: GattHandleFactory,
    private val events: GattClientEvents,
    private val registry: LinkRegistry,
    private val backoff: ConnectBackoff,
) {
    private val lock = Any()
    private val links = mutableMapOf<LinkId, GattClientLink>()

    /** Opens one outbound link for a Rust-allocated id. */
    fun connect(link: LinkId, peer: PeerKey): BleResult<Unit> = synchronized(lock) {
        if (!backoff.allows(peerKeyId(peer))) return Result.failure(BleError.Backoff)
        if (links.containsKey(link)) return Result.failure(BleError.LinkDuplicate)

        val handle = factory.open(link) ?: return Result.failure(BleError.RadioUnavailable)
        val registered = registry.registerCentral(link, peer)
        if (registered.isFailure) {
            handle.close()
            return Result.failure(registered.exceptionOrNull() ?: BleError.TooManyLinks)
        }
        val outbound = GattClientLink(link, peer, handle, ForwardingEvents(peer))
        links[link] = outbound
        outbound.start()
        Result.success(Unit)
    }

    /** Sends one raw ATT value on a connected link. */
    fun write(link: LinkId, bytes: ByteArray): BleResult<Unit> =
        link(link)?.write(bytes) ?: Result.failure(BleError.LinkUnknown)

    /** Requests disconnect on a link. */
    fun disconnect(link: LinkId): BleResult<Unit> {
        val outbound = link(link) ?: return Result.failure(BleError.LinkUnknown)
        outbound.disconnect()
        return Result.success(Unit)
    }

    /** Closes a link with an explicit cause, for radio-state transitions. */
    fun forceClose(link: LinkId, status: BleError): BleResult<Unit> {
        val outbound = link(link) ?: return Result.failure(BleError.LinkUnknown)
        outbound.close(status)
        return Result.success(Unit)
    }

    /** Raises or lowers connection priority around sync. */
    fun setSyncActive(link: LinkId, active: Boolean): BleResult<Unit> {
        val outbound = link(link) ?: return Result.failure(BleError.LinkUnknown)
        outbound.setSyncActive(active)
        return Result.success(Unit)
    }

    /** Forwards a GATT connection callback. */
    fun onConnected(link: LinkId, status: Int) {
        link(link)?.onConnected(status)
    }

    /** Forwards a service-discovery callback. */
    fun onServicesDiscovered(link: LinkId, status: Int, rxFound: Boolean, txFound: Boolean) {
        link(link)?.onServicesDiscovered(status, rxFound, txFound)
    }

    /** Forwards an MTU callback. */
    fun onMtuChanged(link: LinkId, status: Int, mtu: Int) {
        link(link)?.onMtuChanged(status, mtu)
    }

    /** Forwards a CCCD write callback. */
    fun onDescriptorWrite(link: LinkId, status: Int) {
        link(link)?.onDescriptorWrite(status)
    }

    /** Forwards a characteristic write callback. */
    fun onCharacteristicWrite(link: LinkId, status: Int) {
        link(link)?.onCharacteristicWrite(status)
    }

    /** Forwards one notification. */
    fun onNotification(link: LinkId, bytes: ByteArray) {
        link(link)?.onNotification(bytes)
    }

    /** Forwards a disconnect callback. */
    fun onDisconnected(link: LinkId, status: Int) {
        link(link)?.onDisconnected(status)
    }

    /** Number of live outbound links, for resource tests. */
    fun size(): Int = synchronized(lock) { links.size }

    private fun link(link: LinkId): GattClientLink? = synchronized(lock) { links[link] }

    private fun peerKeyId(peer: PeerKey): String =
        peer.bytes.joinToString("") { byte -> "%02x".format(byte.toInt() and 0xFF) }

    private inner class ForwardingEvents(private val peer: PeerKey) : GattClientEvents {
        override fun onConnected(link: LinkId, mtu: Int) {
            events.onConnected(link, mtu)
        }

        override fun onMtuChanged(link: LinkId, mtu: Int) {
            events.onMtuChanged(link, mtu)
        }

        override fun onBytes(link: LinkId, bytes: ByteArray) {
            events.onBytes(link, bytes)
        }

        override fun onWriteReady(link: LinkId) {
            events.onWriteReady(link)
        }

        override fun onClosed(link: LinkId, status: BleError) {
            synchronized(lock) { links.remove(link) }
            registry.remove(link)
            if (status is BleError.Gatt && status.stage == Stage.CONNECT) {
                backoff.noteFailure(peerKeyId(peer), status.code)
            }
            events.onClosed(link, status)
        }
    }
}
