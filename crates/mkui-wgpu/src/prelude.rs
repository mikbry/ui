//! Convenience prelude for scene UI authors.
//!
//! Matches the shape of `mkui::prelude::*` in [`miklabs/ui`]: one glob import
//! brings in the builder, the theme variants, and the geometry types a
//! scene-based panel typically needs, so call sites stay terse:
//!
//! ```
//! use mkui_wgpu::prelude::*;
//! # let theme = HudTheme::default();
//! # let mut scene = Scene::new(Size::new(320.0, 200.0));
//! # let mut layout: PanelLayout<()> = PanelLayout::default();
//! # let content = Rect::new(Point::new(10.0, 10.0), Size::new(300.0, 180.0));
//! let mut ui = UiBuilder::<()>::new(&mut scene, &mut layout, &theme, content);
//! ui.heading("Inspector").subheading("Terrain patch");
//! ```
//!
//! Keep individual paths (e.g. `components::titled_panel`) for one-off
//! escape hatches where the builder does not cover a shape yet.
//!
//! [`miklabs/ui`]: https://github.com/mikbry/ui

pub use crate::app::WgpuApp;
pub use crate::builder::{ListRow, NumberRow, UiBuilder};
pub use crate::components;
pub use crate::high_level::Mkui;
pub use crate::renderer::WgpuRenderer;
pub use crate::theme::{ButtonSize, ButtonState, ButtonVariant, HudTheme, PanelStyle, TextVariant};
pub use crate::types::{Axis, Color, HitRegion, PanelLayout, Point, Rect, Scene, Size, TextStyle};
