package com.aoheyra.ble.core

import android.os.Handler
import android.os.HandlerThread

/** Android's Looper-backed implementation of the one serial BLE thread. */
class HandlerBleThread : BleThread {
    private val thread = HandlerThread(THREAD_NAME)
    private val handler: Handler

    init {
        thread.start()
        handler = Handler(thread.looper)
    }

    override fun post(task: Runnable): Boolean = handler.post(task)

    override fun gattCallbackHandler(): Any? = handler

    override fun close() {
        handler.removeCallbacksAndMessages(null)
        thread.quitSafely()
        thread.join(JOIN_TIMEOUT_MS)
    }

    companion object {
        private const val THREAD_NAME: String = "aoheyra-ble"
        private const val JOIN_TIMEOUT_MS: Long = 1_000L
    }
}
