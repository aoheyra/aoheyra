package com.aoheyra.ble.advertise

import android.bluetooth.BluetoothAdapter
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.BluetoothLeAdvertiser
import android.bluetooth.le.AdvertiseSettings
import android.os.ParcelUuid
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.GattUuids

/** Android implementation of the advertiser; no protocol bytes are decoded. */
class AndroidAdvertiser(
    private val events: AdvertiseEvents,
) : AdvertiseController {
    private val adapter: BluetoothAdapter? = BluetoothAdapter.getDefaultAdapter()
    private var callback: AdvertiseCallback? = null

    override fun start(
        beacon: ByteArray,
        mode: AdvertisePower,
        connectable: Boolean,
    ): BleResult<Unit> {
        val advertiser = currentAdvertiser() ?: return Result.failure(BleError.RadioUnavailable)
        callback?.let { stopQuietly(advertiser, it) }

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(mode.toAndroid())
            .setConnectable(connectable)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .build()
        val advertiseData = AdvertiseData.Builder()
            .addServiceUuid(ParcelUuid(GattUuids.SERVICE))
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .build()
        val scanResponse = AdvertiseData.Builder()
            .addManufacturerData(MANUFACTURER_ID, beacon.copyOf())
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .build()

        val next = object : AdvertiseCallback() {
            override fun onStartFailure(errorCode: Int) {
                events.onStartFailure(errorCode)
            }
        }
        // Android reports asynchronous start failures through the callback,
        // not a return value. Store it before starting, and clear it if the
        // platform refuses the call synchronously, so no failure is lost.
        callback = next
        try {
            advertiser.startAdvertising(settings, advertiseData, scanResponse, next)
        } catch (security: SecurityException) {
            callback = null
            return Result.failure(BleError.RadioUnavailable)
        }
        return Result.success(Unit)
    }

    override fun stop(): BleResult<Unit> {
        val advertiser = currentAdvertiser() ?: return Result.failure(BleError.RadioUnavailable)
        callback?.let { stopQuietly(advertiser, it) }
        callback = null
        return Result.success(Unit)
    }

    /**
     * Stopping an advertiser we no longer hold is a no-op. A revoked
     * permission means advertising is already gone, so the typed result stays
     * successful and the platform refusal is explicitly handled here.
     */
    private fun stopQuietly(advertiser: BluetoothLeAdvertiser, cb: AdvertiseCallback) {
        try {
            advertiser.stopAdvertising(cb)
        } catch (security: SecurityException) {
            Unit
        }
    }

    private fun currentAdvertiser() =
        adapter?.takeIf { it.isEnabled }?.bluetoothLeAdvertiser

    private fun AdvertisePower.toAndroid(): Int = when (this) {
        AdvertisePower.LOW_POWER -> AdvertiseSettings.ADVERTISE_MODE_LOW_POWER
        AdvertisePower.BALANCED -> AdvertiseSettings.ADVERTISE_MODE_BALANCED
        AdvertisePower.LOW_LATENCY -> AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY
    }

    companion object {
        /** Unregistered company id used for the raw beacon, from NET-03. */
        const val MANUFACTURER_ID: Int = 0xFFFF
    }
}
