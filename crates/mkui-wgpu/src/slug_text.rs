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

/// #157 Phase 3 (Codex 8-step-plan step 6): below this cap-height, small UI
/// text gets its baseline snapped to the physical pixel grid. This codebase
/// does not parse the SFNT `OS/2.sCapHeight` table, so `run.font_size_px` is
/// used as the documented proxy metric for cap height — the threshold value
/// itself is the one dame-rubric.md's Phase 3 criteria cite verbatim.
const SMALL_TEXT_CAP_HEIGHT_PX: f32 = 16.0;

/// Round `value_px` to the nearest physical pixel at `device_pixel_ratio`,
/// then convert back to logical pixels. `device_pixel_ratio` is assumed
/// finite and positive (the renderer's own DPI derivation, mirroring #157
/// Phase 2's `half_pixel_dilation_units`, guarantees this upstream).
fn snap_to_physical_pixel(value_px: f32, device_pixel_ratio: f32) -> f32 {
    (value_px * device_pixel_ratio).round() / device_pixel_ratio
}

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
/// `device_pixel_ratio` is the physical-to-logical pixel ratio of the surface
/// this run will be drawn on (1.0 for a 1x/unscaled target). Below
/// [`SMALL_TEXT_CAP_HEIGHT_PX`], each glyph's baseline Y is snapped to the
/// nearest physical pixel (#157 Phase 3, Codex plan step 6) so small UI text
/// doesn't sit on a sub-pixel boundary and blur under the Slug lane's
/// antialiasing; text at or above the threshold is left at its unsnapped
/// position (`device_pixel_ratio` is otherwise unused for those glyphs).
pub fn place_slug_run(
    text_system: &dyn TextSystem,
    cache: &mut SlugBlobCache,
    run: &LayoutRun,
    box_origin_px: [f32; 2],
    device_pixel_ratio: f32,
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
        let mut baseline_y = box_origin_px[1] + run.line_y_baseline_px - glyph.y_offset_px;
        let cap_height_px = run.font_size_px;
        if cap_height_px < SMALL_TEXT_CAP_HEIGHT_PX {
            baseline_y = snap_to_physical_pixel(baseline_y, device_pixel_ratio);
        }

        placed.push(PlacedSlugGlyph {
            blob,
            origin_px: [pen_x, baseline_y],
            scale_px_per_unit: run.font_size_px / units_per_em,
            color: rgba,
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

    fn baseline_at(
        sys: &CompositeTextSystem,
        cache: &mut SlugBlobCache,
        run: &LayoutRun,
        box_origin_y: f32,
        device_pixel_ratio: f32,
    ) -> f32 {
        let glyphs = place_slug_run(
            sys,
            cache,
            run,
            [0.0, box_origin_y],
            device_pixel_ratio,
            Color::rgb(1.0, 1.0, 1.0),
        );
        assert_eq!(glyphs.len(), 1, "M is a single drawable Slug glyph");
        glyphs[0].origin_px[1]
    }

    // dame-rubric.md § Phase 3 (N): the snap function itself, tested in
    // isolation with exact control over the sub-pixel sweep — this is the
    // literal claim ("100 sub-pixel offsets across one physical-pixel
    // period group into exactly 2 piecewise-constant cells, split at the
    // period's midpoint").
    #[test]
    fn snap_to_physical_pixel_is_piecewise_constant_over_one_period() {
        let base = 10.0f32;
        let mut values = Vec::with_capacity(100);
        for i in 0..100 {
            let t = i as f32 / 100.0;
            values.push(snap_to_physical_pixel(base + t, 1.0));
        }
        let mut distinct = values.clone();
        distinct.dedup();
        assert_eq!(
            distinct,
            vec![10.0, 11.0],
            "expected exactly 2 cells (nearest-pixel snap), got {distinct:?}"
        );
        for (i, &v) in values.iter().enumerate() {
            let t = i as f32 / 100.0;
            let expected = if t < 0.5 { 10.0 } else { 11.0 };
            assert_eq!(
                v, expected,
                "offset {t} landed in the wrong cell (value {v})"
            );
        }
    }

    #[test]
    fn snap_to_physical_pixel_scales_grid_with_device_pixel_ratio() {
        for dpr in [1.0f32, 1.5, 2.0, 3.0] {
            let base = 10.0f32;
            let mut values = Vec::with_capacity(100);
            for i in 0..100 {
                // Sweep one physical-pixel period, expressed in logical px.
                let t = (i as f32 / 100.0) / dpr;
                values.push(snap_to_physical_pixel(base + t, dpr));
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert_eq!(
                distinct.len(),
                2,
                "device_pixel_ratio {dpr}: expected 2 cells, got {distinct:?}"
            );
            let delta = distinct[1] - distinct[0];
            assert!(
                (delta - 1.0 / dpr).abs() < 1e-4,
                "device_pixel_ratio {dpr}: cell delta {delta} should be one physical \
                 pixel (1/{dpr} logical px)"
            );
        }
    }

    // Integration-level proof that `place_slug_run` actually wires the snap
    // in (gated correctly) for a real font-backed run, rather than the math
    // being correct in isolation but never reached.
    #[test]
    fn small_text_baseline_snap_moves_in_quantized_physical_pixel_steps() {
        let (sys, run) = registered_run(12.0); // below SMALL_TEXT_CAP_HEIGHT_PX
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let device_pixel_ratio = 2.0f32;

        // Sweep 5 logical px (several physical-pixel periods at 2x) so the
        // boundary phase relative to the font's own baseline metrics doesn't
        // matter — the quantization structure is checked, not a specific
        // offset's cell membership.
        let mut values = Vec::with_capacity(100);
        for i in 0..100 {
            let box_origin_y = i as f32 * 0.05;
            values.push(baseline_at(
                &sys,
                &mut cache,
                &run,
                box_origin_y,
                device_pixel_ratio,
            ));
        }
        let mut distinct = values.clone();
        distinct.dedup();
        assert!(
            distinct.len() < values.len(),
            "small text baseline must be quantized, not continuous ({} distinct of {})",
            distinct.len(),
            values.len()
        );
        for w in distinct.windows(2) {
            let step = w[1] - w[0];
            assert!(
                (step - 1.0 / device_pixel_ratio).abs() < 1e-4,
                "quantization step {step} must equal one physical pixel \
                 (1/{device_pixel_ratio} logical px)"
            );
        }
    }

    #[test]
    fn text_at_or_above_threshold_is_never_snapped() {
        // 48px matches the "Mag" demo title (examples/text); the Phase 1/2
        // parity self-check renders at an even larger effective size
        // (96-192 logical px em). Both must be provably unaffected by the
        // Phase 3 snap.
        let (sys, run) = registered_run(48.0);
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));

        let mut values = Vec::with_capacity(100);
        for i in 0..100 {
            let box_origin_y = i as f32 * 0.01;
            values.push(baseline_at(&sys, &mut cache, &run, box_origin_y, 1.0));
        }
        let mut distinct = values.clone();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            values.len(),
            "unsnapped text must pass every sub-pixel offset through unchanged, \
             got {} distinct of {}",
            distinct.len(),
            values.len()
        );
    }

    #[test]
    fn threshold_boundary_is_exclusive_at_16px() {
        // font_size_px == SMALL_TEXT_CAP_HEIGHT_PX must NOT snap (`<`, not `<=`).
        let (sys, run) = registered_run(16.0);
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let a = baseline_at(&sys, &mut cache, &run, 3.0, 1.0);
        let b = baseline_at(&sys, &mut cache, &run, 3.03, 1.0);
        assert_ne!(
            a, b,
            "16px text (== threshold) must not be snapped; nearby offsets must remain distinct"
        );
    }
}
