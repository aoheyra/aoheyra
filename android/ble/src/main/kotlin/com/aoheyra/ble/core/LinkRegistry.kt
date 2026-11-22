package com.aoheyra.ble.core

/** Which side opened a link; both are keyed by the Rust-allocated id. */
enum class LinkRole {
    CENTRAL,
    PERIPHERAL,
}

/** Registry entry for one live link. */
data class LinkEntry(
    val link: LinkId,
    val role: LinkRole,
    val peer: PeerKey?,
)

/**
 * Tracks the links Kotlin owns.
 *
 * Rust allocates every authoritative `LinkId`: in `connect` for outbound
 * links and as the return of `on_accepted` for inbound ones. The registry
 * registers both roles but never invents an id.
 */
class LinkRegistry {
    private val lock = Any()
    private val links = LinkedHashMap<LinkId, LinkEntry>()

    /** Registers an outbound link after Rust's `connect` call. */
    fun registerCentral(link: LinkId, peer: PeerKey): BleResult<Unit> =
        register(link, LinkRole.CENTRAL, peer)

    /** Registers an inbound link after Rust's `on_accepted` return. */
    fun registerPeripheral(link: LinkId, peer: PeerKey): BleResult<Unit> =
        register(link, LinkRole.PERIPHERAL, peer)

    private fun register(link: LinkId, role: LinkRole, peer: PeerKey): BleResult<Unit> =
        synchronized(lock) {
            if (!link.isUsable()) return Result.failure(BleError.InvalidLink)
            if (links.containsKey(link)) return Result.failure(BleError.LinkDuplicate)
            if (links.size >= MAX_TRACKED_LINKS) return Result.failure(BleError.TooManyLinks)
            links[link] = LinkEntry(link, role, peer)
            Result.success(Unit)
        }

    /** Removes a link exactly once; late callbacks for it are ignored. */
    fun remove(link: LinkId): BleResult<LinkEntry> = synchronized(lock) {
        val entry = links.remove(link) ?: return Result.failure(BleError.LinkUnknown)
        Result.success(entry)
    }

    /** Looks up a live link. */
    fun get(link: LinkId): LinkEntry? = synchronized(lock) { links[link] }

    /** True while a link is registered. */
    fun contains(link: LinkId): Boolean = synchronized(lock) { links.containsKey(link) }

    /** Snapshot in registration order, for diagnostics without payload data. */
    fun snapshot(): List<LinkEntry> = synchronized(lock) { links.values.toList() }

    /** Number of registered links. */
    fun size(): Int = synchronized(lock) { links.size }

    companion object {
        /**
         * Safety bound on registry memory. Link policy and the real cap stay
         * in Rust (NET-03); this only prevents unbounded growth on bugs.
         */
        const val MAX_TRACKED_LINKS: Int = 64
    }
}
