# Local development with sibling crates

The committed `Cargo.toml` consumes aoMesh, aoNetwork and aoIdents as untagged
git sources on their default branches, matching aoMesh's own manifest. There
are no release tags yet; the first tags start at `v0.1.0` and all crates will
move to them together when they are cut. CI and release builds must resolve the
committed untagged sources with no local patches.

To work against uncommitted sibling changes, create an uncommitted
`.cargo/config.toml` in this repository:

```toml
[patch."https://github.com/aoheyra/aoMesh"]
ao_mesh = { path = "../aoMesh-master" }

[patch."https://github.com/aoheyra/aoNetwork"]
ao_network = { path = "../aoNetwork" }

[patch."https://github.com/aoheyra/aoIdents"]
ao_idents = { path = "../aoIdents" }
```

Adjust the paths to your checkout. Never commit this file or a `[patch]`
section. Before opening a PR, land the sibling change on its default branch,
refresh the lockfile here and delete the local override. A PR must build from
the committed untagged sources. When release tags start at `v0.1.0`, all
repositories change their pins together in one coordinated pass.

## Native libraries

With the Android SDK/NDK installed and `cargo-ndk` available, the native
libraries are built from the repository root with one command:

```bash
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 -o android/app/src/main/jniLibs build --release
```

`x86_64` is for emulator UI work only; radio behaviour needs physical phones.
This environment has no Android SDK/NDK or `cargo-ndk`, so the command is
documented here but was not executed locally for APP-01.
