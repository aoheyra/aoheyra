# Release process (APP-18)

## Tag matrix

All four repositories are versioned by git tag. A release is valid only when
the tags below resolve together; no branch pin, revision pin or committed
`[patch]` override is part of a release.

| Repository | Tag | Consumed by aoheyra as |
|---|---|---|
| `aoNetwork` | `v0.1.0` | `tag = "v0.1.0"` |
| `aoIdents` | `v0.1.0` | `tag = "v0.1.0"` |
| `aoMesh` | `v0.1.0` | `tag = "v0.1.0"` |
| `aoheyra` | `v0.1.0` | workspace `version = "0.1.0"` |

`crates/aoheyra/src/release.rs` reads the manifest during `cargo test` and
fails on any tag drift, a branch/revision pin or a committed `[patch]`.

## Verification

Run from a clean checkout of the release commit, with Rust 1.85:

```bash
cargo +1.85 fmt --all -- --check
cargo +1.85 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.85 test --workspace --all-features
cargo +1.85 deny check
RUSTDOCFLAGS=-D\ warnings cargo +1.85 doc --no-deps --workspace --all-features
cargo +1.85 metadata --format-version 1 --locked
```

The metadata command must list the three siblings at the tag and commit in
the table above. Regenerate the table in
[`docs/legal/third-party-licenses.md`](legal/third-party-licenses.md) from that
output when dependencies change; `cargo deny check` remains the gate.

## Android artefacts

Signing keys and keystores are never committed or printed. Release builds use
the R8 rules kept for UniFFI-generated classes (AND-22). Distribution covers
both Play and an open-source store, so core function must not depend on
Google Play Services.

## Status

The tag matrix, licence report and privacy draft are complete for `v0.1.0`.
The security review checklist is a template until the named reviewers record
their sign-off; no sign-off is claimed here.
