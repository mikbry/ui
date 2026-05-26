//! Style-class re-exports.
//!
//! The canonical `StyleClass` + parser live in `mkui-runtime` (ADR 0005) so
//! every binding sees the same parse without depending on `mkui-core`. This
//! module preserves the historical `mkui_core::style::StyleClass` path for
//! downstream callers while the canonical type itself moved one crate
//! deeper.

pub use mkui_runtime::{ClassParseError, ResolvedStyle, StyleClass};

/// Builder helper retained for source compatibility with v0.4.x callers
/// that wrote `Style::class("flex")`.
pub struct Style;

impl Style {
    pub fn class(class: &str) -> StyleClass {
        StyleClass::from_str(class)
    }
}
