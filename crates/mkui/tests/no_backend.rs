//! Without any backend feature enabled, the bridge crate must still
//! compile and `Mkui::new()` must return a clear initialization error
//! rather than panicking or producing a link error.
//!
//! This locks in the contract documented in `mkui/src/lib.rs`: end users
//! get a runtime explanation of which feature to enable, not a confusing
//! build failure deep in a downstream crate.
//!
//! Note: `cargo test --workspace` unifies features (resolver = 2), so when
//! any sibling crate depends on `mkui` with `console` or `web` enabled,
//! the no-backend assertion is meaningless. The `cfg` gate below pins the
//! assertion to the configuration where it is actually testable.

use mkui::prelude::*;

#[cfg(not(any(feature = "web", feature = "console")))]
#[test]
fn new_without_backend_returns_initialization_error() {
    let result = Mkui::new();
    assert!(
        result.is_err(),
        "Mkui::new() must error when no backend feature is enabled"
    );
}

#[test]
fn mkui_error_generic_is_reachable_through_the_prelude() {
    let err = MkuiError::generic("sanity");
    assert!(err.to_string().contains("sanity"));
}
