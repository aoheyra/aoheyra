package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage

/**
 * One outbound link.
 *
 * The handle is owned here and closed exactly once on every path. The central
 * procedure follows NET-03: connect, discover, request MTU, subscribe, then
 * report `onConnected` with one write credit.
 */
class GattClientLink(
    val link: LinkId,
    val peer: PeerKey,
    private val handle: GattHandle,
    private val events: GattClientEvents,
) {
    private val credit = WriteCredit()
    private var phase = ClientPhase.CONNECTING
    private var mtu = MIN_MTU
    private var closed = false

    /** Requests 2M PHY where supported; never required for correctness. */
    fun start() {
        handle.setPreferredPhy(PHY_2M, PHY_2M, PHY_OPTION_NO_PREFERENCE)
    }

    /** Current setup phase, for tests and diagnostics. */
    fun phase(): ClientPhase = phase

    /** Negotiated ATT MTU, or 23 until setup completes. */
    fun mtu(): Int = mtu

    /** Handles the GATT connection callback. */
    fun onConnected(status: Int) {
        if (status != 0) {
            close(BleError.Gatt(Stage.CONNECT, status))
            return
        }
        phase = ClientPhase.DISCOVERING
        if (!handle.discoverServices()) {
            close(BleError.Refused(Stage.DISCOVER, -1))
        }
    }

    /** Handles service discovery; missing RX or TX ends the link. */
    fun onServicesDiscovered(status: Int, rxFound: Boolean, txFound: Boolean) {
        if (status != 0) {
            close(BleError.Gatt(Stage.DISCOVER, status))
            return
        }
        if (!rxFound || !txFound) {
            close(BleError.ServiceMissing)
            return
        }
        phase = ClientPhase.NEGOTIATING_MTU
        if (!handle.requestMtu(MAX_MTU)) {
            subscribe()
        }
    }

    /** Handles the MTU callback; a failure keeps the 23-byte default. */
    fun onMtuChanged(status: Int, newMtu: Int) {
        if (status == 0 && newMtu in MIN_MTU..MAX_MTU) mtu = newMtu
        subscribe()
    }

    /** Handles the CCCD write callback. */
    fun onDescriptorWrite(status: Int) {
        if (status != 0) {
            close(BleError.Gatt(Stage.SUBSCRIBE, status))
            return
        }
        phase = ClientPhase.CONNECTED
        credit.release()
        events.onConnected(link, mtu)
    }

    /** Handles a write-without-response callback. */
    fun onCharacteristicWrite(status: Int) {
        if (status != 0) {
            close(BleError.Gatt(Stage.WRITE, status))
            return
        }
        credit.release()
        events.onWriteReady(link)
    }

    /** Forwards one raw TX notification. */
    fun onNotification(bytes: ByteArray) {
        if (phase == ClientPhase.CONNECTED) {
            events.onBytes(link, bytes.copyOf())
        }
    }

    /** Sends one raw ATT value only while holding the write credit. */
    fun write(bytes: ByteArray): BleResult<Unit> {
        if (phase != ClientPhase.CONNECTED) return Result.failure(BleError.LinkUnknown)
        val acquired = credit.acquire()
        if (acquired.isFailure) return acquired
        if (!handle.writeCharacteristic(bytes.copyOf())) {
            close(BleError.Refused(Stage.WRITE, -1))
            return Result.failure(BleError.Refused(Stage.WRITE, -1))
        }
        return Result.success(Unit)
    }

    /** Raises or lowers connection priority around sync. */
    fun setSyncActive(active: Boolean) {
        if (phase == ClientPhase.CONNECTED) {
            val priority = if (active) PRIORITY_HIGH else PRIORITY_BALANCED
            handle.requestConnectionPriority(priority)
        }
    }

    /** Requests disconnect; the handle is closed on the disconnect callback. */
    fun disconnect() {
        if (closed || phase == ClientPhase.CLOSING) return
        phase = ClientPhase.CLOSING
        if (!handle.disconnect()) {
            close(BleError.Refused(Stage.CONNECT, -1))
        }
    }

    /** Handles a disconnect callback and closes the handle exactly once. */
    fun onDisconnected(status: Int) {
        val error = if (status == 0) {
            BleError.Requested
        } else {
            BleError.Gatt(Stage.CONNECT, status)
        }
        close(error)
    }

    /** Closes the handle and emits the terminal event exactly once. */
    fun close(status: BleError) {
        if (closed) return
        closed = true
        phase = ClientPhase.CLOSED
        credit.clear()
        handle.close()
        events.onClosed(link, status)
    }

    private fun subscribe() {
        phase = ClientPhase.SUBSCRIBING
        val enabled = handle.setCharacteristicNotification(true)
        val written = enabled && handle.writeDescriptor(CCCD_ENABLE)
        if (!written) {
            close(BleError.Refused(Stage.SUBSCRIBE, -1))
        }
    }

    companion object {
        /** First valid ATT MTU. */
        const val MIN_MTU: Int = 23

        /** Largest ATT MTU. */
        const val MAX_MTU: Int = 517

        /** Enable notifications in the CCCD little-endian value. */
        val CCCD_ENABLE: ByteArray = byteArrayOf(0x01, 0x00)

        private const val PHY_2M: Int = 2
        private const val PHY_OPTION_NO_PREFERENCE: Int = 0
        private const val PRIORITY_HIGH: Int = 1
        private const val PRIORITY_BALANCED: Int = 0
    }
}
