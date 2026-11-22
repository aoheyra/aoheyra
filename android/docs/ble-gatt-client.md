# GATT client link (AND-08)

`AndroidGattClientFactory` opens one `BluetoothGatt` per outbound link with
`autoConnect = false`, `TRANSPORT_LE` and the single BLE thread's Handler.
`GattClientLink` owns that handle and follows NET-03 in order: connect,
discover RX and TX, request MTU 517, enable TX notifications, write the CCCD,
then report `onConnected` to Rust. A failed MTU request is not fatal; the
23-byte default is kept. The Rust-allocated peer map is concurrent because
`NativeSink.connect` records it off the BLE thread; `forget` clears it on close
so late callbacks find no link.

Writes use one credit from `WriteCredit`. Rust writes only while it holds the
credit and gets `WriteReady` from each successful callback. A second write is
rejected rather than buffered. The link closes on any failed GATT step and
calls `close()` exactly once, including connect failures and after
`disconnect`. `forceClose` reports an explicit cause for radio-state changes.
A 100-cycle JVM test asserts no handle is left open.

Status 133 starts a five-second cool-down through `ConnectBackoff`. Kotlin
does not retry in that window; Rust decides whether to dial again. Connection
priority is raised only while sync is active and lowered afterwards. 2M PHY
is requested where supported and never relied on.

JVM tests cover setup order, MTU fallback, write pacing, priority changes,
exactly-once close, the 100-cycle handle bound, status 133 cool-down and
missing-device refusal.
