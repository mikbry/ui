//! Glyph-outline input contract and conversion into the path model.
//!
//! The [`GlyphOutline`] / [`OutlineKey`] / [`OutlineCommand`] types mirror the
//! renderer-neutral outline contract owned by `mkui-text` (issue #61). They are
//! reproduced here (documented as a local mirror) so `mkui-vector2d` is
//! buildable and testable before #61 lands; the integration follow-up replaces
//! them with re-exports from `mkui-text`.
//!
//! A [`GlyphOutline`] is **fully resolved** for its [`OutlineKey`]: the
//! provider has already applied variation, synthesis, and the outline-local
//! affine transform exactly once, and the returned `bounds` match the returned
//! points. Conversion here is therefore a faithful 1:1 copy — it does **not**
//! reapply variation, synthesis, transform, or recompute bounds, and it does
//! not touch text/font ownership.
//!
//! Coordinates are font units, **y-up**. Screen-space y-down conversion and GPU
//! packing belong to #66, never here.

use crate::fixed::{Affine2Fixed, VariationSettings};
use crate::path::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
use mkui_text::FontId;

/// Renderer-neutral outline request identity (mirror of #61's `mkui-text`
/// type). Carries the global [`FontId`], font generation, glyph id, canonical
/// variations, synthesis flags, and the canonical outline-local affine.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OutlineKey {
    pub font_id: FontId,
    pub font_generation: u32,
    pub glyph_id: u32,
    pub variation_axes: VariationSettings,
    pub synthesis_flags: u32,
    pub outline_transform: Affine2Fixed,
}

/// Ink bounds of a resolved glyph outline, in font units (y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutlineBounds {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

impl OutlineBounds {
    pub const fn new(x_min: f32, y_min: f32, x_max: f32, y_max: f32) -> Self {
        Self {
            x_min,
            y_min,
            x_max,
            y_max,
        }
    }
}

/// One resolved outline command in font-space y-up. The provider only ever
/// emits move/line/quadratic/close — there is no cubic variant in the outline
/// contract (cubics, if introduced, are converted upstream).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum OutlineCommand {
    MoveTo { x: f32, y: f32 },
    LineTo { x: f32, y: f32 },
    QuadTo { cx: f32, cy: f32, x: f32, y: f32 },
    Close,
}

/// A fully resolved glyph outline (mirror of #61's `mkui-text` type).
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphOutline {
    pub units_per_em: u16,
    pub bounds: OutlineBounds,
    pub commands: Vec<OutlineCommand>,
}

impl GlyphOutline {
    /// Convert this resolved outline into a backend-neutral [`VectorPath`].
    ///
    /// The conversion is a faithful 1:1 copy of the already-resolved commands;
    /// `bounds` is carried through verbatim (not recomputed) and the fill rule
    /// is the glyph-standard non-zero winding.
    pub fn to_vector_path(&self) -> VectorPath {
        let commands = self
            .commands
            .iter()
            .map(|c| match *c {
                OutlineCommand::MoveTo { x, y } => PathCommand::MoveTo(Vec2::new(x, y)),
                OutlineCommand::LineTo { x, y } => PathCommand::LineTo(Vec2::new(x, y)),
                OutlineCommand::QuadTo { cx, cy, x, y } => PathCommand::QuadTo {
                    control: Vec2::new(cx, cy),
                    to: Vec2::new(x, y),
                },
                OutlineCommand::Close => PathCommand::Close,
            })
            .collect();
        let bounds = Bounds::new(
            Vec2::new(self.bounds.x_min, self.bounds.y_min),
            Vec2::new(self.bounds.x_max, self.bounds.y_max),
        );
        VectorPath::new(commands, FillRule::NonZero, bounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_is_one_to_one_and_preserves_bounds() {
        let outline = GlyphOutline {
            units_per_em: 1000,
            bounds: OutlineBounds::new(0.0, 0.0, 500.0, 700.0),
            commands: vec![
                OutlineCommand::MoveTo { x: 0.0, y: 0.0 },
                OutlineCommand::QuadTo {
                    cx: 250.0,
                    cy: 700.0,
                    x: 500.0,
                    y: 0.0,
                },
                OutlineCommand::LineTo { x: 0.0, y: 0.0 },
                OutlineCommand::Close,
            ],
        };
        let path = outline.to_vector_path();
        assert_eq!(path.commands.len(), 4);
        assert_eq!(path.fill, FillRule::NonZero);
        assert_eq!(path.bounds.min, Vec2::new(0.0, 0.0));
        assert_eq!(path.bounds.max, Vec2::new(500.0, 700.0));
        assert_eq!(
            path.commands[1],
            PathCommand::QuadTo {
                control: Vec2::new(250.0, 700.0),
                to: Vec2::new(500.0, 0.0),
            }
        );
    }
}
