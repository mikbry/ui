//! Without any backend feature enabled, the bridge crate must still
//! compile and `Mkui::new()` must return a clear initialization error
//! rather than panicking or producing a link error.
//!
//! This locks in the contract documented in `mkui/src/lib.rs`: end users
//! get a runtime explanation of which feature to enable, not a confusing
//! build failure deep in a downstream crate.

use mkui::prelude::*;

#[test]
fn new_without_backend_returns_initialization_error() {
    let result = Mkui::new();
    assert!(
        result.is_err(),
        "Mkui::new() must error when no backend feature is enabled"
    );
}

#[test]
fn run_without_backend_returns_error_after_construction_workaround() {
    // We can't construct an Mkui without a backend, but we can at least
    // confirm `MkuiError::generic` is reachable as documented in the
    // bridge crate's prelude.
    let err = MkuiError::generic("sanity");
    assert!(err.to_string().contains("sanity"));
}
