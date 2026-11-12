# Rules (android)

Applies to everything under `android/` (Kotlin, Gradle, Compose, the `:app` and `:ble` modules). It **adds to** the root [rules](../AGENTS.md); read that first. The root rules on root-cause fixes, no workarounds, no malicious code, honesty and one-issue-per-PR apply here without exception.

Issues: `AND-01..23`, label `area:android`. Titles carry the ID, e.g. `[AND-08] GATT client link`.

---

## 1. Kotlin-specific rules

- **No workarounds for radio flakiness.** Status 133, dropped notifications and stuck GATT operations have causes: operation overlap, missing `close()`, wrong callback ordering, throttling. Fix the cause. Do not paper over it with `Thread.sleep`, blind retries or catch-all handlers.
- **Rust owns state.** Kotlin has **no Room database**, no SharedPreferences for app data, no authoritative caches. UI reads through UniFFI and renders what Rust reports. The message status shown must match Rust's status machine exactly.
- **Never block the Rust runtime**, and never run Rust calls, GATT work or I/O on the main thread. All `NativeSink` callbacks return immediately and forward to Rust **in order**.
- **No `!!`** on values that come from the radio, Rust or the OS. Model nullability and errors explicitly.
- Generated UniFFI bindings are produced by the Gradle task. Never hand-edit or commit them.
- **No Google Play Services dependency for core function:** QR scanning, maps and location must work without it (fused location is an optional provider).
- Honour the platform: use the correct permissions per API level, correct foreground-service types, and the Keystore. Never request more than the feature needs or ask for a permission before explaining why.

## 2. What to find where

| You need | Look at |
|---|---|
| The radio contract (UUIDs, characteristics, callback ordering, status-code mapping) | aoNetwork `docs/native-contract.md` (NET-03) and Rust `NativeSink`. Implement exactly this; if it is ambiguous, fix the document in aoNetwork, not by guessing |
| BLE feasibility results, per-phone limits | `docs/ble-spike/` results table (AND-02); defaults for link count and MTU come from here |
| BLE core, advertiser, scanner, GATT server/client | `ble/` (AND-04..08) |
| `NativeSink` implementation | `ble/` (AND-09) |
| Foreground service | `app/service/` (AND-10) |
| Permissions and onboarding | `app/onboarding/` (AND-03) |
| Keystore `SecretVault`, app lock | `app/security/` (AND-13) |
| Screens (chat, identity/contacts, settings, nearby, map, share-location) | `app/ui/` (AND-11, 12, 14, 15, 17, 18) |
| Location provider | `app/location/` (AND-16) |
| OEM matrix and per-vendor help | `docs/oem-matrix.md` (AND-20) |
| Battery budget and measurements | `docs/battery.md` (AND-19) |
| Field-test plan and issue template | `docs/field-test/` (AND-23) |

(Paths are the planned layout; update this table in the same PR if the code differs.)

## 3. BLE rules (hard requirements)

1. **One serial thread for all GATT work.** Each link has a **single-flight operation queue** (MTU, service discovery, descriptor write, writes, notifications). Two GATT operations never run on one link at once; this is stress-tested.
2. `LinkRegistry` allocates `LinkId` for both GATT roles (client and server).
3. **Always `close()` the `BluetoothGatt`** on failure and on disconnect. Back off on status 133. 100 connect/disconnect cycles must not leak handles.
4. **Pace writes from callbacks** (write without response; notifications wait for the send callback before the next, then raise `WriteReady`). No unbounded buffering.
5. **Scanning:** always filter by the service UUID (required for screen-off scanning); guard Android's scan-start throttling with long on/off windows, never rapid cycling; deliver the **raw** scan record to Rust (beacon parsing is Rust-only).
6. **Advertising:** service UUID in the advertising packet, beacon in scan-response manufacturer data; connectable toggle driven by Rust; report `onStartFailure` to Rust; restart after Bluetooth toggles.
7. **Over-limit inbound connections** are cancelled; Rust's cap is authoritative.
8. Raise connection priority during sync and lower it afterwards; request 2M PHY only where supported and never rely on it.
9. Recover cleanly if the Rust side restarts; ignore late callbacks after stop.
10. Never decode protocol bytes in Kotlin. Pass bytes through.

## 4. Privacy, permissions and UX rules

- Location is shared only through an explicit session (contacts, duration, precision). Sharing shows a **persistent in-app and notification indicator**, has a **one-tap stop**, and stops when permission is revoked. The indicator must always be accurate.
- Never start background location, advertising or scanning without the matching permission, the correct foreground-service type and a visible notification.
- Every denied or revoked permission produces an explanation and a working recovery path (tested on API 29, 31 and 34).
- Crash reporting and diagnostics **stay on the device** unless the user exports them; exports contain no message content, keys or coordinates.
- Keystore: the identity seed and database key are wrapped by a Keystore-held key (hardware-backed where available); the app recovers cleanly if the key is invalidated; copying app data to another device yields nothing usable. Auto-backup is excluded.
- The map must **never need internet** to function; offline tiles plus a bundled low-zoom fallback and a radar view when no tiles exist.
- Accessibility is part of "done": content descriptions, large text, dark theme, translatable string resources.

## 5. Testing: what "done" requires

- **Unit and instrumented tests** for operation-queue serialisation, link registry, permission flows and Keystore recovery.
- **Real-phone evidence** for anything radio, battery, background or OEM related. Emulators cannot validate BLE. State the devices and Android versions used.
- **Stress tests** from AND-04 and AND-08 (parallel requests; 100 connect/disconnect cycles) are run and kept in the repo.
- **Soak:** foreground service keeps the mesh running with the screen off for 1 hour and restarts after being killed (AND-10); battery budgets measured with Battery Historian (AND-19).
- **Lint and accessibility:** `lintDebug` clean; accessibility scanner shows no blocking issues on core screens.
- Results (spike, OEM matrix, battery) are committed as tables, not described in prose.

**Definition of done:** the "Done when" line is met with evidence; tests added; `./gradlew assembleDebug lintDebug test` green locally and in CI; verified on the stated physical devices when radio-related; docs updated.

## 6. Commands

```bash
cd android
./gradlew assembleDebug
./gradlew lintDebug
./gradlew test
./gradlew connectedDebugAndroidTest     # needs a device

# Native libs from the repo root
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android/app/src/main/jniLibs build --release
```

If a command or a device is unavailable, say so plainly; do not skip silently or claim success.

## 7. Release notes

- R8 rules must keep UniFFI-generated classes (AND-22); verify a release build, not just debug.
- Signing keys and keystores are never committed or printed.
- Distribution plan covers Play and an open-source store; do not add proprietary-only dependencies that would block the latter.
