//! Renderer-side glue (#67): turn an outline (`TextRenderClass::Slug`)
//! [`LayoutRun`] into placed GPU glyphs for the #66 Slug lane.
//!
//! This is the seam between the text system and the GPU: `mkui-text` produces
//! resolved outlines (font units, y-up) and `mkui-vector2d` encodes them into
//! size-independent Slug blobs; this function places each glyph at its
//! screen-pixel pen position and hands the renderer a
//! [`PlacedSlugGlyph`] it can push into a [`Scene`](crate::Scene) as a
//! [`Primitive::SlugGlyph`](crate::Primitive::SlugGlyph). The font-unit→pixel
//! scale and the y-up→y-down flip are applied downstream by
//! `mkui-vector2d-wgpu`'s packing — this function only computes the pen origin
//! and the per-unit scale.
//!
//! `mkui-text` never names a WGPU type (it emits outlines, not primitives); the
//! conversion lives here, in the renderer crate, under the `slug` feature.

use mkui_text::{Affine2Fixed, LayoutRun, OutlineKey, TextRenderClass, TextSystem};
use mkui_vector2d::{glyph_outline_to_path, SlugBlobCache, SlugGlyphKey};
use mkui_vector2d_wgpu::PlacedSlugGlyph;

use crate::types::Color;

/// Convert a Slug-lane [`LayoutRun`] into placed GPU glyphs, resolving each
/// glyph's outline through `text_system` and encoding it through `cache`.
///
/// `box_origin_px` is the screen-space (y-down) top-left of the text box the run
/// was laid out against; the run's own `origin_x_px` / `line_y_baseline_px` and
/// each glyph's offsets are added to place the glyph's font-unit origin (the pen
/// point on the baseline). Glyphs whose outline is unavailable (e.g. a fallback
/// glyph routed here by mistake) or which encode to nothing (whitespace) are
/// skipped — never dropped silently for a drawable glyph, only for genuinely
/// empty ones. Returns an empty vector for a non-Slug run.
pub fn place_slug_run(
    text_system: &dyn TextSystem,
    cache: &mut SlugBlobCache,
    run: &LayoutRun,
    box_origin_px: [f32; 2],
    color: Color,
) -> Vec<PlacedSlugGlyph> {
    if run.render_class != TextRenderClass::Slug {
        return Vec::new();
    }
    let rgba = [color.r, color.g, color.b, color.a];
    let mut placed = Vec::with_capacity(run.glyphs.len());

    for glyph in &run.glyphs {
        let key = OutlineKey {
            font_id: run.font_id,
            font_generation: run.font_generation,
            glyph_id: glyph.glyph_id,
            variations: run.variations.clone(),
            synthesis_flags: run.synthesis_flags,
            transform: Affine2Fixed::IDENTITY,
        };
        let outline = match text_system.glyph_outline(&key) {
            Ok(outline) => outline,
            // No outline for this glyph on this face: leave it to whatever lane
            // owns it rather than forging a Slug draw.
            Err(_) => continue,
        };
        let units_per_em = (outline.units_per_em.max(1)) as f32;
        let path = glyph_outline_to_path(&outline);
        let blob_key = SlugGlyphKey::from_run(run, glyph.glyph_id, Affine2Fixed::IDENTITY);
        let blob = match cache.encode(blob_key, &path) {
            Ok(blob) => blob,
            // An empty outline (e.g. the space glyph) draws nothing.
            Err(_) => continue,
        };

        // Pen origin in screen space: the glyph's font-unit (0, 0) maps to the
        // baseline point at the glyph's advance position. The adapter applies
        // `bounds * scale` and the y-flip from here.
        let pen_x = box_origin_px[0] + run.origin_x_px + glyph.x_px + glyph.x_offset_px;
        let baseline_y = box_origin_px[1] + run.line_y_baseline_px - glyph.y_offset_px;

        placed.push(PlacedSlugGlyph {
            blob,
            origin_px: [pen_x, baseline_y],
            scale_px_per_unit: run.font_size_px / units_per_em,
            color: rgba,
        });
    }
    placed
}
