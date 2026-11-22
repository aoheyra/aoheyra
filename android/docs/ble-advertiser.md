# BLE advertiser (AND-05)

`AdvertisePlanner` keeps the latest Rust request and emits one of three
commands: `Start`, `Stop` or `Hold`. `Hold` means the platform must not be
called: either the desired state already matches, the radio is off, or the
10-second restart window has not elapsed. The long window matches Rust's
discovery interval and prevents Android's start throttling.

The advertising packet contains only the service UUID. The scan response
contains the raw 16-byte beacon as manufacturer data with company id `0xFFFF`.
`AndroidAdvertiser` builds those packets; Kotlin never parses or rewrites the
beacon. `AdvertiseCallback.onStartFailure` is reported to Rust, and the
planner blocks another start for the restart window instead of retrying in a
loop. Bluetooth toggles force a restart once the radio is ready again.

The connectable flag is a first-class planner input so a Rust-driven toggle
can replace the advertisement without changing the beacon. The APP-03 bridge
must expose that toggle before AND-09 can wire it end to end.

JVM tests cover byte passthrough, coalescing, throttling, stop/start, radio
restarts, connectable changes, start-failure reporting and executor overflow.
