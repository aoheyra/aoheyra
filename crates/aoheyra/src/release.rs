//! Release-tag verification for the four-repository release process (APP-18).
//!
//! Siblings are consumed by git tag, never by branch or local `[patch]`. The
//! test below reads the checked-in manifest so a drift in any tag is caught
//! by the normal `cargo test` run, before a tag is cut.

/// Workspace version that must match the `v0.1.0` sibling tags.
pub const RELEASE_VERSION: &str = "0.1.0";

/// The git tags every repository in the release set must resolve together.
pub const TAG_MATRIX: [(&str, &str); 4] = [
    ("aoNetwork", "v0.1.0"),
    ("aoIdents", "v0.1.0"),
    ("aoMesh", "v0.1.0"),
    ("aoheyra", "v0.1.0"),
];

#[cfg(test)]
mod tests {
    use super::{RELEASE_VERSION, TAG_MATRIX};
    use crate::VERSION;

    const MANIFEST: &str = include_str!("../Cargo.toml");
    const WORKSPACE: &str = include_str!("../../../Cargo.toml");

    #[test]
    fn workspace_version_matches_the_release_matrix() {
        assert_eq!(VERSION, RELEASE_VERSION);
        assert!(WORKSPACE.contains(&format!("version = \"{RELEASE_VERSION}\"")));
    }

    #[test]
    fn every_sibling_is_pinned_to_the_release_tag() {
        for (repo, tag) in TAG_MATRIX.iter().take(3) {
            let needle = format!("{repo}\", tag = \"{tag}\"");
            assert!(
                MANIFEST.contains(&needle),
                "{repo} must be pinned to {tag}, found in manifest:\n{MANIFEST}"
            );
        }
    }

    #[test]
    fn no_branch_or_rev_pin_is_committed() {
        assert!(!MANIFEST.contains("branch ="));
        assert!(!MANIFEST.contains("rev ="));
        assert!(!MANIFEST.contains("[patch"));
    }
}
