#![forbid(unsafe_code)]
//! # mkui-web — WebAssembly/DOM backend for mkui
//!
//! `mkui-web` is one of the *backend crates* of the mkui workspace. It
//! consumes the shared component contract defined in [`mkui_core`] and
//! turns it into a live DOM tree via `wasm-bindgen` and `web-sys`.
//!
//! ## Module layout (aligned with [`mkui_console`] / [`mkui_wgpu`])
//!
//! Every backend exposes the same five-module shape so the bridge crate
//! ([`mkui`]) can dispatch against a uniform surface and follow-up backends
//! have a template to copy:
//!
//! - [`app`] — backend app object ([`app::WebApp`]); long-lived state
//!   (theme mode + color theme, persisted to local storage).
//! - [`renderer`] — output surface ([`renderer::WebRenderer`]); the only
//!   place that owns the root DOM element and handles mounting/clearing.
//! - [`components`] — backend-specific projections of the
//!   [`mkui_core::components`] tree ([`components::WebButton`],
//!   [`components::WebToggle`]) that pair headless logic with concrete
//!   `web_sys::Element` wrappers.
//! - [`high_level`] — the [`Mkui`] entry point that composes the three
//!   above; this is what the bridge crate ([`mkui`]) re-exports.
//! - [`prelude`] — one-glob import for end users.
//!
//! The [`utils`] module contains tiny `window()` / `document()` helpers
//! shared by every layer.
//!
//! ## Extending the renderer
//!
//! Dispatch goes through the [`render::WebRendererRegistry`] in [`render`].
//! Downstream crates that ship their own components implement
//! [`render::WebRenderable`] on their types and call
//! [`Mkui::register`](high_level::Mkui::register) at app construction; the
//! built-in [`mkui_core::components::View`] /
//! [`Text`](mkui_core::components::Text) /
//! [`Button`](mkui_core::components::Button) plus the in-crate
//! [`ThemeSelector`](high_level::ThemeSelector) all flow through the same
//! path. See [`render`] for the full contract.
//!
//! ## What does *not* belong here
//!
//! Cross-platform component types, headless logic, theme primitives, layout
//! contracts, and input event shapes live in [`mkui_core`]. The web backend
//! depends on the contract crate — it never redefines those types.
//!
//! [`mkui`]: https://docs.rs/mkui
//! [`mkui_console`]: https://docs.rs/mkui-console
//! [`mkui_wgpu`]: https://docs.rs/mkui-wgpu

pub mod app;
pub mod components;
pub mod high_level;
pub mod prelude;
pub mod render;
pub mod renderer;
pub mod utils;

pub use app::WebApp;
pub use components::{WebButton, WebToggle};
pub use high_level::{Mkui, ThemeSelector};
pub use render::{CustomWebRenderable, WebRendererRegistry};
pub use renderer::WebRenderer;
