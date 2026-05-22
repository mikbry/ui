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
    #[cfg(target_arch = "wasm32")]
    #[error("js: {0:?}")]
    JsValue(#[from] wasm_bindgen::JsValue),

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
