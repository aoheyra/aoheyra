# NativeSink implementation (AND-09)

`BleNativeSink` is the `:ble` side of aoNetwork's `NativeSink` (NET-03). It ties
the advertiser, scanner, GATT server and GATT client together behind the five
Rust-to-Kotlin calls from PR #54: `set_advertising`, `set_scanning`, `connect`,
`write` and `disconnect`.

Every entry point validates its arguments, updates planner state and posts at
most one bounded task to the single BLE thread. No GATT call, lock wait, I/O or
callback into Rust runs on the Rust caller's thread. The GATT server is opened
by `start()` before advertising is permitted. One write remains in flight per
link because the client owns the single write credit; a second write is
rejected rather than buffered. `disconnect` resolves the link's role from the
registry and uses a Requested close for either GATT role.

`NativeEventForwarder` implements all four platform event interfaces and
forwards each callback exactly once, in stack order, to the `NativeEvents`
interface that mirrors NET-03. Per-link events arrive in this order:
`on_connected`/`on_accepted`, then `on_bytes`/`on_write_ready`/`on_mtu_changed`
in callback order, then exactly one `on_closed`. Adapter state and permission
changes close every link first and only then raise `on_radio_state`. Bytes are
copied but never decoded in Kotlin, and no payload or key is logged.

`close()` is idempotent and marks the forwarder inactive before cleanup, so a
Rust restart can create a fresh sink without receiving late callbacks from the
old one. `onAccepted` returns link id 0 after close, matching the contract.

## APP-03 blocker

PR #54 exposes only Rust-to-Kotlin `NativeSink`. It does **not** expose
Kotlin-to-Rust `NativeEvents` or the `on_*` callbacks required by NET-03; a
search of the generated `com.aoheyra.core` bindings confirms this. `:ble`
therefore defines `NativeEvents` locally and `BleModule` requires an injected
implementation. Until APP-03 issue #5 exports that interface through UniFFI,
the application cannot construct the module and the two-phone Ready link
acceptance test cannot be run. The generated `NativeSink` adapter will map its
`AdvertiseMode`/`ScanMode` enums to the identically named `AdvertisePower` and
`ScanPower` values and turn a failed `BleResult` into the generated typed
exception. No generated bindings are committed or hand-edited.

## Tests

JVM tests cover immediate return, mode mapping, ordered callback forwarding,
exactly-once close, radio-off ordering, late-callback suppression, typed
argument rejection and bounded-executor overflow. The two-phone Ready link and
all real-radio behaviour remain untested locally: no Android SDK, Gradle or
physical phones are available, and the APP-03 callback surface is missing.
