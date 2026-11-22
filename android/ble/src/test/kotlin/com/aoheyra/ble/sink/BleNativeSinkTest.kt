package com.aoheyra.ble.sink

import com.aoheyra.ble.advertise.AdvertiseController
import com.aoheyra.ble.advertise.AdvertisePlanner
import com.aoheyra.ble.advertise.AdvertisePower
import com.aoheyra.ble.advertise.Advertiser
import com.aoheyra.ble.client.ConnectBackoff
import com.aoheyra.ble.client.GattClient
import com.aoheyra.ble.client.GattHandle
import com.aoheyra.ble.client.GattHandleFactory
import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.BleResult
import com.aoheyra.ble.core.BleThread
import com.aoheyra.ble.core.BoundedTaskExecutor
import com.aoheyra.ble.core.Clock
import com.aoheyra.ble.core.LinkId
import com.aoheyra.ble.core.LinkRegistry
import com.aoheyra.ble.core.PeerKey
import com.aoheyra.ble.scan.ScanController
import com.aoheyra.ble.scan.ScanPlanner
import com.aoheyra.ble.scan.ScanPower
import com.aoheyra.ble.scan.Scanner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BleNativeSinkTest {
    private class ManualThread : BleThread {
        private val pending = ArrayDeque<Runnable>()

        override fun post(task: Runnable): Boolean {
            pending.addLast(task)
            return true
        }

        override fun gattCallbackHandler(): Any? = null

        override fun close() = Unit

        fun drain() {
            while (pending.isNotEmpty()) pending.removeFirst().run()
        }
    }

    private class TestClock(var now: Long = 1_000L) : Clock {
        override fun nowMillis(): Long = now
    }

    private class AdvertiseRecorder : AdvertiseController {
        data class Start(val beacon: ByteArray, val mode: AdvertisePower, val connectable: Boolean)

        val starts = mutableListOf<Start>()
        var stops = 0

        override fun start(beacon: ByteArray, mode: AdvertisePower, connectable: Boolean): BleResult<Unit> {
            starts += Start(beacon.copyOf(), mode, connectable)
            return Result.success(Unit)
        }

        override fun stop(): BleResult<Unit> {
            stops += 1
            return Result.success(Unit)
        }
    }

    private class ScanRecorder : ScanController {
        val modes = mutableListOf<ScanPower>()
        var stops = 0

        override fun start(mode: ScanPower): BleResult<Unit> {
            modes += mode
            return Result.success(Unit)
        }

        override fun stop(): BleResult<Unit> {
            stops += 1
            return Result.success(Unit)
        }
    }

    private class PeerRecorder : PeerDirectory {
        val remembered = mutableListOf<Pair<LinkId, PeerKey>>()
        val forgotten = mutableListOf<LinkId>()

        override fun remember(link: LinkId, peer: PeerKey) {
            remembered += link to peer
        }

        override fun forget(link: LinkId) {
            forgotten += link
        }
    }

    private class ServerRecorder : GattServerController {
        var starts = 0
        var stops = 0
        val cancels = mutableListOf<Pair<PeerKey, BleError>>()

        override fun start(): BleResult<Unit> {
            starts += 1
            return Result.success(Unit)
        }

        override fun send(peer: PeerKey, bytes: ByteArray): BleResult<Unit> = Result.success(Unit)

        override fun cancel(peer: PeerKey, status: BleError): BleResult<Unit> {
            cancels += peer to status
            return Result.success(Unit)
        }

        override fun stop() {
            stops += 1
        }
    }

    private class HandleRecorder : GattHandle {
        val writes = mutableListOf<ByteArray>()
        var disconnects = 0
        var closes = 0

        override fun requestMtu(mtu: Int): Boolean = true
        override fun discoverServices(): Boolean = true
        override fun setCharacteristicNotification(enabled: Boolean): Boolean = true
        override fun writeDescriptor(value: ByteArray): Boolean = true

        override fun writeCharacteristic(value: ByteArray): Boolean {
            writes += value.copyOf()
            return true
        }

        override fun disconnect(): Boolean {
            disconnects += 1
            return true
        }

        override fun close() {
            closes += 1
        }

        override fun requestConnectionPriority(priority: Int): Boolean = true
        override fun setPreferredPhy(tx: Int, rx: Int, options: Int) = Unit
    }

    private class EventRecorder : NativeEvents {
        val calls = mutableListOf<String>()

        override fun onRadioState(state: RadioState) {
            calls += "radio:${state.name}"
        }

        override fun onDiscoveryFailed(kind: DiscoveryKind, code: Int) {
            calls += "failed:${kind.name}:$code"
        }

        override fun onScanResult(peer: PeerKey, rssi: Short, beacon: ByteArray) {
            calls += "scan:${beacon.size}"
        }

        override fun onConnected(link: LinkId, mtu: Int) {
            calls += "connected:${link.value}"
        }

        override fun onAccepted(peer: PeerKey, mtu: Int): LinkId {
            calls += "accepted"
            return LinkId(99UL)
        }

        override fun onMtuChanged(link: LinkId, mtu: Int) {
            calls += "mtu:${link.value}:$mtu"
        }

        override fun onBytes(link: LinkId, bytes: ByteArray) {
            calls += "bytes:${link.value}:${bytes.size}"
        }

        override fun onWriteReady(link: LinkId) {
            calls += "ready:${link.value}"
        }

        override fun onClosed(link: LinkId, status: NativeStatus) {
            calls += "closed:${link.value}:${status.javaClass.simpleName}"
        }
    }

    private class Fixture(capacity: Int = BoundedTaskExecutor.DEFAULT_CAPACITY) {
        val thread = ManualThread()
        val executor = BoundedTaskExecutor(thread, capacity)
        val clock = TestClock()
        val links = LinkRegistry()
        val events = EventRecorder()
        val peers = PeerRecorder()
        val advertise = AdvertiseRecorder()
        val scan = ScanRecorder()
        val server = ServerRecorder()
        val handle = HandleRecorder()
        val forwarder: NativeEventForwarder
        val advertiser: Advertiser
        val scanner: Scanner
        val client: GattClient
        val sink: BleNativeSink

        init {
            forwarder = NativeEventForwarder(links, peers, events)
            advertiser = Advertiser(AdvertisePlanner(forwarder, clock), advertise, executor)
            scanner = Scanner(ScanPlanner(forwarder, clock), scan, executor)
            client = GattClient(
                object : GattHandleFactory {
                    override fun open(linkId: LinkId): GattHandle = handle
                },
                forwarder,
                links,
                ConnectBackoff(clock),
            )
            sink = BleNativeSink(
                advertiser = advertiser,
                scanner = scanner,
                client = client,
                peers = peers,
                server = server,
                executor = executor,
                links = links,
                forwarder = forwarder,
            )
        }

        fun started() {
            assertTrue(sink.start().isSuccess)
            thread.drain()
        }
    }

    private fun peer(): ByteArray = ByteArray(PeerKey.LENGTH) { index -> (index + 1).toByte() }

    private fun beacon(): ByteArray = ByteArray(16) { 0x2A }

    private fun Fixture.connectedLink(link: ULong = 1UL): LinkId {
        val linkId = LinkId(link)
        assertTrue(sink.connect(link, peer()).isSuccess)
        thread.drain()
        client.onConnected(linkId, 0)
        client.onServicesDiscovered(linkId, 0, true, true)
        client.onMtuChanged(linkId, 0, 517)
        client.onDescriptorWrite(linkId, 0)
        return linkId
    }

    @Test
    fun `sink methods post work and never call Rust from the caller thread`() {
        val fixture = Fixture()
        fixture.started()

        assertTrue(fixture.sink.setAdvertising(beacon(), AdvertisePower.LOW_POWER).isSuccess)
        assertTrue(fixture.sink.connect(1UL, peer()).isSuccess)

        assertEquals(0, fixture.advertise.starts.size)
        assertEquals(0, fixture.handle.writes.size)
        assertEquals(emptyList<String>(), fixture.events.calls)

        fixture.thread.drain()
        assertEquals(1, fixture.advertise.starts.size)
        assertEquals(0, fixture.handle.writes.size)
        assertEquals(emptyList<String>(), fixture.events.calls)

        assertTrue(fixture.sink.write(1UL, byteArrayOf(0x01, 0x02)).isSuccess)
        assertTrue(fixture.sink.disconnect(1UL).isSuccess)
        assertEquals(0, fixture.handle.writes.size)
        assertEquals(0, fixture.handle.disconnects)

        fixture.thread.drain()
        assertEquals(1, fixture.handle.writes.size)
        assertEquals(1, fixture.handle.disconnects)
        assertEquals(emptyList<String>(), fixture.events.calls)
    }

    @Test
    fun `advertise and scan modes are mapped one to one`() {
        val advertiseFixture = Fixture()
        advertiseFixture.started()
        assertTrue(
            advertiseFixture.sink.setAdvertising(beacon(), AdvertisePower.LOW_LATENCY).isSuccess,
        )
        advertiseFixture.thread.drain()
        assertEquals(AdvertisePower.LOW_LATENCY, advertiseFixture.advertise.starts.single().mode)

        val scanFixture = Fixture()
        scanFixture.started()
        assertTrue(scanFixture.sink.setScanning(ScanPower.BALANCED).isSuccess)
        scanFixture.thread.drain()
        assertEquals(listOf(ScanPower.BALANCED), scanFixture.scan.modes)
    }

    @Test
    fun `central callbacks reach Rust in stack order with exactly one close`() {
        val fixture = Fixture()
        val link = fixture.connectedLink()
        assertEquals(listOf("connected:${link.value}"), fixture.events.calls)

        assertTrue(fixture.sink.write(link.value, byteArrayOf(0x01, 0x02)).isSuccess)
        fixture.thread.drain()
        fixture.client.onNotification(link, byteArrayOf(0x03, 0x04, 0x05))
        fixture.client.onCharacteristicWrite(link, 0)
        fixture.client.onDisconnected(link, 0)
        fixture.client.onDisconnected(link, 0)

        assertEquals(
            listOf(
                "connected:${link.value}",
                "bytes:${link.value}:3",
                "ready:${link.value}",
                "closed:${link.value}:Requested",
            ),
            fixture.events.calls,
        )
        assertEquals(1, fixture.handle.closes)
    }

    @Test
    fun `radio off closes every link before forwarding the state`() {
        val fixture = Fixture()
        val link = fixture.connectedLink()

        fixture.sink.onRadioState(RadioState.OFF)
        fixture.thread.drain()

        assertEquals(
            listOf(
                "connected:${link.value}",
                "closed:${link.value}:RadioOff",
                "radio:OFF",
            ),
            fixture.events.calls,
        )
        assertEquals(1, fixture.server.starts)
        assertEquals(0, fixture.server.stops)
    }

    @Test
    fun `late callbacks are ignored after close and accepted returns no link`() {
        val fixture = Fixture()
        val link = fixture.connectedLink()
        fixture.sink.close()
        fixture.thread.drain()
        val countAtClose = fixture.events.calls.size

        fixture.client.onNotification(link, byteArrayOf(0x01))
        fixture.client.onMtuChanged(link, 0, 100)
        fixture.forwarder.onClosed(link, BleError.Requested)
        fixture.forwarder.onScanResult(
            PeerKey.from(peer()).getOrThrow(),
            0,
            beacon(),
        )
        fixture.forwarder.onRadioState(RadioState.OFF)
        fixture.forwarder.onStartFailure(1)
        fixture.forwarder.onDiscoveryFailed(2)

        assertEquals(countAtClose, fixture.events.calls.size)
        assertEquals(LinkId.INVALID, fixture.forwarder.onAccepted(
            PeerKey.from(peer()).getOrThrow(),
            100,
        ))
    }

    @Test
    fun `rejecting unstarted closed zero-id and oversized requests is typed`() {
        val unstarted = Fixture()
        assertEquals(
            BleError.NotStarted,
            unstarted.sink.setAdvertising(beacon(), AdvertisePower.BALANCED).exceptionOrNull(),
        )

        val fixture = Fixture()
        fixture.started()
        assertEquals(
            BleError.InvalidWrite,
            fixture.sink.write(1UL, ByteArray(BleNativeSink.MAX_WRITE_LENGTH + 1)).exceptionOrNull(),
        )
        assertEquals(BleError.InvalidLink, fixture.sink.connect(0UL, peer()).exceptionOrNull())

        fixture.sink.close()
        assertEquals(BleError.Closed, fixture.sink.write(1UL, byteArrayOf(0x01)).exceptionOrNull())
        assertEquals(BleError.Closed, fixture.sink.connect(1UL, peer()).exceptionOrNull())
    }

    @Test
    fun `a full bounded executor rejects new work instead of dropping it`() {
        val fixture = Fixture(capacity = 1)
        assertTrue(fixture.sink.start().isSuccess)
        assertEquals(BleError.QueueFull, fixture.sink.setScanning(ScanPower.LOW_POWER).exceptionOrNull())
    }
}
