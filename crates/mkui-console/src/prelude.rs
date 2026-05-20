//! Convenience prelude for console-backed apps.
//!
//! Mirrors the shape of `mkui_web::prelude` and `mkui_wgpu::prelude`:
//! one glob import brings in the cross-platform component tree from
//! [`mkui_core::prelude`] plus the console-side [`Mkui`] entry point.

pub use crate::app::ConsoleApp;
pub use crate::components::{ConsoleButton, Line};
pub use crate::high_level::Mkui;
pub use crate::renderer::ConsoleRenderer;
pub use mkui_core::prelude::*;
