package com.aoheyra.ble.server

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey

/** Events the GATT server reports back to Rust through AND-09. */
interface ServerEvents {
    /** Inbound setup finished; Rust returns the authoritative link id. */
    fun onAccepted(peer: PeerKey, mtu: Int): LinkId
    /** One unmodified ATT value arrived on RX. */
    fun onBytes(link: LinkId, bytes: ByteArray)
    /** The stack accepted the previous notification. */
    fun onWriteReady(link: LinkId)
    /** The link is gone; this is the last event for it. */
    fun onClosed(link: LinkId, status: BleError)
    /** The central broke the peripheral procedure. */
    fun onPeerMisbehaved(peer: PeerKey)
    /** Registry safety bound reached; the platform must cancel this central. */
    fun onRegistrationFailed(peer: PeerKey, error: BleError)
}

/** The next platform notification, if any. */
sealed class NotificationSend {
    /** Nothing to send on the serial thread. */
    object None : NotificationSend()

    /** Notify this peer with these raw bytes. */
    data class Send(val peer: PeerKey, val bytes: ByteArray) : NotificationSend()
}

/**
 * Platform-independent GATT server state.
 *
 * Kotlin accepts every central; Rust closes ones it does not want. This host
 * only keeps per-central state and enforces one outstanding notification,
 * with WriteReady raised after each accepted send.
 */
class GattServerHost(
    private val events: ServerEvents,
    private val links: LinkRegistry,
) {
    private val lock = Any()
    private val states = mutableMapOf<PeerKey, InboundState>()

    private class InboundState(val peer: PeerKey) {
        var mtu: Int = MIN_MTU
        var subscribed: Boolean = false
        var link: LinkId? = null
        val pacer = NotificationPacer()
    }

    /** Records a new central connection. */
    fun onCentralConnected(peer: PeerKey): BleResult<Unit> = synchronized(lock) {
        if (states.containsKey(peer)) return Result.failure(BleError.LinkDuplicate)
        states[peer] = InboundState(peer)
        Result.success(Unit)
    }

    /** Records the negotiated MTU before inbound setup completes. */
    fun onMtuChanged(peer: PeerKey, mtu: Int): BleResult<Unit> = synchronized(lock) {
        val state = states[peer] ?: return Result.failure(BleError.LinkUnknown)
        if (mtu !in MIN_MTU..MAX_MTU) return Result.failure(BleError.InvalidMtu)
        state.mtu = mtu
        Result.success(Unit)
    }

    /**
     * Handles a TX CCCD write.
     *
     * A value of 1 completes setup and asks Rust for a link id. A value of 0
     * after setup is a protocol violation under NET-03.
     */
    fun onCccdWritten(peer: PeerKey, value: Int): BleResult<Unit> = synchronized(lock) {
        val state = states[peer] ?: return Result.failure(BleError.LinkUnknown)
        when (value) {
            CCCD_ENABLE -> {
                if (state.subscribed) return Result.success(Unit)
                val link = events.onAccepted(peer, state.mtu)
                if (!link.isUsable()) {
                    states.remove(peer)
                    return Result.failure(BleError.InvalidLink)
                }
                val registered = links.registerPeripheral(link, peer)
                if (registered.isFailure) {
                    val error = registered.exceptionOrNull() as? BleError
                        ?: BleError.TooManyLinks
                    states.remove(peer)
                    events.onRegistrationFailed(peer, error)
                    return registered
                }
                state.link = link
                state.subscribed = true
                Result.success(Unit)
            }

            CCCD_DISABLE -> {
                if (state.subscribed) {
                    val link = state.link
                    states.remove(peer)
                    if (link != null) {
                        links.remove(link)
                        events.onClosed(link, BleError.PeerMisbehaved)
                    }
                    events.onPeerMisbehaved(peer)
                }
                Result.success(Unit)
            }

            else -> Result.failure(BleError.InvalidMtu)
        }
    }

    /** Handles one RX write; prepared writes and offsets violate NET-03. */
    fun onRxWrite(
        peer: PeerKey,
        bytes: ByteArray,
        preparedWrite: Boolean,
        offset: Int,
    ): BleResult<Unit> = synchronized(lock) {
        val state = states[peer] ?: return Result.failure(BleError.LinkUnknown)
        val link = state.link
        if (preparedWrite || offset != 0 || !state.subscribed || link == null) {
            states.remove(peer)
            events.onPeerMisbehaved(peer)
            return Result.failure(BleError.LinkUnknown)
        }
        events.onBytes(link, bytes.copyOf())
        Result.success(Unit)
    }

    /** Buffers one raw notification and returns the first value to send. */
    fun enqueueNotification(link: LinkId, bytes: ByteArray): BleResult<NotificationSend> =
        synchronized(lock) {
            val state = stateForLink(link) ?: return Result.failure(BleError.LinkUnknown)
            val queued = state.pacer.enqueue(bytes)
            if (queued.isFailure) return Result.failure(queued.exceptionOrNull() ?: BleError.QueueFull)
            Result.success(nextSendLocked(state))
        }

    /** Handles one `onNotificationSent` callback. */
    fun onNotificationSent(peer: PeerKey, success: Boolean, code: Int): NotificationSend =
        synchronized(lock) {
            val state = states[peer] ?: return NotificationSend.None
            when (val outcome = state.pacer.onSent(success, code)) {
                PacerOutcome.Idle -> NotificationSend.None
                is PacerOutcome.Failed -> {
                    val link = state.link
                    states.remove(peer)
                    if (link != null) {
                        links.remove(link)
                        events.onClosed(link, outcome.error)
                    }
                    NotificationSend.None
                }

                PacerOutcome.WriteReady -> {
                    state.link?.let(events::onWriteReady)
                    NotificationSend.None
                }

                is PacerOutcome.SendNext -> {
                    state.link?.let(events::onWriteReady)
                    NotificationSend.Send(peer, outcome.bytes)
                }
            }
        }

    /** Reports a disconnect and removes all state for the central. */
    fun onCentralDisconnected(peer: PeerKey, status: BleError): BleResult<Unit> =
        synchronized(lock) {
            val state = states.remove(peer) ?: return Result.failure(BleError.LinkUnknown)
            state.pacer.clear()
            val link = state.link
            if (link != null) {
                links.remove(link)
                events.onClosed(link, status)
            }
            Result.success(Unit)
        }

    private fun stateForLink(link: LinkId): InboundState? =
        states.values.firstOrNull { it.link == link }

    private fun nextSendLocked(state: InboundState): NotificationSend {
        val bytes = state.pacer.next() ?: return NotificationSend.None
        return NotificationSend.Send(state.peer, bytes)
    }

    companion object {
        /** First valid ATT MTU. */
        const val MIN_MTU: Int = 23

        /** Largest ATT MTU. */
        const val MAX_MTU: Int = 517

        private const val CCCD_ENABLE: Int = 0x0001
        private const val CCCD_DISABLE: Int = 0x0000
    }
}
