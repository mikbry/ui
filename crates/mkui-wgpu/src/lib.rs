#![forbid(unsafe_code)]
//! # mkui-wgpu — WGPU/scene backend for mkui
//!
//! Shared GUI primitives, theme, and backend scaffolding for mkui WGPU surfaces.
//!
//! The crate is domain-free — it depends on nothing above the standard
//! library and does not know about projects, timelines, tools, or any
//! product-specific concept. WGPU-facing shells and overlays compose from here.
//!
//! ## Module layout (aligned with [`mkui_web`] / [`mkui_console`])
//!
//! Every backend in the workspace exposes the same five-module shape so the
//! bridge crate ([`mkui`]) can dispatch against a uniform surface and
//! follow-up backends have a template to copy. For the WGPU backend that
//! shape is filled by [`app`] / [`renderer`] / [`components`] /
//! [`high_level`] / [`prelude`], with a few scene-specific extras
//! ([`builder`], [`theme`], [`types`]) layered on top.
//!
//! ## Layers
//!
//! - [`types`]        — primitives (Point / Rect / Color / Scene / Primitive),
//!   text & font handles, `HitRegion<T>`, `PanelLayout<T>`.
//! - [`theme`]        — [`HudTheme`], [`ButtonVariant`], [`ButtonSize`],
//!   [`ButtonState`], [`TextVariant`], and cva-style style resolvers.
//!   Mirrors shadcn's variant / size / state model.
//! - [`components`]   — scene builders: [`components::card`],
//!   [`components::button`], [`components::input`], [`components::slider`],
//!   [`components::chip_group`], [`components::scrollbar`],
//!   [`components::swatch`], [`components::heading`], [`components::text`], plus the
//!   [`components::panel`] / [`components::titled_panel`] primitives the
//!   higher-level components are built from. Mirrors the component surface
//!   of [`miklabs/ui`] (`mkui-core::components`) — `View` / `Text` /
//!   `Button` / `Toggle` with variant + size.
//! - [`builder`]      — [`UiBuilder<T>`], [`NumberRow`], [`ListRow`]: a
//!   declarative immediate-mode layer above `components`. Overlay panels
//!   describe rows as data (`NumberRow { … }`, `ListRow { … }`) rather than
//!   hand-rolling per-row drawing helpers. Mirrors
//!   `mkui-core::View` / `Stack` / `Row`.
//! - [`app`] / [`renderer`] / [`high_level`] — backend-oriented wrappers so
//!   the crate can evolve like `mkui-web` and `mkui-console`, rather than
//!   staying a loose pile of scene helpers.
//! - [`prelude`]      — one-glob import for scene builders: builder, theme
//!   variants, geometry. Target shape is `use mkui_wgpu::prelude::*;`
//!   so scene-based hosts read like `mkui::prelude::*` consumers.
//! - `tessellation`   — private: `Scene` → triangles and the bitmap-glyph
//!   fallback used until the SDF atlas lands.
//!
//! [`miklabs/ui`]: https://github.com/mikbry/ui
//! [`mkui`]: https://docs.rs/mkui
//! [`mkui_web`]: https://docs.rs/mkui-web
//! [`mkui_console`]: https://docs.rs/mkui-console

pub mod app;
pub mod badge;
pub mod builder;
pub mod components;
pub mod dot;
pub mod high_level;
pub mod prelude;
#[cfg(not(target_arch = "wasm32"))]
pub mod render;
pub mod renderer;
pub mod theme;
pub mod types;

mod tessellation;

pub use app::WgpuApp;
pub use high_level::Mkui;
#[cfg(not(target_arch = "wasm32"))]
pub use render::{RenderOutcome, Renderer};
pub use renderer::WgpuRenderer;

// Low-level types
pub use types::{
    rect_contains, AtlasRect, Axis, Color, Constraints, CornerRadii, DotAnimation,
    DotAnimationInstance, Element, FontFaceId, GlyphKey, GlyphPlacement, GuiTriangle, HitRegion,
    Icon, IconId, IconMask, Insets, PanelLayout, Point, Primitive, Quad, RasterizedGlyph, Rect,
    Scene, Shadow, Size, StackCursor, StackStyle, Stroke, Text, TextAlign, TextBuffer, TextStyle,
};

// Theme + cva-style variant system
pub use theme::{
    BadgeSize, BadgeStyle, BadgeVariant, ButtonSize, ButtonState, ButtonStyle, ButtonVariant,
    CardStyle, DotSize, DotStyle, DotVariant, HudTheme, InputStyle, PanelStyle, ScrollbarStyle,
    ShadowStyle, SliderStyle, SwatchStyle, TextVariant, ThemeTokens,
};

// Components (shadcn / miklabs-ui aligned)
pub use components::SliderRegions;

// Declarative builder layer (immediate-mode mirror of mkui-core's retained
// `View` / `Stack` / `Row` tree).
pub use builder::{ListRow, NumberRow, UiBuilder};

pub use tessellation::{tessellate_scene, tessellate_scene_with_text};

/// Convenience alias: the flat `widgets` module lives on as a
/// re-export of `components`. New code should use `components::*` directly.
pub mod widgets {
    pub use super::components::*;
}
