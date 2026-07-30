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
///
/// Each placed glyph's `cap_height_px` is set from `run.font_size_px` (this
/// codebase doesn't parse the SFNT `OS/2.sCapHeight` table, so nominal font
/// size is the documented proxy metric) and `origin_px` is left **unsnapped**
/// — #157 Phase 3's small-text baseline snap is applied downstream in
/// `mkui-vector2d-wgpu`'s `pack`, where the frame's fresh `device_pixel_ratio`
/// is available every render (Codex round 1 of the Phase 3 PR review
/// correctly rejected an earlier revision that baked a caller-supplied DPR in
/// here, at scene-construction time, before the real per-frame DPR is known
/// and with no re-snap on a DPI change).
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
        // #157 Phase 2: normalize the band overlap epsilon against this
        // face's *real* units-per-em, not the cache's default `1.0` — a
        // 2048-upem face would otherwise get an epsilon 2048x smaller than
        // intended. `blob_key` already carries `font_id`, so a cache shared
        // across faces with different units-per-em still can't alias blobs.
        let blob = match cache.encode_with_units_per_em(blob_key, &path, units_per_em) {
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
            cap_height_px: run.font_size_px,
        });
    }
    placed
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_text::{CompositeTextSystem, LayoutSpec};
    use mkui_vector2d::SlugConfig;

    const ABEL: &[u8] = include_bytes!("../../mkui-text/tests/fixtures/abel/Abel-Regular.ttf");

    /// Register Abel as a Slug-routed face and lay out a single-glyph "M" run
    /// at `font_size_px`.
    fn registered_run(font_size_px: f32) -> (CompositeTextSystem, LayoutRun) {
        let mut sys = CompositeTextSystem::new();
        let id = sys
            .register_sfnt_face(std::sync::Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
            .expect("Abel registers");
        let spec = LayoutSpec {
            font_id: id,
            font_size_px,
            ..Default::default()
        };
        let runs = sys.layout("M", &spec, None);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].render_class, TextRenderClass::Slug);
        (sys, runs.into_iter().next().unwrap())
    }

    fn place(
        sys: &CompositeTextSystem,
        cache: &mut SlugBlobCache,
        run: &LayoutRun,
        box_origin_y: f32,
    ) -> PlacedSlugGlyph {
        let glyphs = place_slug_run(
            sys,
            cache,
            run,
            [0.0, box_origin_y],
            Color::rgb(1.0, 1.0, 1.0),
        );
        assert_eq!(glyphs.len(), 1, "M is a single drawable Slug glyph");
        glyphs.into_iter().next().unwrap()
    }

    #[test]
    fn cap_height_px_reflects_the_run_font_size() {
        // #157 Phase 3: `place_slug_run` no longer snaps the baseline itself
        // (that moved to `mkui-vector2d-wgpu::pack`, where the frame's fresh
        // `device_pixel_ratio` is available) — it only tags each placed
        // glyph with the cap-height proxy the adapter gates on.
        let (sys, run) = registered_run(12.0);
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let glyph = place(&sys, &mut cache, &run, 10.3);
        assert_eq!(glyph.cap_height_px, 12.0);
    }

    #[test]
    fn origin_px_is_never_snapped_here_regardless_of_font_size() {
        // Snapping now happens downstream in `pack`, using a fresh
        // per-frame `device_pixel_ratio` this function never sees. Every
        // sub-pixel offset must pass straight through unchanged, for both
        // small (12px) and large (48px, matching the "Mag" demo) text — this
        // is what makes the fix immune to the staleness Codex round 1
        // flagged (a caller-supplied DPR baked in before the real one is
        // known, and never re-applied on a DPI change).
        for font_size_px in [12.0f32, 48.0] {
            let (sys, run) = registered_run(font_size_px);
            let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
            let mut values = Vec::with_capacity(50);
            for i in 0..50 {
                let box_origin_y = i as f32 * 0.01;
                values.push(place(&sys, &mut cache, &run, box_origin_y).origin_px[1]);
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert_eq!(
                distinct.len(),
                values.len(),
                "font_size_px {font_size_px}: origin_px must pass every sub-pixel \
                 offset through unchanged, got {} distinct of {}",
                distinct.len(),
                values.len()
            );
        }
    }
}
