#![forbid(unsafe_code)]
//! # mkui-core — shared component contract for every mkui backend
//!
//! `mkui-core` is the *contract crate* of the mkui workspace. It owns the
//! types every backend agrees on:
//!
//! - **Components** ([`components::Component`], [`components::View`],
//!   [`components::Text`], [`components::Button`]) — the renderable tree.
//! - **Headless logic** ([`headless`]) — state, events, and a11y helpers for
//!   interactive components that are identical across backends.
//! - **Theme contracts** ([`theme::Theme`], [`theme::ThemeMode`],
//!   [`theme::ColorTheme`]) — backend-agnostic theme values.
//! - **Layout contracts** ([`layout::Layout`], [`layout::FlexDirection`], ...)
//!   — abstract flex layout primitives.
//! - **Input contracts** ([`input::InputEvent`], [`input::Key`], ...) — the
//!   normalized event stream every backend feeds into headless components.
//! - **Style classes** ([`style::StyleClass`]) — the utility-class string
//!   format shared by every backend.
//! - **Error types** ([`error::MkuiError`]).
//!
//! ## What does *not* belong here
//!
//! `mkui-core` deliberately avoids depending on `wasm-bindgen`, `web-sys`,
//! `crossterm`, `ratatui`, `wgpu`, or any other backend-facing crate. The
//! whole point of the contract crate is that each backend
//! (`mkui-web`, `mkui-console`, `mkui-native`, ...) can compile against the
//! same model without pulling the others in.
//!
//! Rendering logic, terminal styles, DOM construction, and GPU scene
//! plumbing live in their respective backend crates. If you find yourself
//! reaching for a backend type while editing `mkui-core`, that is the cue to
//! move the code out of core.

pub mod components;
pub mod error;
pub mod event;
pub mod headless;
pub mod input;
pub mod layout;
pub mod state;
pub mod style;
pub mod theme;

pub mod prelude {
    // Components (high-level cross-platform components)
    pub use crate::components::{Button, Component, Mkui, Text, View};

    // Headless components (logic-only, no rendering)
    pub use crate::headless::{
        Button as HeadlessButton, ButtonBuilder, ButtonSize, ButtonVariant, Focusable,
        KeyboardInteractable, Text as HeadlessText, TextAlign, TextBuilder, TextVariant,
        TextWeight, Toggle, ToggleBuilder,
    };

    // Error handling
    pub use crate::error::{MkuiError, MkuiErrorKind};

    // Other modules
    pub use crate::event::*;
    pub use crate::input::*;
    pub use crate::layout::*;
    pub use crate::state::*;
    pub use crate::style::*;
    pub use crate::theme::*;
}
