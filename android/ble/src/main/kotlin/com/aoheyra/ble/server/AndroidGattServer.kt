package com.aoheyra.ble.server

import android.Manifest
import android.annotation.SuppressLint
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
import android.content.pm.PackageManager
import android.os.Build
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.GattUuids
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.core.Stage
import com.aoheyra.ble.scan.BluetoothAddress

/**
 * Android GATT server host; all callbacks run on the single BLE thread.
 *
 * Lint cannot follow BLUETOOTH_CONNECT checks through the private platform
 * helpers below. Every call is gated by `checkSelfPermission` on API 31+ and
 * also handles `SecurityException` if the permission is revoked between the
 * check and the call, so the suppression is limited to this class.
 */
@SuppressLint("MissingPermission")
class AndroidGattServer(
    context: Context,
    private val host: GattServerHost,
    private val bleThread: BleThread,
) {
    // Keep only the application context beyond construction; the platform
    // needs it to open the server, while holding the activity would leak it.
    private val appContext: Context = context.applicationContext
    private val manager: BluetoothManager? =
        appContext.getSystemService(BluetoothManager::class.java)
    private val devices = mutableMapOf<PeerKey, BluetoothDevice>()
    private var server: BluetoothGattServer? = null
    private var tx: BluetoothGattCharacteristic? = null
    private val callback = object : BluetoothGattServerCallback() {
        // openGattServer's two-argument form calls back on a platform thread.
        // Re-post every callback so host state is touched only on the BLE thread.
        override fun onConnectionStateChange(device: BluetoothDevice, status: Int, newState: Int) {
            bleThread.post(Runnable {
                val peer = peerOf(device) ?: return@Runnable
                if (status == BluetoothGatt.GATT_SUCCESS &&
                    newState == BluetoothGatt.STATE_CONNECTED
                ) {
                    devices[peer] = device
                    host.onCentralConnected(peer)
                    return@Runnable
                }
                devices.remove(peer)
                val error = if (status == BluetoothGatt.GATT_SUCCESS) {
                    BleError.Requested
                } else {
                    BleError.Gatt(Stage.CONNECT, status)
                }
                host.onCentralDisconnected(peer, error)
            })
        }

        override fun onMtuChanged(device: BluetoothDevice, mtu: Int) {
            bleThread.post(Runnable {
                val peer = peerOf(device) ?: return@Runnable
                host.onMtuChanged(peer, mtu)
            })
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
            bleThread.post(Runnable {
                val peer = peerOf(device) ?: return@Runnable
                val accepted = characteristic.uuid == GattUuids.RX &&
                    host.onRxWrite(peer, value, preparedWrite, offset).isSuccess
                val status = if (accepted) {
                    BluetoothGatt.GATT_SUCCESS
                } else {
                    BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED
                }
                if (responseNeeded) {
                    respond(device, requestId, status, offset)
                }
                if (!accepted) {
                    cancel(device, peer, BleError.PeerMisbehaved)
                }
            })
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
            bleThread.post(Runnable {
                val peer = peerOf(device) ?: return@Runnable
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
                    respond(device, requestId, status, offset)
                }
                if (!accepted) {
                    cancel(device, peer, BleError.PeerMisbehaved)
                }
            })
        }

        override fun onNotificationSent(device: BluetoothDevice, status: Int) {
            bleThread.post(Runnable {
                val peer = peerOf(device) ?: return@Runnable
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
                            notifyQuietly(gattServer, device, characteristic, result.bytes)
                        }
                    }
                }
            })
        }
    }

    /** Opens the GATT server and installs the NET-03 service once. */
    fun start(): BleResult<Unit> {
        if (server != null) return Result.success(Unit)
        val adapter = BluetoothAdapter.getDefaultAdapter()
        if (adapter?.isEnabled != true) return Result.failure(BleError.RadioUnavailable)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            appContext.checkSelfPermission(
                Manifest.permission.BLUETOOTH_CONNECT,
            ) != PackageManager.PERMISSION_GRANTED
        ) return Result.failure(BleError.RadioUnavailable)
        val bluetoothManager = manager ?: return Result.failure(BleError.RadioUnavailable)
        val opened: BluetoothGattServer
        try {
            opened = bluetoothManager.openGattServer(appContext, callback)
                ?: return Result.failure(BleError.RadioUnavailable)
        } catch (security: SecurityException) {
            return Result.failure(BleError.RadioUnavailable)
        }
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
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            appContext.checkSelfPermission(
                Manifest.permission.BLUETOOTH_CONNECT,
            ) != PackageManager.PERMISSION_GRANTED
        ) return Result.failure(BleError.RadioUnavailable)
        var added: Boolean
        try {
            added = opened.addService(service)
        } catch (security: SecurityException) {
            added = false
        }
        if (!added) {
            closeQuietly(opened)
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
        return if (notifyQuietly(gattServer, device, characteristic, bytes)) {
            Result.success(Unit)
        } else {
            Result.failure(BleError.Refused(Stage.WRITE, -1))
        }
    }

    /** A revoked permission leaves no live link to notify; report refusal. */
    private fun notifyQuietly(
        gattServer: BluetoothGattServer,
        device: BluetoothDevice,
        characteristic: BluetoothGattCharacteristic,
        bytes: ByteArray,
    ): Boolean {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            appContext.checkSelfPermission(
                Manifest.permission.BLUETOOTH_CONNECT,
            ) != PackageManager.PERMISSION_GRANTED
        ) return false
        characteristic.value = bytes.copyOf()
        var delivered: Boolean
        try {
            delivered = gattServer.notifyCharacteristicChanged(device, characteristic, false)
        } catch (security: SecurityException) {
            return false
        }
        return delivered
    }

    /** The peer already timed out if the response cannot be delivered. */
    private fun respond(
        device: BluetoothDevice,
        requestId: Int,
        status: Int,
        offset: Int,
    ) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            if (appContext.checkSelfPermission(
                    Manifest.permission.BLUETOOTH_CONNECT,
                ) != PackageManager.PERMISSION_GRANTED
            ) return
        }
        val gattServer = server ?: return
        try {
            gattServer.sendResponse(device, requestId, status, offset, null)
        } catch (security: SecurityException) {
            Unit
        }
    }

    private fun closeQuietly(gattServer: BluetoothGattServer) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            appContext.checkSelfPermission(
                Manifest.permission.BLUETOOTH_CONNECT,
            ) != PackageManager.PERMISSION_GRANTED
        ) return
        try {
            gattServer.close()
        } catch (security: SecurityException) {
            Unit
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
        val gattServer = server
        server = null
        tx = null
        devices.clear()
        if (gattServer != null) closeQuietly(gattServer)
    }

    private fun cancel(device: BluetoothDevice, peer: PeerKey, status: BleError) {
        val gattServer = server
        if (gattServer != null &&
            (Build.VERSION.SDK_INT < Build.VERSION_CODES.S ||
                appContext.checkSelfPermission(
                    Manifest.permission.BLUETOOTH_CONNECT,
                ) == PackageManager.PERMISSION_GRANTED)
        ) {
            try {
                gattServer.cancelConnection(device)
            } catch (security: SecurityException) {
                Unit
            }
        }
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
