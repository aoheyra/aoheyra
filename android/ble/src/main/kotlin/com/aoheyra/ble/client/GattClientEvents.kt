package com.aoheyra.ble.client

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.LinkId

/** Events the GATT client reports back to Rust through AND-09. */
interface GattClientEvents {
    /** Central setup finished and the link has one write credit. */
    fun onConnected(link: LinkId, mtu: Int)
    /** The ATT MTU changed after setup. */
    fun onMtuChanged(link: LinkId, mtu: Int)
    /** One unmodified notification arrived on TX. */
    fun onBytes(link: LinkId, bytes: ByteArray)
    /** The stack accepted the previous write. */
    fun onWriteReady(link: LinkId)
    /** The link is gone; this is the last event for it. */
    fun onClosed(link: LinkId, status: BleError)
}

/** Ordered steps of the central procedure from NET-03. */
enum class ClientPhase {
    CONNECTING,
    DISCOVERING,
    NEGOTIATING_MTU,
    SUBSCRIBING,
    CONNECTED,
    CLOSING,
    CLOSED,
}
