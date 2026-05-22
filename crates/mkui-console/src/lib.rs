#![forbid(unsafe_code)]
//! # mkui-console — terminal/TUI backend for mkui
//!
//! `mkui-console` is one of the *backend crates* of the mkui workspace. It
//! consumes the shared component contract defined in [`mkui_core`] and
//! turns it into terminal output via `crossterm`.
//!
//! ## Module layout (aligned with [`mkui_web`] / [`mkui_wgpu`])
//!
//! Every backend exposes the same five-module shape so the bridge crate
//! ([`mkui`]) can dispatch against a uniform surface and follow-up backends
//! have a template to copy:
//!
//! - [`app`] — backend app object ([`app::ConsoleApp`]); long-lived
//!   state (terminal size, selection cursor) that survives between frames.
//! - [`renderer`] — output surface ([`renderer::ConsoleRenderer`]); the
//!   only place that talks to `crossterm` / `stdout`.
//! - [`components`] — backend-specific projections of the
//!   [`mkui_core::components`] tree ([`components::ConsoleButton`],
//!   [`components::Line`], and the [`components::flatten_component`] /
//!   [`components::collect_buttons_in`] walkers).
//! - [`high_level`] — the [`Mkui`] entry point that composes the three
//!   above; this is what the bridge crate ([`mkui`]) re-exports.
//! - [`prelude`] — one-glob import for end users.
//!
//! ## What does *not* belong here
//!
//! Cross-platform component types, headless logic, theme primitives, layout
//! contracts, and input event shapes live in [`mkui_core`]. The console
//! backend depends on the contract crate — it never redefines those types.
//!
//! [`mkui`]: https://docs.rs/mkui
//! [`mkui_web`]: https://docs.rs/mkui-web
//! [`mkui_wgpu`]: https://docs.rs/mkui-wgpu

pub mod app;
pub mod components;
pub mod high_level;
pub mod prelude;
pub mod renderer;

pub use app::ConsoleApp;
pub use components::{ConsoleButton, Line};
pub use high_level::Mkui;
pub use renderer::ConsoleRenderer;
