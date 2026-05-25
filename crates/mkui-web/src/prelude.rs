//! Convenience prelude for web-backed apps.
//!
//! Mirrors the shape of `mkui_console::prelude` and `mkui_wgpu::prelude`:
//! one glob import brings in the cross-platform component tree from
//! [`mkui_core::prelude`] plus the web-side [`Mkui`] entry point and
//! component wrappers.

pub use crate::app::WebApp;
pub use crate::components::{WebButton, WebToggle};
pub use crate::high_level::Mkui;
pub use crate::render::{CustomWebRenderable, WebRendererRegistry};
pub use crate::renderer::WebRenderer;
pub use crate::utils::*;
pub use mkui_core::prelude::*;
