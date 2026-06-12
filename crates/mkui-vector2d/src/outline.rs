//! Conversion from `mkui-text`'s resolved glyph-outline contract into the
//! backend-neutral path model.
//!
//! The outline request/data types ([`OutlineKey`], [`GlyphOutline`],
//! [`OutlineCommand`], [`OutlineBounds`]) and the canonical identity values are
//! **owned by `mkui-text`** (#61) — they are text/font domain types and are
//! re-exported here purely for consumer convenience. This module adds only the
//! glue: turning a fully-resolved [`GlyphOutline`] into a [`VectorPath`].
//!
//! A [`GlyphOutline`] is already fully resolved for its [`OutlineKey`]: the
//! provider applied variation, synthesis, and the outline-local affine exactly
//! once, and its `ink_bounds` match its points. Conversion here is therefore a
//! faithful 1:1 copy — it does **not** reapply variation, synthesis, transform,
//! or recompute bounds, and it does not touch text/font ownership.
//!
//! Coordinates are font units, **y-up**. Screen-space y-down conversion and GPU
//! packing belong to #66, never here.

use crate::path::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
use mkui_text::{GlyphOutline, OutlineBounds, OutlineCommand};

/// Convert a fully-resolved [`GlyphOutline`] into a backend-neutral
/// [`VectorPath`].
///
/// The conversion is a faithful 1:1 copy of the already-resolved commands;
/// `ink_bounds` is carried through verbatim (not recomputed) and the fill rule
/// is the glyph-standard non-zero winding.
pub fn glyph_outline_to_path(outline: &GlyphOutline) -> VectorPath {
    let commands = outline
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
            // `OutlineCommand` is `#[non_exhaustive]`; a future cubic command
            // would be representable by the path model but is rejected by the
            // Sprint 7 glyph encoder (see `crate::slug`).
            other => unreachable!("unhandled outline command: {other:?}"),
        })
        .collect();
    let OutlineBounds {
        min_x,
        min_y,
        max_x,
        max_y,
    } = outline.ink_bounds;
    let bounds = Bounds::new(Vec2::new(min_x, min_y), Vec2::new(max_x, max_y));
    VectorPath::new(commands, FillRule::NonZero, bounds)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversion_is_one_to_one_and_preserves_bounds() {
        let outline = GlyphOutline {
            units_per_em: 1000,
            ink_bounds: OutlineBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 500.0,
                max_y: 700.0,
            },
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
        let path = glyph_outline_to_path(&outline);
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
