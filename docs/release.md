# Release process (APP-18)

## Pre-release sibling sources

The first release is `v0.1.0`, but no tags have been cut. Until that
coordinated release, all three siblings are consumed as untagged git sources
on their default branches. Cargo treats a tagged source as a different
identity, so pinning one sibling early would duplicate `Identity`, `LinkId`
and `NodeId` types across the seam. A committed branch pin, revision pin or
`[patch]` override is likewise forbidden.

| Repository | Planned first tag | Consumed by aoheyra before release |
|---|---|---|
| `aoNetwork` | `v0.1.0` | untagged git source |
| `aoIdents` | `v0.1.0` | untagged git source |
| `aoMesh` | `v0.1.0` | untagged git source with `adapter` feature |
| `aoheyra` | `v0.1.0` | workspace `version = "0.1.0"` |

`crates/aoheyra/src/release.rs` reads the manifest during `cargo test` and
fails on tag drift, a branch/revision pin or a committed `[patch]`. At release
time, the same matrix is switched to tags in one commit across all four
repositories; no repository is tagged alone.

## Verification

Run from a clean checkout of the release commit, with Rust 1.89:

```bash
cargo +1.89 fmt --all -- --check
cargo +1.89 clippy --workspace --all-targets --all-features -- -D warnings
cargo +1.89 test --workspace --all-features
cargo +1.89 deny check
RUSTDOCFLAGS=-D\ warnings cargo +1.89 doc --no-deps --workspace --all-features
cargo +1.89 metadata --format-version 1 --locked
```

The metadata command must list the three siblings at the same source before
release, then at the tag and commit in the table above once tags are cut.
Regenerate the table in
[`docs/legal/third-party-licenses.md`](legal/third-party-licenses.md) from that
output when dependencies change; `cargo deny check` remains the gate.

## Android artefacts

Signing keys and keystores are never committed or printed. Release builds use
the R8 rules kept for UniFFI-generated classes (AND-22). Distribution covers
both Play and an open-source store, so core function must not depend on
Google Play Services.

## Status

The tag matrix, licence report and privacy draft are prepared for `v0.1.0`.
No release tags exist yet. The security review checklist is a template until
the named reviewers record their sign-off; no sign-off is claimed here.
