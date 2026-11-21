# aoheyra

**Mesh network messaging for Android.** Chat, share your location and see your contacts on an offline map with no internet and no servers, over a Bluetooth LE mesh of nearby phones. Messages hop phone to phone, and are carried and delivered later if the recipient is out of range.

> **Status: pre-alpha, M0 (Foundations).** The Rust workspace is scaffolded and consumes tagged sibling crates. This README records the planned design from the project backlog (`APP-xx`, `AND-xx`); each issue is implemented in its own PR. Fuzzing is centralised in [aoheyra-fuzz](https://github.com/aoheyra/aoheyra-fuzz).

Platform: **Android only** (minSdk proposed 26, to be confirmed by the BLE spike AND-02).

---

## The four repos

```
                aoheyra   (this repo)
          Rust core (UniFFI)  +  Kotlin Android app (android/)
                   │  depends on all three
        ┌──────────┼───────────┐
        ▼          ▼           ▼
    aoMesh ───▶ aoNetwork    aoIdents
   routing,      BLE link      identity,
   store, sync   layer         crypto, trust
        │                       ▲
        └───────────────────────┘

aoNetwork and aoIdents never depend on each other.
```

| Repo | Package | Owns |
|---|---|---|
| [`aoNetwork`](https://github.com/aoheyra/aoNetwork) | `ao_network` | Radio peers, link state machine, fragmentation, send queue, degree cap, per-link abuse protection, native radio contract, simulator |
| [`aoIdents`](https://github.com/aoheyra/aoIdents) | `ao_idents` | Node identity, keys, signatures, sealed messages, rotating tags, contact-book logic, replay guard |
| [`aoMesh`](https://github.com/aoheyra/aoMesh) | `ao_mesh` | Packet format, routing per traffic class, store-and-forward, sync, flood control, presence, node directory |
| [`aoheyra`](https://github.com/aoheyra/aoheyra) | `aoheyra` | App payload schema, persistence (SQLite), runtime orchestration, **the only FFI (UniFFI)**, Kotlin UI and BLE shim |

## Weekly fuzzing

[aoheyra-fuzz](https://github.com/aoheyra/aoheyra-fuzz) starts a 90-minute fuzz run every Sunday at 00:00 IST (Saturday 18:30 UTC, GitHub cron `30 18 * * 6`). It dispatches 5,400 libFuzzer seconds to each of `aoMesh`, `aoNetwork` and `aoIdents`, split across every target in the repository, and validates this docs-only repository. `aoheyra` has no fuzz targets while it has no Rust code; adding code here requires a matching fuzz target in the same change that introduces the parser.

## Ownership map (one owner per concept)

| Concept | Owner |
|---|---|
| Node identity, keys, `NodeId`, `IdentityCard` | aoIdents |
| Rotating link tag and beacon hint | aoIdents generates; aoMesh passes down; aoNetwork carries (duplicate-link tie-break only) |
| Link `Hello` (version, caps, tag, opaque blob) | aoNetwork (unauthenticated); aoMesh puts a signed `MeshHello` in the blob |
| Neighbour authentication and hop encryption | aoIdents handshake; aoMesh drives it (optional, M6) |
| E2E encryption, signatures, replay guard | aoIdents |
| Packet header, TTL, `MessageId`, routing | aoMesh |
| Forwarding dedupe (seen-cache) | aoMesh |
| Rate limiting | aoNetwork per link; aoMesh per origin and class |
| Coalescing and expiry | aoNetwork mechanism; aoMesh policy |
| Degree cap, slots, link classes | aoNetwork enforces; aoMesh advises; aoheyra supplies numbers |
| Neighbour table / node directory / contact book | aoNetwork (radio peers) / aoMesh (mesh-wide) / aoIdents logic, **aoheyra persists** |
| Store-and-forward policy and sync | aoMesh; **aoheyra supplies the SQLite `MessageStore`** |
| App payload schema (text, location, profile, receipt) | **aoheyra** |
| Persistence | **aoheyra only** (Rust SQLite is the single source of truth; no Room) |
| FFI | **aoheyra only** (UniFFI) |
| Android radio | Kotlin `:ble` module in `android/` (contract is aoNetwork's `NativeSink`) |
| Location capture, permission, precision | aoheyra + Android |
| Map and tiles | Android (offline-first) |

The versioned binary `AppPayload` codec lives in
`crates/aoheyra/src/payload.rs`; its exact layout and mapping onto aoMesh
packet classes are recorded in
[ADR 0003](docs/adr/0003-app-payload-schema.md).

## What the app will do (MVP)

- **Direct messaging** end-to-end encrypted, delivered across multiple hops, with delivery receipts.
- **Store-and-forward:** phones carry messages for others and hand them over when they meet.
- **Flood control:** broadcasts are suppressed, spammers are throttled network-wide.
- **Presence and public channels** (signed, unencrypted).
- **Location sharing** with chosen contacts for a chosen time, with three precision modes (precise, about 100 m, off), shown on an **offline map**.
- **Identity by QR code**, trust on first use, safety numbers, loud key-change warnings.
- Runs in a **foreground service**, battery-profiled, with a diagnostics bundle that contains no message content or keys.

### Privacy stance

- All data stays on the device. Nothing leaves unless the user explicitly shares or exports it.
- Precise coordinates never leave the device unless the sharing session is set to precise.
- Location is sealed to chosen contacts, never public by default; "stop sharing" is immediate.
- The database is encrypted (SQLCipher) with a key held by the Android Keystore; auto-backup is excluded.
- **Known MVP limits** (see the aoIdents threat model, IDN-19): relays can see origin and destination of packets so they can rate-limit spam; no forward secrecy against recipient key compromise until IDN-15.

## Repository layout

```
aoheyra/
├── Cargo.toml              # workspace
├── crates/
│   └── aoheyra/            # Rust core (cdylib for Android), UniFFI surface
│       └── src/            # runtime, storage, messaging, contacts, location, map data, settings
├── android/                # Kotlin app, Gradle Kotlin DSL, Jetpack Compose
│   ├── app/                # UI, DI, service, permissions, Keystore vault
│   └── ble/                # :ble module: advertiser, scanner, GATT server/client, NativeSink impl
├── docs/
│   ├── adr/                # the five APP-02 decisions
│   └── local-development.md
├── AGENTS.md
└── android/AGENTS.md
```

AND-01 adds the Gradle Kotlin DSL scaffold with `:app` and `:ble`, minSdk 26,
targetSdk 34 and manual constructor injection. The BLE module is an empty
placeholder until AND-04..09.

## Architecture

```
 Kotlin UI (Compose)  ──UniFFI──▶  AoHeyra object (Rust)
   chat · contacts · map              │  event listener, secret vault, BLE NativeSink (callbacks)
   settings · nearby                  ▼
 Foreground service            Runtime orchestrator
 Keystore vault                 ├─ Messaging · Contacts · Location · Channels · Map data
 Kotlin :ble module ◀─NativeSink─┤
   (GATT, advertise, scan)       ├─ aoMesh engine ── aoNetwork ── (Kotlin radio)
                                 ├─ aoIdents (identity, crypto, trust)
                                 └─ Storage: rusqlite + SQLCipher (WAL, migrations)
```

Key decisions (APP-02), recorded as short ADRs:

1. [Rust owns all persistence](docs/adr/0001-rust-owns-persistence.md); Kotlin never writes its own database.
2. [UniFFI boundary and threading model](docs/adr/0002-uniffi-boundary-and-threading.md).
3. [The app payload schema lives here and is versioned](docs/adr/0003-app-payload-schema.md).
4. [Time and RNG are injected](docs/adr/0004-injected-time-and-rng.md); no shared utility crate unless a real duplication appears.
5. [One runtime-owned object reaches aoNetwork](docs/adr/0005-kotlin-ble-reaches-ao-network.md) on behalf of the Kotlin `:ble` module.

## Build

> Requirements: Rust 1.85 or newer, Android SDK/NDK, `cargo-ndk`, JDK 17+. The Android Gradle scaffold is present; the native build command is documented below. Android SDK/NDK and `cargo-ndk` were unavailable locally, so device installation and native builds are verified in CI rather than claimed here.

```bash
# Native libraries (one documented command, APP-01)
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android/app/src/main/jniLibs build --release

# Android debug build (generates UniFFI Kotlin bindings via a Gradle task)
cd android && ./gradlew assembleDebug lintDebug

# Rust checks
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cargo deny check
```

`x86_64` is for emulator UI work only; the radio needs **physical phones**.

### Sibling crates during development

Sibling crates are pinned to the `v0.1.0` git tag in `crates/aoheyra/Cargo.toml`. To co-develop locally, add an `[patch]` override in an **uncommitted** `.cargo/config.toml`; the exact paths and the rule that it must never be committed are documented in [docs/local-development.md](docs/local-development.md). A PR must resolve with the committed tags and no local patches.

## Milestones and issues

Same milestone names in every repo.

| # | Milestone | Exit criteria |
|---|---|---|
| M0 | Foundations | Repos scaffolded, CI green, ADRs written, BLE spike results on real phones |
| M1 | Wire formats and primitives | All codecs and crypto primitives done and tested, no networking yet |
| M2 | Two phones talk | Link layer + Android BLE shim + UniFFI: two phones form a link and exchange frames |
| M3 | Direct messaging over the mesh | Identity, contacts, sealed 1:1 messages, multihop delivery, degree-capped topology, chat UI |
| M4 | Store-and-forward and flood control | Store, sync, spray-and-wait, receipts, broadcast suppression, quotas |
| M5 | Presence, location and map | Announcements, node directory, location sharing, offline map |
| M6 | Hardening and optimization | Fuzzing, hop encryption, battery and soak tests, OEM matrix, sync v2 |
| M7 | Beta | Release engineering, privacy review, field-test sign-off |

Issues in this repo (Rust core `APP-01..18`, Android `AND-01..23`):

| Milestone | Core | Android |
|---|---|---|
| M0 | APP-01 workspace and Android build, APP-02 ADRs | AND-01 scaffold, AND-02 BLE feasibility spike |
| M2 | APP-03 UniFFI skeleton, APP-04 storage, APP-06 contact store and vault bridge, APP-07 payload schema, APP-08 runtime orchestrator | AND-03 permissions, AND-04..08 BLE core/advertiser/scanner/GATT server/client, AND-09 `NativeSink`, AND-10 foreground service |
| M3 | APP-09 messaging, APP-10 contacts, APP-11 settings | AND-11 chat UI, AND-12 identity UI, AND-13 Keystore, AND-14 settings UI, AND-15 nearby screen |
| M4 | APP-05 SQLite `MessageStore`, APP-17 integration suite (part) | none |
| M5 | APP-12 location, APP-13 map data, APP-14 channels | AND-16 location provider, AND-17 offline map, AND-18 share-location UX |
| M6 | APP-15 diagnostics, APP-16 data lifecycle | AND-19 battery, AND-20 OEM matrix, AND-21 accessibility/i18n |
| M7 | APP-17 (rest), APP-18 release and legal | AND-22 release engineering, AND-23 field-test plan |

Where this summary and an issue header differ, **the issue header wins**.

Issue conventions: put the ID in the title (`[APP-09] Messaging service`). Labels: `type:feature|infra|test|spike|adr|docs`, `prio:P0|P1|P2`, `size:S|M|L`, plus `area:android` for Kotlin work.

## MVP acceptance scenarios

These become the integration suite (APP-17) and the field-test script (AND-23).

1. **Direct chat:** two phones, message delivered, receipt shown.
2. **Relay:** A, B, C in a line, A and C out of range; A's message reaches C through B.
3. **Store-and-forward:** A messages C while C is away; B carries it; C arrives later and receives it; the receipt purges the copies.
4. **Degree cap:** 8 phones in one room; no phone exceeds its link cap and the graph stays connected.
5. **Flood control:** one phone spams broadcasts; others throttle it; normal traffic keeps its delivery ratio.
6. **Location:** A shares with contact B for 15 minutes; B sees A on the offline map; sharing ends on time.
7. **Persistence:** kill and restart the app; identity, contacts, messages and the carried store survive.
8. **Soak:** 8 hours, screen off, within the battery budget set in AND-19.

## Open decisions and risks

- **Sibling API stability:** the three siblings are pinned to `v0.1.0`; changes to their public APIs require a new tag here.
- **Relay-visible metadata** in the MVP (see MSH-02, IDN-19).
- **Forward secrecy** deferred to IDN-15; the threat model says so plainly.
- **Android connection limits and OEM background kills** are unknown until AND-02 and AND-20; aoNetwork defaults may change.
- **Cross-repo versioning** by git tag plus local `[patch]` needs discipline; revisit if friction grows (APP-01).
- **minSdk** proposed 26; confirm against the spike.

## Contributing

1. Read the issue's scope and "Done when" line.
2. Read the [rules](AGENTS.md) (and [android/AGENTS.md](android/AGENTS.md) for Kotlin work).
3. One issue per PR; CI green without loosened checks.

Report vulnerabilities privately to the maintainers (contact in `SECURITY.md`), not in a public issue.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option, matching aoMesh, aoNetwork and aoIdents.
