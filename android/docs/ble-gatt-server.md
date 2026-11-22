# GATT server host (AND-07)

`AndroidGattServer` opens one `BluetoothGattServer` with the NET-03 service:
RX write-without-response, TX notify and the TX CCCD. Callbacks run on the
single BLE thread. No pairing, bonding or protocol decoding happens in Kotlin.

`GattServerHost` keeps one `InboundState` per central. MTU is recorded before
setup; a `0x0001` CCCD write asks Rust for the authoritative `LinkId` and
registers it with `LinkRegistry`. RX writes before setup, prepared writes,
non-zero offsets and a later `0x0000` CCCD write are reported as
`PeerMisbehaved` and the central is cancelled. Rust, not Kotlin, decides which
inbound links to keep; a registry safety failure cancels the new central.
A Rust-requested disconnect cancels the central with `Requested`, so the final
event reports the cause of the close rather than a protocol violation.

`NotificationPacer` holds at most eight raw ATT values per central. One value
is in flight; the next is released only from `onNotificationSent`, and
`WriteReady` is raised once per accepted send. A failed send closes the link
with a typed GATT error and clears the backlog.

JVM tests cover setup ordering, protocol violations, pacing, WriteReady
ordering, exactly-once close and registry-failure cancellation.
