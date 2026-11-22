package com.aoheyra.ble.scan

import android.bluetooth.BluetoothAdapter
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.bluetooth.le.BluetoothLeScanner
import android.os.ParcelUuid
import com.aoheyra.ble.advertise.AndroidAdvertiser
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.PeerKey

/** Android implementation of the service-UUID-filtered scanner. */
class AndroidScanner(
    private val events: ScanEvents,
) : ScanController {
    private val adapter: BluetoothAdapter? = BluetoothAdapter.getDefaultAdapter()
    private var callback: ScanCallback? = null

    override fun start(mode: ScanPower): BleResult<Unit> {
        val scanner = adapter?.takeIf { it.isEnabled }?.bluetoothLeScanner
            ?: return Result.failure(BleError.RadioUnavailable)
        // The facade calls this controller on the serial BLE thread. The
        // three-argument overload delivers callbacks on that caller thread,
        // so no Handler is needed here; the four-argument overload is not
        // available at this module's compile SDK.
        callback?.let { stopQuietly(scanner, it) }

        val filter = ScanFilter.Builder()
            .setServiceUuid(ParcelUuid(AndroidAdvertiser.SERVICE_UUID))
            .build()
        val settings = ScanSettings.Builder()
            .setScanMode(mode.toAndroid())
            .setCallbackType(ScanSettings.CALLBACK_TYPE_ALL_MATCHES)
            .setMatchMode(ScanSettings.MATCH_MODE_AGGRESSIVE)
            .setNumOfMatches(ScanSettings.MATCH_NUM_ONE_ADVERTISEMENT)
            .setReportDelay(0L)
            .build()

        val next = object : ScanCallback() {
            override fun onScanFailed(errorCode: Int) {
                events.onDiscoveryFailed(errorCode)
            }

            override fun onScanResult(callbackType: Int, result: ScanResult) {
                forward(result)
            }

            override fun onBatchScanResults(results: MutableList<ScanResult>) {
                results.forEach(::forward)
            }
        }
        try {
            scanner.startScan(listOf(filter), settings, next)
        } catch (security: SecurityException) {
            return Result.failure(BleError.RadioUnavailable)
        }
        callback = next
        return Result.success(Unit)
    }

    override fun stop(): BleResult<Unit> {
        val scanner = adapter?.bluetoothLeScanner
            ?: return Result.failure(BleError.RadioUnavailable)
        callback?.let { stopQuietly(scanner, it) }
        callback = null
        return Result.success(Unit)
    }

    /** A revoked permission means scanning is already gone, so stop is a no-op. */
    private fun stopQuietly(scanner: BluetoothLeScanner, cb: ScanCallback) {
        try {
            scanner.stopScan(cb)
        } catch (security: SecurityException) {
            Unit
        }
    }

    private fun forward(result: ScanResult) {
        val address = BluetoothAddress.parse(result.device.address).getOrElse {
            return
        }
        val peer = PeerKey.fromAddress(address).getOrElse {
            return
        }
        val manufacturer = mutableMapOf<Int, ByteArray>()
        val data = result.scanRecord?.manufacturerSpecificData
        if (data != null) {
            for (index in 0 until data.size()) {
                manufacturer[data.keyAt(index)] = data.valueAt(index)
            }
        }
        ScanResultParser.forward(
            RawScan(peer = peer, rssi = result.rssi.toShort(), manufacturer = manufacturer),
            events,
        )
    }

    private fun ScanPower.toAndroid(): Int = when (this) {
        ScanPower.LOW_POWER -> ScanSettings.SCAN_MODE_LOW_POWER
        ScanPower.BALANCED -> ScanSettings.SCAN_MODE_BALANCED
        ScanPower.LOW_LATENCY -> ScanSettings.SCAN_MODE_LOW_LATENCY
    }
}
