# BLE scanner (AND-06)

`ScanPlanner` keeps the latest Rust discovery profile and emits `Start`,
`Stop` or `Hold`. `Hold` prevents a platform call when state already matches,
the radio is off, or the 10-second restart window has not elapsed. Combined
with Rust's 10-second discovery interval, this avoids Android's scan-start
throttling; Kotlin never rapid-cycles scan starts.

`AndroidScanner` installs a `ScanFilter` for the aoNetwork service UUID and
uses the single BLE thread's `Handler` for callbacks. Every result is
converted to a 16-byte peer key from the six-byte LE address. Only the raw
`0xFFFF` manufacturer payload is forwarded to Rust, unchanged. Records with
more than 31 bytes are dropped and records without that company id carry an
empty beacon, as required by NET-03. No protocol bytes are decoded in Kotlin.

Manifest permissions are split by API level: coarse location on API 26-30,
`BLUETOOTH_SCAN` with `neverForLocation` on API 31 and later. Google Play
Services is not used.

JVM tests cover start coalescing, throttling, radio restarts, failure
reporting, raw-byte forwarding, malformed addresses, oversize records and
executor overflow.
