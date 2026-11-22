package com.aoheyra.ble

import android.content.Context
import com.aoheyra.ble.advertise.AdvertisePlanner
import com.aoheyra.ble.advertise.Advertiser
import com.aoheyra.ble.advertise.AndroidAdvertiser
import com.aoheyra.ble.client.AndroidGattClientFactory
import com.aoheyra.ble.client.ConnectBackoff
import com.aoheyra.ble.client.GattClient
import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.Clock
import com.aoheyra.ble.core.HandlerBleThread
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.OperationQueue
import com.aoheyra.ble.core.SystemClock
import com.aoheyra.ble.scan.AndroidScanner
import com.aoheyra.ble.scan.ScanPlanner
import com.aoheyra.ble.scan.Scanner
import com.aoheyra.ble.server.AndroidGattServer
import com.aoheyra.ble.server.GattServerHost
import com.aoheyra.ble.sink.BleNativeSink
import com.aoheyra.ble.sink.NativeEventForwarder
import com.aoheyra.ble.sink.NativeEvents
import com.aoheyra.ble.sink.NativeSink

/**
 * Owns the BLE primitives and the Rust-facing sink.
 *
 * [events] is the Kotlin view of aoNetwork's `NativeEvents`. APP-03 must export
 * it before the application can construct this module; until then the missing
 * seam is intentional and documented in `android/docs/ble-native-sink.md`.
 */
class BleModule(
    context: Context,
    events: NativeEvents,
    clock: Clock = SystemClock(),
) {
    private val bleThread = HandlerBleThread()
    private val executor = BoundedTaskExecutor(bleThread)
    private val links = LinkRegistry()
    private val clientFactory = AndroidGattClientFactory(context, bleThread)
    private val forwarder = NativeEventForwarder(links, clientFactory, events)

    private val advertiser = Advertiser(
        AdvertisePlanner(forwarder, clock),
        AndroidAdvertiser(forwarder),
        executor,
    )
    private val scanner = Scanner(
        ScanPlanner(forwarder, clock),
        AndroidScanner(forwarder),
        executor,
    )
    private val client = GattClient(clientFactory, forwarder, links, ConnectBackoff(clock))
    private val serverHost = GattServerHost(forwarder, links)
    private val server = AndroidGattServer(context, serverHost, bleThread)

    /** The single Rust-facing BLE sink; `start` must be called before discovery. */
    val sink: NativeSink = BleNativeSink(
        advertiser = advertiser,
        scanner = scanner,
        client = client,
        peers = clientFactory,
        server = server,
        executor = executor,
        links = links,
        forwarder = forwarder,
    )

    init {
        clientFactory.client = client
    }

    /** Creates a per-link operation queue on the single serial thread. */
    fun newOperationQueue(): OperationQueue = OperationQueue(executor)

    /** The registry of Rust-allocated link ids. */
    fun links(): LinkRegistry = links

    /** Closes the thread and rejects all later work. */
    fun close() {
        sink.close()
        bleThread.close()
    }
}
