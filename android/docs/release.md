# AND-22 Release engineering

## Signing

The release signing key is **never** in this repository. It lives in the
release maintainer's Keystore-backed store, outside the checkout, and is
supplied at build time through four environment variables:

| Variable | Meaning |
|---|---|
| `ANDROID_KEYSTORE_PATH` | Absolute path to the keystore file |
| `ANDROID_KEYSTORE_PASSWORD` | Keystore password |
| `ANDROID_KEY_ALIAS` | Signing key alias |
| `ANDROID_KEY_PASSWORD` | Key password |

`build.gradle.kts` reads those variables into a `release` signing config.
`android/scripts/release/build-release.sh` aborts before Gradle if any is
missing and never prints their values. Do not put them in `gradle.properties`,
a local property file, CI logs or a PR body. A lost or rotated key changes the
app's signing identity; coordinate that with distribution separately.

## R8 and UniFFI

Release builds enable `isMinifyEnabled` and `isShrinkResources`. The generated
UniFFI bindings live in `com.aoheyra.core` (see `crates/aoheyra/uniffi.toml` on
the APP-03 branch) and use JNA; `app/proguard-rules.pro` keeps that package, the
`UniFFILib` implementations, JNA classes and native methods. Generated bindings
are produced by the Gradle task and are never committed. When APP-03 lands on
master, verify a release build on a device: a stripped binding fails at the
first Rust call with an `UnsatisfiedLinkError` or a missing class, not at build
time. Do not disable minification to work around a bad keep rule.

## ABI splits

A release produces `arm64-v8a`, `armeabi-v7a` and `x86_64` APKs plus a universal
APK. `x86_64` is for emulator UI checks only; radio behaviour needs physical
phones. The native libraries come from the documented `cargo ndk` command and
are not committed.

## On-device crash reporting

There is no network crash-reporting SDK and no `INTERNET` permission in the
release manifest. Crash diagnostics stay on the device in app-private storage
owned by the Rust core (APP-15) and leave only when the user explicitly exports
a bundle. The export contains no message text, keys or coordinates. Do not add
a crash SDK that uploads automatically: it would break the privacy stance and
the open-source store distribution.

## Reproducible build notes

Record these inputs with every release:

* source commit, with no local `[patch]` override or uncommitted native library;
* JDK 17, Android Gradle Plugin 8.2.2, Kotlin 1.9.24, NDK 25.1.8937393;
* pinned sibling crate tags from `crates/aoheyra/Cargo.toml`;
* the exact `cargo ndk` command from the root README;
* Gradle task and SHA-256 of each APK/AAB (the script prints the hashes).

```bash
ANDROID_KEYSTORE_PATH=/path/to/keystore.jks \
ANDROID_KEYSTORE_PASSWORD=... ANDROID_KEY_ALIAS=... ANDROID_KEY_PASSWORD=... \
  android/scripts/release/build-release.sh
```

The same inputs and commands make the build reproducible in the sense that the
release can be rebuilt and reviewed from source. Gradle/AGP do not guarantee
byte-identical archives across machines, so do not claim byte-for-byte
reproducibility: compare the recorded hashes, investigate differences and
verify the signing certificate with `apksigner verify --print-certs` before
publishing.

## Distribution plan

* Play Store: signed AAB, staged rollout, no proprietary SDK required for core
  function; declare the foreground-service and nearby-device permissions in
  the store listing.
* Open-source store: signed universal APK or per-ABI APKs built from the same
  commit, with the source tag and licence files included. No dependency that
  requires Google Play Services for core function; fused location is optional.

## Release acceptance

The acceptance step is a clean-device installation of the signed release build
followed by the full AND-23 scenario list. Neither a physical device nor the
Android SDK was available here, and APP-03's generated bindings are not yet on
master, so this artefact is a release procedure and a verifiable config, not a
claim that a release build was installed.

Run the config gate locally with:

```bash
python3 -m unittest discover -s android/scripts/release
python3 android/scripts/release/verify_release_config.py
```
