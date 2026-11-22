//! Release-tag verification for the four-repository release process (APP-18).
//!
//! Until the first coordinated release, siblings are consumed as untagged git
//! sources on their default branches. Cargo treats a tagged source as a
//! different identity, so pinning one sibling early would duplicate `Identity`,
//! `LinkId` and `NodeId` types across the seam. The tag matrix below records
//! the tags that will be cut together; the tests read the checked-in manifest
//! so manifest drift is caught by the normal `cargo test` run.

/// Workspace version that will be matched by the first `v0.1.0` sibling tags.
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
    fn release_matrix_covers_all_four_repositories() {
        assert_eq!(TAG_MATRIX.len(), 4);
        for (repo, tag) in TAG_MATRIX {
            assert_eq!(tag, "v0.1.0", "{repo} has an unexpected release tag");
        }
    }

    #[test]
    fn siblings_are_untagged_until_the_first_release() {
        for (repo, _) in TAG_MATRIX.iter().take(3) {
            let source = format!("git = \"https://github.com/aoheyra/{repo}\"");
            assert!(
                MANIFEST.contains(&source),
                "{repo} must be an untagged git source, found in manifest:\n{MANIFEST}"
            );
            assert!(
                !MANIFEST.contains(&format!("{repo}\", tag =")),
                "{repo} must not be pinned to a release tag yet"
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
