# Third-party licence report

Status: generated for the `v0.1.0` dependency set on 2026-11-22.

Source: `cargo +1.85 metadata --format-version 1 --locked`, filtered to unique
(name, version, declared licence, source) tuples. `cargo deny check` is the
enforcement gate; this table is the human-readable report required by APP-18.

The project licences are `MIT OR Apache-2.0`; both texts are committed as
`LICENSE-MIT` and `LICENSE-APACHE`. Sibling crates declare the same dual
licence at the pinned tags.

| Package | Version | Declared licence | Source |
|---|---|---|---|
| aead | 0.5.2 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| ao_idents | 0.1.0 | `MIT OR Apache-2.0` | `git+https://github.com/aoheyra/aoIdents#8fb173690ba4ade2b80f0459c40e7198bd0eb9ac` |
| ao_idents | 0.1.0 | `MIT OR Apache-2.0` | `git+https://github.com/aoheyra/aoIdents?tag=v0.1.0#8fb173690ba4ade2b80f0459c40e7198bd0eb9ac` |
| ao_mesh | 0.1.0 | `MIT OR Apache-2.0` | `git+https://github.com/aoheyra/aoMesh?tag=v0.1.0#0e9c36a38938412d993e0eea57df9b8a0d76c92f` |
| ao_network | 0.1.0 | `MIT OR Apache-2.0` | `git+https://github.com/aoheyra/aoNetwork#84f84eadfdaf2134b839b4d5dceeeeacdbf914a0` |
| ao_network | 0.1.0 | `MIT OR Apache-2.0` | `git+https://github.com/aoheyra/aoNetwork?tag=v0.1.0#84f84eadfdaf2134b839b4d5dceeeeacdbf914a0` |
| aoheyra | 0.1.0 | `MIT OR Apache-2.0` | `workspace` |
| argon2 | 0.5.3 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| arrayvec | 0.7.8 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| base64ct | 1.8.3 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| blake2 | 0.10.6 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| blake3 | 1.8.7 | `CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception` | `registry+https://github.com/rust-lang/crates.io-index` |
| block-buffer | 0.10.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| cc | 1.11.3 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| cfg-if | 1.0.5 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| chacha20 | 0.9.1 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| chacha20poly1305 | 0.10.1 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| cipher | 0.4.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| const-oid | 0.9.6 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| constant_time_eq | 0.4.2 | `CC0-1.0 OR MIT-0 OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| cpufeatures | 0.2.17 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| cpufeatures | 0.3.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| crypto-common | 0.1.7 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| curve25519-dalek | 4.1.3 | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| curve25519-dalek-derive | 0.1.1 | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| der | 0.7.10 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| digest | 0.10.7 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| ed25519 | 2.2.3 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| ed25519-dalek | 2.2.0 | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| fiat-crypto | 0.2.9 | `MIT OR Apache-2.0 OR BSD-1-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| find-msvc-tools | 0.1.15 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| generic-array | 0.14.7 | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| getrandom | 0.2.17 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| hkdf | 0.12.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| hmac | 0.12.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| inout | 0.1.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| libc | 0.2.191 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| once_cell | 1.21.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| opaque-debug | 0.3.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| password-hash | 0.5.0 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| pin-project-lite | 0.2.18 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| pkcs8 | 0.10.2 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| poly1305 | 0.8.0 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| proc-macro2 | 1.0.107 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| quote | 1.0.47 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| rand_core | 0.6.4 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| rand_core | 0.9.5 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| rustc_version | 0.4.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| semver | 1.0.28 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| serde | 1.0.229 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| serde_core | 1.0.229 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| serde_derive | 1.0.229 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| sha2 | 0.10.9 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| shlex | 2.0.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| signature | 2.2.0 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| snow | 0.10.0 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| spki | 0.7.3 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| subtle | 2.6.1 | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| syn | 2.0.119 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| syn | 3.0.10 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| thiserror | 2.0.21 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| thiserror-impl | 2.0.21 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| tracing | 0.1.44 | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| tracing-core | 0.1.36 | `MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| typenum | 1.20.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| unicode-ident | 1.0.27 | `(MIT OR Apache-2.0) AND Unicode-3.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| universal-hash | 0.5.1 | `MIT OR Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| version_check | 0.9.5 | `MIT/Apache-2.0` | `registry+https://github.com/rust-lang/crates.io-index` |
| wasi | 0.11.1+wasi-snapshot-preview1 | `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| x25519-dalek | 2.0.1 | `BSD-3-Clause` | `registry+https://github.com/rust-lang/crates.io-index` |
| zeroize | 1.9.2 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
| zeroize_derive | 1.5.0 | `Apache-2.0 OR MIT` | `registry+https://github.com/rust-lang/crates.io-index` |
