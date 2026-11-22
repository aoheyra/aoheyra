package com.aoheyra.ble.client

/** The subset of `BluetoothGatt` used by the outbound link. */
interface GattHandle {
    /** Requests the largest ATT MTU; failure is not fatal. */
    fun requestMtu(mtu: Int): Boolean

    /** Starts service discovery. */
    fun discoverServices(): Boolean

    /** Enables TX notifications locally. */
    fun setCharacteristicNotification(enabled: Boolean): Boolean

    /** Writes `0x0001` to the TX CCCD. */
    fun writeDescriptor(value: ByteArray): Boolean

    /** Writes one raw ATT value to RX without response. */
    fun writeCharacteristic(value: ByteArray): Boolean

    /** Starts a GATT disconnect. */
    fun disconnect(): Boolean

    /** Releases the handle exactly once. */
    fun close()

    /** Raises connection priority during sync. */
    fun requestConnectionPriority(priority: Int): Boolean

    /** Requests 2M PHY where supported; never required for correctness. */
    fun setPreferredPhy(tx: Int, rx: Int, options: Int)
}

/** Opens one handle per outbound link; implemented by Android only. */
interface GattHandleFactory {
    /** Opens a handle or returns null when the radio refuses the call. */
    fun open(linkId: com.aoheyra.ble.core.LinkId): GattHandle?
}
