package com.aoheyra.ble.client

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.content.Context
import android.os.Build
import android.os.Handler
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.GattUuids
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.PeerKey
import java.util.concurrent.ConcurrentHashMap

/** Android `BluetoothGatt` implementation; all operations stay serial. */
class AndroidGattClientFactory(
    private val context: Context,
    private val bleThread: BleThread,
) : GattHandleFactory {
    private val adapter: BluetoothAdapter? = BluetoothAdapter.getDefaultAdapter()
    private val gattLinks = mutableMapOf<BluetoothGatt, LinkId>()
    private val peerByLink = ConcurrentHashMap<LinkId, PeerKey>()

    /** Set once by the BLE wiring after the client object exists. */
    var client: GattClient? = null

    /** Records the 16-byte radio peer for a Rust-allocated link. */
    fun remember(link: LinkId, peer: PeerKey) {
        peerByLink[link] = peer
    }

    /** Forgets a closed link; late callbacks then find no peer. */
    fun forget(link: LinkId) {
        peerByLink.remove(link)
    }

    override fun open(linkId: LinkId): GattHandle? {
        val peer = peerByLink[linkId] ?: return null
        val handler = bleThread.gattCallbackHandler() as? Handler ?: return null
        val device = adapter?.getRemoteDevice(peer.addressBytes()) ?: return null
        val gatt = device.connectGatt(
            context,
            false,
            gattCallback,
            BluetoothDevice.TRANSPORT_LE,
            handler,
        ) ?: return null
        gattLinks[gatt] = linkId
        return AndroidGattHandle(gatt) { gattLinks.remove(it) }
    }

    private val gattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(gatt: BluetoothGatt, status: Int, newState: Int) {
            val link = gattLinks[gatt] ?: return
            if (status == 0 && newState == BluetoothGatt.STATE_CONNECTED) {
                client?.onConnected(link, status)
            } else {
                client?.onDisconnected(link, status)
            }
        }

        override fun onServicesDiscovered(gatt: BluetoothGatt, status: Int) {
            val link = gattLinks[gatt] ?: return
            val service = gatt.getService(GattUuids.SERVICE)
            client?.onServicesDiscovered(
                link,
                status,
                service?.getCharacteristic(GattUuids.RX) != null,
                service?.getCharacteristic(GattUuids.TX) != null,
            )
        }

        override fun onMtuChanged(gatt: BluetoothGatt, mtu: Int, status: Int) {
            val link = gattLinks[gatt] ?: return
            client?.onMtuChanged(link, status, mtu)
        }

        override fun onDescriptorWrite(
            gatt: BluetoothGatt,
            descriptor: BluetoothGattDescriptor,
            status: Int,
        ) {
            val link = gattLinks[gatt] ?: return
            if (descriptor.uuid == GattUuids.CCCD) client?.onDescriptorWrite(link, status)
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            val link = gattLinks[gatt] ?: return
            if (characteristic.uuid == GattUuids.RX) client?.onCharacteristicWrite(link, status)
        }

        override fun onCharacteristicChanged(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
        ) {
            val link = gattLinks[gatt] ?: return
            if (characteristic.uuid == GattUuids.TX) {
                client?.onNotification(link, characteristic.value ?: ByteArray(0))
            }
        }
    }
}

private class AndroidGattHandle(
    private val gatt: BluetoothGatt,
    private val onClose: (BluetoothGatt) -> Unit,
) : GattHandle {
    private var closed = false

    override fun requestMtu(mtu: Int): Boolean = gatt.requestMtu(mtu)

    override fun discoverServices(): Boolean = gatt.discoverServices()

    override fun setCharacteristicNotification(enabled: Boolean): Boolean {
        val tx = txCharacteristic() ?: return false
        return gatt.setCharacteristicNotification(tx, enabled)
    }

    override fun writeDescriptor(value: ByteArray): Boolean {
        val descriptor = txCharacteristic()?.getDescriptor(GattUuids.CCCD) ?: return false
        descriptor.value = value.copyOf()
        return gatt.writeDescriptor(descriptor)
    }

    override fun writeCharacteristic(value: ByteArray): Boolean {
        val rx = rxCharacteristic() ?: return false
        rx.value = value.copyOf()
        rx.writeType = BluetoothGattCharacteristic.WRITE_TYPE_NO_RESPONSE
        return gatt.writeCharacteristic(rx)
    }

    override fun disconnect(): Boolean {
        gatt.disconnect()
        return true
    }

    override fun close() {
        if (closed) return
        closed = true
        gatt.close()
        onClose(gatt)
    }

    override fun requestConnectionPriority(priority: Int): Boolean =
        gatt.requestConnectionPriority(priority)

    override fun setPreferredPhy(tx: Int, rx: Int, options: Int) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            gatt.setPreferredPhy(tx, rx, options)
        }
    }

    private fun rxCharacteristic(): BluetoothGattCharacteristic? =
        gatt.getService(GattUuids.SERVICE)?.getCharacteristic(GattUuids.RX)

    private fun txCharacteristic(): BluetoothGattCharacteristic? =
        gatt.getService(GattUuids.SERVICE)?.getCharacteristic(GattUuids.TX)
}
