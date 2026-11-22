# Android documentation

- [BLE core](ble-core.md) - serial thread, per-link operation queue and link registry (AND-04).
- [BLE advertiser](ble-advertiser.md) - service UUID, beacon scan response and start throttling (AND-05).

The BLE radio contract is normative in aoNetwork's `docs/native-contract.md`.
Kotlin passes raw bytes only; all framing and protocol decisions live in Rust.
