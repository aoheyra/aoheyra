package com.aoheyra.ble.client

import com.aoheyra.ble.core.Clock
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class ConnectBackoffTest {
    private class TestClock(var now: Long = 1_000L) : Clock {
        override fun nowMillis(): Long = now
    }

    @Test
    fun `status 133 blocks only its own peer until the cooldown expires`() {
        val clock = TestClock()
        val backoff = ConnectBackoff(clock)

        backoff.noteFailure("aa", ConnectBackoff.STATUS_133)
        assertFalse(backoff.allows("aa"))
        assertTrue(backoff.allows("bb"))

        clock.now += ConnectBackoff.COOLDOWN_MS - 1
        assertFalse(backoff.allows("aa"))
        clock.now += 1
        assertTrue(backoff.allows("aa"))
    }

    @Test
    fun `other statuses do not start a cooldown`() {
        val backoff = ConnectBackoff(TestClock())
        backoff.noteFailure("aa", 8)
        assertTrue(backoff.allows("aa"))
    }
}
