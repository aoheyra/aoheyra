//! Rust core for the aoheyra Android application.
//!
//! This crate is the only FFI surface in the project. It will expose the
//! UniFFI `AoHeyra` object in APP-03; this M0 scaffold only pins the workspace
//! layout, the Android `cdylib` target and the tagged sibling dependencies.
//! Application schema, persistence and orchestration are added by APP-04 to
//! APP-16, never by reimplementing sibling-crate behaviour.

/// Crate version from `Cargo.toml`, used by diagnostics and release checks.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg(test)]
mod tests {
    use super::VERSION;

    #[test]
    fn version_is_the_workspace_version() {
        assert_eq!(VERSION, "0.1.0");
    }
}
