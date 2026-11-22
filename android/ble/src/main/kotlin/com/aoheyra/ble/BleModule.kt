package com.aoheyra.ble

import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.HandlerBleThread
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.OperationQueue

/**
 * Owns the BLE primitives shared by advertiser, scanner, GATT server and
 * client. Radio APIs are confined to this module; protocol bytes stay opaque.
 */
class BleModule {
    private val bleThread = HandlerBleThread()
    private val executor = BoundedTaskExecutor(bleThread)
    private val links = LinkRegistry()

    /** Creates a per-link operation queue on the single serial thread. */
    fun newOperationQueue(): OperationQueue = OperationQueue(executor)

    /** The registry of Rust-allocated link ids. */
    fun links(): LinkRegistry = links

    /** Closes the thread and rejects all later work. */
    fun close() {
        bleThread.close()
    }
}
