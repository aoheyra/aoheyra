package com.aoheyra.ble.server

import com.aoheyra.ble.core.BleError
import com.aoheyra.ble.core.Stage
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class NotificationPacerTest {
    @Test
    fun `next notification waits for the send callback`() {
        val pacer = NotificationPacer()
        val first = byteArrayOf(1, 2)
        val second = byteArrayOf(3, 4)

        assertTrue(pacer.enqueue(first).isSuccess)
        assertTrue(pacer.enqueue(second).isSuccess)
        assertTrue(pacer.next()?.contentEquals(first) == true)
        assertNull(pacer.next())

        val outcome = pacer.onSent(true, 0)
        assertTrue(outcome is PacerOutcome.SendNext)
        val sendNext = outcome as PacerOutcome.SendNext
        assertTrue(sendNext.bytes.contentEquals(second))
        assertTrue(sendNext.writeReady)
        assertEquals(PacerOutcome.WriteReady, pacer.onSent(true, 0))
    }

    @Test
    fun `bounded queue rejects excess notifications`() {
        val pacer = NotificationPacer(capacity = 2)
        assertTrue(pacer.enqueue(byteArrayOf(1)).isSuccess)
        assertTrue(pacer.enqueue(byteArrayOf(2)).isSuccess)
        assertEquals(BleError.QueueFull, pacer.enqueue(byteArrayOf(3)).exceptionOrNull())
    }

    @Test
    fun `failed send clears the backlog with a typed error`() {
        val pacer = NotificationPacer()
        pacer.enqueue(byteArrayOf(1))
        pacer.enqueue(byteArrayOf(2))
        pacer.next()

        val outcome = pacer.onSent(false, 133)
        assertTrue(outcome is PacerOutcome.Failed)
        assertEquals(BleError.Gatt(Stage.WRITE, 133), (outcome as PacerOutcome.Failed).error)
        assertEquals(0, pacer.pendingSize())
        assertNull(pacer.next())
    }
}
