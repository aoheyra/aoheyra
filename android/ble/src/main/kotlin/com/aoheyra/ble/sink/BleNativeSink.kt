package com.aoheyra.ble.sink

import com.aoheyra.ble.advertise.Advertiser
import com.aoheyra.ble.advertise.AdvertisePower
import com.aoheyra.ble.client.GattClient
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.LinkRole
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.scan.ScanPower
import com.aoheyra.ble.scan.Scanner
import java.util.concurrent.atomic.AtomicBoolean

/**
 * Kotlin implementation of Rust's `NativeSink` (NET-03, PR #54).
 *
 * Every entry point only validates arguments, updates in-memory planner state
 * and posts one bounded task to the serial BLE thread. No GATT call, lock wait,
 * I/O or callback into Rust happens on the Rust caller's thread. The single
 * serial thread preserves per-link callback order and closes links before the
 * radio-state event is forwarded.
 */
class BleNativeSink(
    private val advertiser: Advertiser,
    private val scanner: Scanner,
    private val client: GattClient,
    private val peers: PeerDirectory,
    private val server: GattServerController,
    private val executor: BoundedTaskExecutor,
    private val links: LinkRegistry,
    private val forwarder: NativeEventForwarder,
) : NativeSink {
    private val started = AtomicBoolean(false)
    private val closed = AtomicBoolean(false)

    override fun start(): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        if (started.get()) return Result.success(Unit)
        return executor.execute(Runnable {
            if (server.start().isSuccess) started.set(true)
        })
    }

    override fun setAdvertising(beacon: ByteArray?, mode: AdvertisePower): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        if (beacon != null && !started.get()) return Result.failure(BleError.NotStarted)
        return advertiser.setAdvertising(beacon, mode)
    }

    override fun setScanning(mode: ScanPower?): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        return scanner.setScanning(mode)
    }

    override fun connect(link: ULong, peer: ByteArray): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        if (link == 0UL) return Result.failure(BleError.InvalidLink)
        val peerKey = PeerKey.from(peer).getOrElse { return Result.failure(it) }
        val linkId = LinkId(link)
        if (links.contains(linkId)) return Result.failure(BleError.LinkDuplicate)

        peers.remember(linkId, peerKey)
        val posted = executor.execute(Runnable {
            if (client.connect(linkId, peerKey).isFailure) peers.forget(linkId)
        })
        if (posted.isFailure) peers.forget(linkId)
        return posted
    }

    override fun write(link: ULong, bytes: ByteArray): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        if (link == 0UL) return Result.failure(BleError.InvalidLink)
        if (bytes.size > MAX_WRITE_LENGTH) return Result.failure(BleError.InvalidWrite)
        val copy = bytes.copyOf()
        return executor.execute(Runnable { client.write(LinkId(link), copy) })
    }

    override fun disconnect(link: ULong): BleResult<Unit> {
        if (closed.get()) return Result.failure(BleError.Closed)
        if (link == 0UL) return Result.failure(BleError.InvalidLink)
        val linkId = LinkId(link)
        val entry = links.get(linkId) ?: return Result.failure(BleError.LinkUnknown)
        return when (entry.role) {
            LinkRole.CENTRAL -> executor.execute(Runnable { client.disconnect(linkId) })

            LinkRole.PERIPHERAL -> {
                val peer = entry.peer ?: return Result.failure(BleError.LinkUnknown)
                executor.execute(Runnable { server.cancel(peer, BleError.Requested) })
            }
        }
    }

    /** Reports adapter or permission changes; closes links before the event. */
    fun onRadioState(state: RadioState) {
        if (closed.get() || !forwarder.isActive()) return
        val posted = executor.execute(Runnable {
            val error = when (state) {
                RadioState.OFF -> BleError.RadioOff
                RadioState.PERMISSION_DENIED -> BleError.PermissionDenied
                RadioState.READY -> null
            }
            if (error != null) {
                closeAllLinks(error)
                advertiser.setRadioReady(false)
                scanner.setRadioReady(false)
            } else {
                server.start()
                advertiser.setRadioReady(true)
                scanner.setRadioReady(true)
            }
            forwarder.onRadioState(state)
        })
        if (posted.isFailure) {
            // A rejected event must still reach Rust; queueing is a resource
            // guard, not a policy decision about radio state.
            forwarder.onRadioState(state)
        }
    }

    private fun closeAllLinks(error: BleError) {
        for (entry in links.snapshot()) {
            when (entry.role) {
                LinkRole.CENTRAL -> client.forceClose(entry.link, error)
                LinkRole.PERIPHERAL -> {
                    val peer = entry.peer ?: continue
                    server.cancel(peer, error)
                }
            }
        }
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) return
        forwarder.close()
        val posted = executor.execute(Runnable {
            for (entry in links.snapshot()) {
                peers.forget(entry.link)
                links.remove(entry.link)
            }
            advertiser.setRadioReady(false)
            scanner.setRadioReady(false)
            server.stop()
        })
        // If the queue is full the scheduler still owns shutdown; the inactive
        // forwarder already discards every late callback.
        if (posted.isFailure) {
            for (entry in links.snapshot()) {
                peers.forget(entry.link)
                links.remove(entry.link)
            }
        }
    }

    companion object {
        /** NET-03 maximum bytes in one ATT value. */
        const val MAX_WRITE_LENGTH: Int = 514
    }
}
