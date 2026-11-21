# ADR 0005: One runtime-owned object reaches aoNetwork

Status: accepted (APP-01, M0).

## Context

Kotlin owns the radio (advertising, scanning, GATT server and client) but
aoNetwork owns link state, fragmentation and the native radio contract. A
direct Kotlin-to-aoNetwork call path would bypass the Rust runtime's lifecycle
and queueing rules.

Options considered:

1. Each Kotlin BLE component obtains its own aoNetwork handle and calls it
   directly.
2. The Rust runtime owns one aoNetwork object. Kotlin `:ble` implements the
   `NativeSink` callbacks and talks only to that runtime-owned object.

Option 1 scatters state and makes shutdown and callback ordering unclear.
Option 2 keeps one owner for start/stop/pause and one place to enforce ordered,
bounded delivery of radio events.

## Decision

The Rust runtime owns the single aoNetwork object and drives it through
lifecycle methods. Kotlin `:ble` modules implement the `NativeSink` contract
from aoNetwork's `docs/native-contract.md` and pass bytes and link events to
that object. Kotlin never decodes protocol bytes. `LinkId` allocation follows
aoNetwork's contract for both GATT roles.

## Consequences

- Connection state, pending events and shutdown live in Rust, not in a Kotlin
  cache.
- Carbons of radio events are not replayed after stop; late callbacks are
  ignored (AND-04..10).
- The `NativeSink` implementation is a separate tracked issue (AND-09), not
  part of this ADR's scaffold.
