//! Workspace-wide error type for mkui operations.
//!
//! Built on `thiserror`. Variants stay narrow on purpose; if a call site does
//! not fit cleanly, add a new variant rather than smuggle structure through a
//! free-form `String`.

use thiserror::Error;

/// Completely abstract error type for mkui operations.
///
/// `MkuiError` is `Send + Sync` on native targets so it can flow through
/// `Result<_, MkuiError>` across spawned tasks if backends introduce async at
/// the edges (e.g. `pollster::block_on` in `mkui-wgpu`). On `wasm32` the
/// `JsValue` variant is enabled and the enum is intentionally `!Send + !Sync`
/// — errors there are local to the single-threaded WASM context.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MkuiError {
    /// Initialization error (e.g., backend failed to create a UI context).
    #[error("initialization: {0}")]
    Initialization(String),

    /// Rendering error (e.g., backend failed to render a component).
    #[error("rendering: {0}")]
    Rendering(String),

    /// I/O error surfaced by terminal, file, or socket backends.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Text-system error (font registration, layout, rasterization).
    #[error("text: {0}")]
    Text(#[from] mkui_text::TextError),

    /// Error surfaced by the web/WASM backend's `JsValue` boundary.
    ///
    /// Not `#[from]`: that macro treats the inner value as an error *source*,
    /// which requires `wasm_bindgen::JsValue: std::error::Error` — a bound
    /// `JsValue` does not satisfy. The manual `From` impl below gives the
    /// same ergonomic `.into()` / `?` conversion without that bound.
    #[cfg(target_arch = "wasm32")]
    #[error("js: {0:?}")]
    JsValue(wasm_bindgen::JsValue),

    /// Generic/uncategorized error.
    #[error("{0}")]
    Generic(String),
}

impl MkuiError {
    pub fn initialization(message: impl Into<String>) -> Self {
        Self::Initialization(message.into())
    }

    pub fn rendering(message: impl Into<String>) -> Self {
        Self::Rendering(message.into())
    }

    pub fn io(message: impl Into<String>) -> Self {
        Self::Io(std::io::Error::other(message.into()))
    }

    pub fn generic(message: impl Into<String>) -> Self {
        Self::Generic(message.into())
    }
}

impl From<String> for MkuiError {
    fn from(msg: String) -> Self {
        MkuiError::Generic(msg)
    }
}

impl From<&str> for MkuiError {
    fn from(msg: &str) -> Self {
        MkuiError::Generic(msg.to_string())
    }
}

#[cfg(target_arch = "wasm32")]
impl From<wasm_bindgen::JsValue> for MkuiError {
    fn from(value: wasm_bindgen::JsValue) -> Self {
        MkuiError::JsValue(value)
    }
}

// `MkuiError` must stay `Send + Sync` on native targets (rustdoc on the enum
// promises it so backends can carry errors across spawned tasks, e.g.
// `pollster::block_on` in `mkui-wgpu`). This whole module — rather than just
// the one `#[test]` — is native-only: on `wasm32` nothing would be left in
// it once the assertion is cfg'd out, leaving a dangling `use super::*;`.
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn mkui_error_is_send_sync_on_native() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MkuiError>();
    }
}

#[cfg(all(test, target_arch = "wasm32"))]
mod wasm_tests {
    use super::*;
    use wasm_bindgen::JsValue;

    // Exercises the acceptance criterion directly: `MkuiError` must keep an
    // ergonomic `JsValue` conversion on wasm32 (`?` through `From`) without
    // relying on thiserror's `#[from]` error-source machinery, which
    // `JsValue` cannot satisfy (it is not `std::error::Error`).
    fn convert(result: Result<(), JsValue>) -> Result<(), MkuiError> {
        result?;
        Ok(())
    }

    #[test]
    fn js_value_converts_into_mkui_error_through_try_operator() {
        let err = convert(Err(JsValue::from_str("boom"))).unwrap_err();
        assert!(matches!(err, MkuiError::JsValue(_)));
    }

    #[test]
    fn js_value_error_display_includes_the_debug_rendering() {
        let err = MkuiError::from(JsValue::from_str("boom"));
        assert!(err.to_string().starts_with("js: "));
    }
}
