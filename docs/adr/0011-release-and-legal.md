# ADR 0011: Git-tag releases with generated legal evidence (APP-18)

Status: accepted (APP-18, M7). Security sign-off remains pending.

## Context

Four repositories have independent histories but one wire protocol. Branch
pins would let a release silently mix incompatible revisions, and a manual
licence list drifts as dependencies change. Options considered:

1. Publish all crates to crates.io and resolve by semver.
2. Keep the current git-tag distribution and verify the tag matrix in CI.

Option 1 adds a registry trust boundary and does not match the `publish =
false` sibling manifests. Option 2 keeps the existing dependency direction and
makes drift visible in a normal `cargo test`.

## Decision

Releases are a four-tag set, all `v0.1.0` for the first release. `aoheyra`
pins each sibling by `tag`, never by branch, revision or committed `[patch]`.
`crates/aoheyra/src/release.rs` checks those pins and the workspace version
during tests. The third-party licence table is generated from `cargo
metadata`; `cargo deny check` enforces the allowed licence set. The privacy
draft and the security checklist are checked in under `docs/legal/` and are
updated whenever a feature changes storage or off-device behaviour.

## Consequences

- A release requires all four tags to exist and resolve together; no partial
  tag is valid.
- Legal documents are reviewable text in the same repository as the code
  they describe.
- The security checklist is not signed off by this ADR; named reviewers must
  record evidence and dates before beta.
