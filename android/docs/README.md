# Android documentation

- [BLE core](ble-core.md) - serial thread, per-link operation queue and link registry (AND-04).
- [BLE advertiser](ble-advertiser.md) - service UUID, beacon scan response and start throttling (AND-05).
- [BLE scanner](ble-scanner.md) - service-UUID filter, start throttling and raw records (AND-06).
- [GATT server host](ble-gatt-server.md) - inbound setup, CCCDs and paced notifications (AND-07).
- [GATT client link](ble-gatt-client.md) - outbound setup, write credit and handle lifecycle (AND-08).
- [NativeSink](ble-native-sink.md) - Rust-facing facade, callback ordering and APP-03 blocker (AND-09).
- [Foreground service](foreground-service.md) - connected-device type, stop action and restart policy (AND-10).

The BLE radio contract is normative in aoNetwork's `docs/native-contract.md`.
Kotlin passes raw bytes only; all framing and protocol decisions live in Rust.
