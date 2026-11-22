package com.aoheyra.ble.server

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.content.Context
import android.os.Handler
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.GattUuids
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage
import com.aoheyra.ble.scan.BluetoothAddress

/** Android GATT server host; all callbacks run on the single BLE thread. */
class AndroidGattServer(
    context: Context,
    private val host: GattServerHost,
    private val bleThread: BleThread,
) {
    private val manager: BluetoothManager? =
        context.getSystemService(BluetoothManager::class.java)
    private val devices = mutableMapOf<PeerKey, BluetoothDevice>()
    private var server: BluetoothGattServer? = null
    private var tx: BluetoothGattCharacteristic? = null
    private val callback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            val peer = peerOf(device) ?: return
            if (status == BluetoothGatt.GATT_SUCCESS &&
                newState == BluetoothGatt.STATE_CONNECTED
            ) {
                devices[peer] = device
                host.onCentralConnected(peer)
                return
            }
            devices.remove(peer)
            val error = if (status == BluetoothGatt.GATT_SUCCESS) {
                BleError.Requested
            } else {
                BleError.Gatt(Stage.CONNECT, status)
            }
            host.onCentralDisconnected(peer, error)
        }

        override fun onMtuChanged(device: BluetoothDevice, mtu: Int, status: Int) {
            val peer = peerOf(device) ?: return
            if (status != BluetoothGatt.GATT_SUCCESS) {
                devices.remove(peer)
                host.onCentralDisconnected(peer, BleError.Gatt(Stage.DISCOVER, status))
                return
            }
            host.onMtuChanged(peer, mtu)
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            val peer = peerOf(device) ?: return
            val accepted = characteristic.uuid == GattUuids.RX &&
                host.onRxWrite(peer, value, preparedWrite, offset).isSuccess
            val status = if (accepted) {
                BluetoothGatt.GATT_SUCCESS
            } else {
                BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
            }
            if (responseNeeded) {
                server?.sendResponse(device, requestId, status, offset, null)
            }
            if (!accepted) {
                cancel(device, peer, BleError.PeerMisbehaved)
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            descriptor: BluetoothGattDescriptor,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            val peer = peerOf(device) ?: return
            val accepted = descriptor.uuid == GattUuids.CCCD &&
                !preparedWrite &&
                offset == 0 &&
                value.size >= 2 &&
                host.onCccdWritten(peer, cccdValue(value)).isSuccess
            val status = if (accepted) {
                BluetoothGatt.GATT_SUCCESS
            } else {
                BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
            }
            if (responseNeeded) {
                server?.sendResponse(device, requestId, status, offset, null)
            }
            if (!accepted) {
                cancel(device, peer, BleError.PeerMisbehaved)
            }
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            val peer = peerOf(device) ?: return
            when (
                val result = host.onNotificationSent(
                    peer,
                    status == BluetoothGatt.GATT_SUCCESS,
                    status,
                )
            ) {
                NotificationSend.None -> Unit
                is NotificationSend.Send -> {
                    val characteristic = tx
                    val gattServer = server
                    if (characteristic != null && gattServer != null) {
                        characteristic.value = result.bytes
                        gattServer.notifyCharacteristicChanged(device, characteristic, false)
                    }
                }
            }
        }
    }

    /** Opens the GATT server and installs the NET-03 service once. */
    fun start(): BleResult<Unit> {
        if (server != null) return Result.success(Unit)
        val adapter = BluetoothAdapter.getDefaultAdapter()
        if (adapter?.isEnabled != true) return Result.failure(BleError.RadioUnavailable)
        val handler = bleThread.gattCallbackHandler() as? Handler
            ?: return Result.failure(BleError.RadioUnavailable)
        val opened = manager?.openGattServer(context, callback, handler)
            ?: return Result.failure(BleError.RadioUnavailable)
        val service = BluetoothGattService(GattUuids.SERVICE, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        val rx = BluetoothGattCharacteristic(
            GattUuids.RX,
            BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE,
        )
        val notifyCharacteristic = BluetoothGattCharacteristic(
            GattUuids.TX,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            0,
        )
        notifyCharacteristic.addDescriptor(
            BluetoothGattDescriptor(
                GattUuids.CCCD,
                BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE,
            ),
        )
        service.addCharacteristic(rx)
        service.addCharacteristic(notifyCharacteristic)
        if (!opened.addService(service)) {
            opened.close()
            return Result.failure(BleError.RadioUnavailable)
        }
        server = opened
        tx = notifyCharacteristic
        return Result.success(Unit)
    }

    /** Sends one raw notification; pacing is enforced by the host. */
    fun send(peer: PeerKey, bytes: ByteArray): BleResult<Unit> {
        val device = devices[peer] ?: return Result.failure(BleError.LinkUnknown)
        val characteristic = tx ?: return Result.failure(BleError.RadioUnavailable)
        val gattServer = server ?: return Result.failure(BleError.RadioUnavailable)
        characteristic.value = bytes.copyOf()
        return if (gattServer.notifyCharacteristicChanged(device, characteristic, false)) {
            Result.success(Unit)
        } else {
            Result.failure(BleError.Refused(Stage.WRITE, -1))
        }
    }

    /** Reports a Rust disconnect as a cancel for an inbound central. */
    fun cancel(peer: PeerKey, status: BleError = BleError.Requested): BleResult<Unit> {
        val device = devices[peer] ?: return Result.failure(BleError.LinkUnknown)
        cancel(device, peer, status)
        return Result.success(Unit)
    }

    /** Closes the server once; late callbacks find no device. */
    fun stop() {
        server?.close()
        server = null
        tx = null
        devices.clear()
    }

    private fun cancel(device: BluetoothDevice, peer: PeerKey, status: BleError) {
        server?.cancelConnection(device)
        devices.remove(peer)
        host.onCentralDisconnected(peer, status)
    }

    private fun peerOf(device: BluetoothDevice): PeerKey? {
        val address = BluetoothAddress.parse(device.address).getOrElse { return null }
        return PeerKey.fromAddress(address).getOrElse { return null }
    }

    private fun cccdValue(value: ByteArray): Int =
        (value[0].toInt() and 0xFF) or ((value[1].toInt() and 0xFF) shl 8)
}
