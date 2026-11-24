//! Renderer-neutral glyph-outline request/data contract.
//!
//! These types let a [`TextSystem`](crate::TextSystem) hand a fully-resolved
//! vector outline to an encoder (#65) or rasterizer (#67) without leaking any
//! renderer resource into `mkui-text`. They are text/font domain types and do
//! **not** depend on `mkui-vector2d`.
//!
//! A [`GlyphOutline`] returned for an [`OutlineKey`] is **fully resolved**: the
//! provider applies the key's variation coordinate, synthesis flags, and
//! outline-local affine transform exactly once, and the returned
//! [`ink_bounds`](GlyphOutline::ink_bounds) match the returned points.

use crate::canonical::{Affine2Fixed, VariationSettings};
use crate::font_id::FontId;

/// Cache/request key uniquely identifying one resolved glyph outline.
///
/// Every field that can change the resolved outline appears here, in canonical
/// form, so two distinct outlines can never alias onto one key:
/// - `font_id` / `font_generation` — which face, at which mutation generation.
/// - `glyph_id` — the glyph within that face.
/// - `variations` — canonical, tag-sorted variation coordinate.
/// - `synthesis_flags` — synthetic bold/italic and similar.
/// - `transform` — the outline-local affine transform (translation included).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineKey {
    pub font_id: FontId,
    pub font_generation: u32,
    pub glyph_id: u32,
    pub variations: VariationSettings,
    pub synthesis_flags: u32,
    pub transform: Affine2Fixed,
}

/// One contour-drawing command, in font-space units, **y-up**.
///
/// Coordinates are in the design grid described by
/// [`GlyphOutline::units_per_em`]. `#[non_exhaustive]` so a future cubic
/// command can be added without breaking the contract.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum OutlineCommand {
    /// Begin a new contour at `(x, y)`.
    MoveTo { x: f32, y: f32 },
    /// Straight line from the current point to `(x, y)`.
    LineTo { x: f32, y: f32 },
    /// Quadratic Bézier through control `(cx, cy)` to `(x, y)`.
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    /// Close the current contour back to its start point.
    Close,
}

/// Axis-aligned ink bounds of a resolved outline, in font-space units, y-up.
///
/// These bounds reflect the outline **after** variation, synthesis, and the
/// outline-local affine transform have been applied — they always match the
/// emitted [`commands`](GlyphOutline::commands).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OutlineBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// A fully-resolved glyph outline.
///
/// The contour commands are expressed in a documented font-space, y-up
/// convention (positive y points up, away from the baseline). The provider has
/// already applied the [`OutlineKey`]'s variation, synthesis, and
/// outline-local affine transform exactly once; consumers do not re-resolve
/// them.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphOutline {
    /// Font design units per em (the grid the coordinates live in).
    pub units_per_em: u16,
    /// Ink bounds of the resolved outline — matches `commands`.
    pub ink_bounds: OutlineBounds,
    /// Contour commands in font-space, y-up order.
    pub commands: Vec<OutlineCommand>,
}
