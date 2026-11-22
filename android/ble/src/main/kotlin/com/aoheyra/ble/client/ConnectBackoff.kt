package com.aoheyra.ble.client

import com.aoheyra.ble.core.Clock

/**
 * Cool-down after Android status 133.
 *
 * Kotlin never retries on its own; it refuses an immediate `connect` and
 * lets Rust decide whether to dial again after the cooldown.
 */
class ConnectBackoff(private val clock: Clock) {
    private val untilByPeer = mutableMapOf<String, Long>()

    /** True when a new connect for this peer may start. */
    fun allows(peerKey: String): Boolean {
        val until = untilByPeer[peerKey] ?: return true
        return clock.nowMillis() >= until
    }

    /** Records a failure; only status 133 starts a cooldown. */
    fun noteFailure(peerKey: String, code: Int) {
        if (code != STATUS_133) return
        untilByPeer[peerKey] = clock.nowMillis() + COOLDOWN_MS
    }

    /** Forgets a peer when its link is fully closed. */
    fun forget(peerKey: String) {
        untilByPeer.remove(peerKey)
    }

    companion object {
        /** Android's GATT_ERROR / connection-limit family code. */
        const val STATUS_133: Int = 133

        /** Cooldown long enough to avoid hammering a saturated controller. */
        const val COOLDOWN_MS: Long = 5_000L
    }
}
