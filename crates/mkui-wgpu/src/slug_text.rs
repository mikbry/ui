//! Renderer-side glue (#67, extended #171 Part B.2): turn an outline
//! (`TextRenderClass::Slug`) [`LayoutRun`] into placed GPU glyphs for the #66
//! Slug lane, and expand a component's [`Primitive::Text`] into Slug + bitmap
//! primitives when a Slug-registered face is selected.
//!
//! This is the seam between the text system and the GPU: `mkui-text` produces
//! resolved outlines (font units, y-up) and `mkui-vector2d` encodes them into
//! size-independent Slug blobs; [`place_slug_run`] places each glyph at its
//! screen-pixel pen position and hands the renderer a [`PlacedSlugGlyph`] it
//! can push into a [`Scene`](crate::Scene) as a
//! [`Primitive::SlugGlyph`](crate::Primitive::SlugGlyph). The font-unit→pixel
//! scale and the y-up→y-down flip are applied downstream by
//! `mkui-vector2d-wgpu`'s packing — this function only computes the pen origin
//! and the per-unit scale.
//!
//! [`expand_slug_text`] is #171 Part B.2's component call site: it is the
//! function a wgpu render pass drives to route `mkui-core`/`mkui-wgpu`
//! component text (every widget that renders text lowers to a
//! [`Primitive::Text`] — there is exactly one text-bearing primitive kind in
//! this renderer, so this one seam covers Button/Card/Label/Heading/whatever
//! the widget set exposes without touching how any of them are authored)
//! through the Slug lane instead of the bitmap lane.
//!
//! `mkui-text` never names a WGPU type (it emits outlines, not primitives); the
//! conversion lives here, in the renderer crate, under the `slug` feature.

use mkui_text::{
    Affine2Fixed, FontId, HintingMode, LayoutRun, LayoutSpec, OutlineKey, TextRenderClass,
    TextSystem, VariationSettings,
};
use mkui_vector2d::{extract_slug_glyph, glyph_outline_to_path, SlugBlobCache, SlugGlyphKey};
use mkui_vector2d_wgpu::PlacedSlugGlyph;

use crate::tessellation::map_align;
use crate::types::{Color, Point, Primitive, Rect, Size, Text, TextAlign, TextStyle};

/// One glyph within a Slug-class run whose extraction into a
/// [`PlacedSlugGlyph`] failed — the SFNT-to-Slug bridge
/// (`mkui_vector2d::TextExtractionError`) named a composite glyph, a
/// non-quadratic curve, or missing outline data. The caller degrades this
/// specific glyph to the bitmap lane (#171 Part B.2's fallible per-glyph
/// fallback); every other glyph in the run still renders through Slug.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitmapFallbackGlyph {
    /// Index into the source text's `chars()` (the run's
    /// [`LayoutGlyph::cluster`](mkui_text::LayoutGlyph::cluster)) — recovers
    /// the character to rasterize through the bitmap lane.
    pub cluster: u32,
    /// Pen x-position of the glyph cell, already offset by the caller's
    /// `box_origin_px` and the run's origin — the same absolute coordinate
    /// frame as [`PlacedSlugGlyph::origin_px`].
    pub pen_x_px: f32,
}

/// Output of [`place_slug_run`]: successfully placed Slug glyphs, plus any
/// glyph that failed extraction and must fall back to the bitmap lane.
/// Empty on a non-Slug run (see [`place_slug_run`]).
#[derive(Debug, Clone, Default)]
pub struct SlugRunResult {
    pub glyphs: Vec<PlacedSlugGlyph>,
    pub bitmap_fallback: Vec<BitmapFallbackGlyph>,
}

/// Convert a Slug-lane [`LayoutRun`] into placed GPU glyphs, resolving each
/// glyph's outline through `text_system` and encoding it through `cache`.
/// Returns [`SlugRunResult::default`] (both lists empty) for a non-Slug run.
///
/// `box_origin_px` is the screen-space (y-down) top-left of the text box the run
/// was laid out against; the run's own `origin_x_px` / `line_y_baseline_px` and
/// each glyph's offsets are added to place the glyph's font-unit origin (the pen
/// point on the baseline).
///
/// Each glyph is extracted through [`extract_slug_glyph`] (#171 Part B.2
/// consuming #169/#170's Part B.1 bridge) when `text_system` exposes a
/// concrete [`SfntFace`](mkui_text::SfntFace) for `run.font_id` via
/// [`TextSystem::sfnt_face`] — the bridge checks `cache` **before** decoding,
/// so a warm cache never re-decodes or re-encodes. A `TextExtractionError`
/// (composite glyph, non-quadratic curve, or missing outline data) routes the
/// glyph into [`SlugRunResult::bitmap_fallback`] instead of being dropped.
///
/// When `text_system` has no concrete `SfntFace` to offer (a generic
/// outline-only implementer, e.g. a test mock), extraction falls back to the
/// pre-#171 generic path through [`TextSystem::glyph_outline`] +
/// [`SlugBlobCache::encode_with_units_per_em`]; a failure there has no typed
/// reason to fall back by, so it is silently skipped exactly as before #171
/// (this is also where a glyph with no outline at all — e.g. one routed here
/// by mistake — lands).
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
) -> SlugRunResult {
    if run.render_class != TextRenderClass::Slug {
        return SlugRunResult::default();
    }
    let rgba = [color.r, color.g, color.b, color.a];
    let mut result = SlugRunResult::default();
    let sfnt_face = text_system.sfnt_face(run.font_id);

    for glyph in &run.glyphs {
        let blob_key = SlugGlyphKey::from_run(run, glyph.glyph_id, Affine2Fixed::IDENTITY);
        // Pen origin in screen space: the glyph's font-unit (0, 0) maps to the
        // baseline point at the glyph's advance position. The adapter applies
        // `bounds * scale` and the y-flip from here.
        let pen_x = box_origin_px[0] + run.origin_x_px + glyph.x_px + glyph.x_offset_px;
        let baseline_y = box_origin_px[1] + run.line_y_baseline_px - glyph.y_offset_px;

        let extracted = match sfnt_face {
            Some(face) => extract_slug_glyph(cache, blob_key, face)
                .ok()
                .map(|blob| (blob, face.units_per_em() as f32)),
            None => {
                let key = OutlineKey {
                    font_id: run.font_id,
                    font_generation: run.font_generation,
                    glyph_id: glyph.glyph_id,
                    variations: run.variations.clone(),
                    synthesis_flags: run.synthesis_flags,
                    transform: Affine2Fixed::IDENTITY,
                };
                text_system.glyph_outline(&key).ok().and_then(|outline| {
                    let units_per_em = outline.units_per_em.max(1) as f32;
                    let path = glyph_outline_to_path(&outline);
                    cache
                        .encode_with_units_per_em(blob_key, &path, units_per_em)
                        .ok()
                        .map(|blob| (blob, units_per_em))
                })
            }
        };

        match extracted {
            Some((blob, units_per_em)) => {
                result.glyphs.push(PlacedSlugGlyph {
                    blob,
                    origin_px: [pen_x, baseline_y],
                    scale_px_per_unit: run.font_size_px / units_per_em,
                    color: rgba,
                    cap_height_px: run.font_size_px,
                });
            }
            // `sfnt_face` was `Some` but extraction failed with a typed
            // `TextExtractionError`: degrade this one glyph to the bitmap lane
            // (#171 acceptance: fallible per-glyph fallback) instead of
            // dropping it. The `sfnt_face.is_none()` generic path has no typed
            // failure reason to distinguish "no outline" from "extraction
            // failed", so it stays silent-skip, matching pre-#171 behavior.
            None if sfnt_face.is_some() => {
                result.bitmap_fallback.push(BitmapFallbackGlyph {
                    cluster: glyph.cluster,
                    pen_x_px: pen_x,
                });
            }
            None => {}
        }
    }
    result
}

/// Rewrite `primitives`, expanding any [`Primitive::Text`] that resolves (laid
/// out under `slug_font_id`) into at least one [`TextRenderClass::Slug`] run
/// into [`Primitive::SlugGlyph`]s — #171 Part B.2's component call site: every
/// `mkui-core`/`mkui-wgpu` widget that renders text lowers to a
/// [`Primitive::Text`], so this single seam covers the whole widget set
/// without touching how any component is authored.
///
/// Both the bitmap-lane's own [`TextRenderClass::Bitmap`] runs (e.g. #67's
/// layout-time character fallback for a glyph `slug_font_id`'s face doesn't
/// map) and any per-glyph extraction failure (see [`place_slug_run`]) degrade
/// to a narrowed [`Primitive::Text`] positioned at the failing run/glyph's pen
/// position — the widget as a whole still renders, with no user-facing
/// failure.
///
/// A [`Primitive::Text`] that lays out under `slug_font_id` with **no** Slug
/// run at all (no SFNT face registered for `slug_font_id`, or every character
/// routes to the bitmap fallback) is pushed through **unchanged** — this
/// keeps the no-Slug-font-registered path byte-identical to the pre-#171
/// bitmap-only renderer (#171 acceptance: "bitmap path is the unconditional
/// fallback"). Non-`Text` primitives always pass through unchanged.
pub fn expand_slug_text(
    primitives: &[Primitive],
    slug_font_id: FontId,
    text_system: &dyn TextSystem,
    cache: &mut SlugBlobCache,
) -> Vec<Primitive> {
    let mut out = Vec::with_capacity(primitives.len());
    for primitive in primitives {
        match primitive {
            Primitive::Text(text) => {
                out.extend(expand_text_primitive(
                    text,
                    slug_font_id,
                    text_system,
                    cache,
                ));
            }
            other => out.push(other.clone()),
        }
    }
    out
}

fn expand_text_primitive(
    text: &Text,
    slug_font_id: FontId,
    text_system: &dyn TextSystem,
    cache: &mut SlugBlobCache,
) -> Vec<Primitive> {
    if text.content.is_empty() {
        return vec![Primitive::Text(text.clone())];
    }

    let line_height = text.style.line_height_px.max(1.0);
    let spec = LayoutSpec {
        font_id: slug_font_id,
        font_generation: 0,
        font_size_px: text.style.font_size_px,
        line_height_px: line_height,
        align: map_align(text.style.align),
        // The Slug-lane provider (`SfntProvider`) lays out a single unwrapped
        // line per call — see its `layout_local` doc — so there is exactly
        // one line's worth of runs to expand here regardless of this value.
        max_lines: Some(1),
        hinting: HintingMode::None,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
    };
    let runs = text_system.layout(&text.content, &spec, Some(text.rect.size.width));

    if !runs
        .iter()
        .any(|run| run.render_class == TextRenderClass::Slug)
    {
        return vec![Primitive::Text(text.clone())];
    }

    let chars: Vec<char> = text.content.chars().collect();
    let mut out = Vec::new();
    for run in &runs {
        if run.render_class == TextRenderClass::Slug {
            let result = place_slug_run(
                text_system,
                cache,
                run,
                [text.rect.origin.x, text.rect.origin.y],
                text.style.color,
            );
            out.extend(result.glyphs.into_iter().map(Primitive::SlugGlyph));
            let origin_y = text.rect.origin.y + run.origin_y_px;
            for fallback in result.bitmap_fallback {
                if let Some(&ch) = chars.get(fallback.cluster as usize) {
                    out.push(bitmap_text_primitive(
                        text,
                        fallback.pen_x_px,
                        origin_y,
                        ch.to_string(),
                    ));
                }
            }
        } else {
            // A run the composite router itself routed to bitmap (e.g. #67's
            // layout-time character fallback: an emoji the SFNT face doesn't
            // map) — draw its exact cluster span through the untouched
            // bitmap lane, one `Primitive::Text` per run.
            if let (Some(first), Some(last)) = (run.glyphs.first(), run.glyphs.last()) {
                let start = (first.cluster as usize).min(chars.len());
                let end = (last.cluster as usize + 1).min(chars.len());
                let span: String = chars[start..end].iter().collect();
                out.push(bitmap_text_primitive(
                    text,
                    text.rect.origin.x + run.origin_x_px,
                    text.rect.origin.y + run.origin_y_px,
                    span,
                ));
            }
        }
    }
    out
}

/// Build a narrowed, `Start`-aligned bitmap [`Primitive::Text`] at
/// `(origin_x_px, origin_y_px)` (already-resolved absolute positions, so no
/// re-alignment should happen) with `content`, reusing `text`'s font size,
/// line height, and color.
fn bitmap_text_primitive(
    text: &Text,
    origin_x_px: f32,
    origin_y_px: f32,
    content: String,
) -> Primitive {
    let width = (text.rect.width_end() - origin_x_px).max(1.0);
    Primitive::Text(Text {
        rect: Rect::new(
            Point::new(origin_x_px, origin_y_px),
            Size::new(width, text.style.line_height_px.max(1.0)),
        ),
        content,
        style: TextStyle {
            align: TextAlign::Start,
            ..text.style
        },
    })
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
        let result = place_slug_run(
            sys,
            cache,
            run,
            [0.0, box_origin_y],
            Color::rgb(1.0, 1.0, 1.0),
        );
        assert!(
            result.bitmap_fallback.is_empty(),
            "M must extract cleanly on Abel"
        );
        assert_eq!(result.glyphs.len(), 1, "M is a single drawable Slug glyph");
        result.glyphs.into_iter().next().unwrap()
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

    #[test]
    fn extraction_reuses_the_warm_cache_without_redecoding() {
        // #171 acceptance: "cache-hit path preserved (no per-render
        // extractions when the cache is warm)". `extract_slug_glyph` checks
        // `cache` before ever touching `SfntFace`, so a second call for the
        // same run must register as a cache hit, not a second miss.
        let (sys, run) = registered_run(16.0);
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let _ = place(&sys, &mut cache, &run, 0.0);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.hits(), 0);
        let _ = place(&sys, &mut cache, &run, 0.0);
        assert_eq!(cache.misses(), 1, "second placement must not re-decode");
        assert_eq!(cache.hits(), 1);
    }

    #[test]
    fn a_glyph_id_extraction_failure_degrades_to_bitmap_fallback_not_a_dropped_glyph() {
        // Force a typed `TextExtractionError` deterministically (an
        // out-of-`u16`-range glyph id, exactly like
        // `mkui-vector2d`'s own `sfnt_bridge` test suite) rather than relying
        // on an exotic composite-glyph fixture. Before #171 this glyph was
        // silently dropped (`continue`); it must now surface as a bitmap
        // fallback candidate instead.
        let (sys, mut run) = registered_run(16.0);
        run.glyphs[0].glyph_id = u16::MAX as u32 + 1;
        run.glyphs[0].cluster = 0;
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let result = place_slug_run(
            &sys,
            &mut cache,
            &run,
            [0.0, 0.0],
            Color::rgb(1.0, 1.0, 1.0),
        );
        assert!(
            result.glyphs.is_empty(),
            "the bogus glyph must not draw as Slug"
        );
        assert_eq!(result.bitmap_fallback.len(), 1);
        assert_eq!(result.bitmap_fallback[0].cluster, 0);
    }

    #[test]
    fn non_slug_run_returns_empty_result() {
        let (sys, mut run) = registered_run(16.0);
        run.render_class = TextRenderClass::Bitmap;
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let result = place_slug_run(
            &sys,
            &mut cache,
            &run,
            [0.0, 0.0],
            Color::rgb(1.0, 1.0, 1.0),
        );
        assert!(result.glyphs.is_empty());
        assert!(result.bitmap_fallback.is_empty());
    }

    // ---- `expand_slug_text` — #171 Part B.2's component call site ---------

    fn text_primitive(content: &str, font_size_px: f32) -> (Text, Vec<Primitive>) {
        let text = Text {
            rect: Rect::new(Point::new(4.0, 8.0), Size::new(400.0, 40.0)),
            content: content.to_string(),
            style: TextStyle {
                font: crate::types::FontFaceId(0),
                font_size_px,
                line_height_px: font_size_px * 1.25,
                color: Color::rgb(1.0, 1.0, 1.0),
                align: TextAlign::Start,
            },
        };
        let primitives = vec![Primitive::Text(text.clone())];
        (text, primitives)
    }

    #[test]
    fn a_fully_mapped_string_expands_entirely_to_slug_glyphs() {
        let mut sys = CompositeTextSystem::new();
        let font_id = sys
            .register_sfnt_face(std::sync::Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
            .unwrap();
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let (_, primitives) = text_primitive("Mag", 16.0);

        let expanded = expand_slug_text(&primitives, font_id, &sys, &mut cache);
        let slug_count = expanded
            .iter()
            .filter(|p| matches!(p, Primitive::SlugGlyph(_)))
            .count();
        let bitmap_count = expanded
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();
        assert_eq!(slug_count, 3, "M, a, g are each one drawable Slug glyph");
        assert_eq!(
            bitmap_count, 0,
            "a fully-mapped string needs no bitmap fallback"
        );
    }

    #[test]
    fn a_layout_time_bitmap_fallback_run_becomes_a_narrowed_bitmap_text_primitive() {
        // "M中": Abel maps 'M' but not '中', so the composite router's own
        // layout-time fallback (#67, distinct from #171's extraction-time
        // fallback) splits this into one Slug run and one Bitmap-class run —
        // exactly the mixed-run scenario `render::sfnt_slug_gpu`'s GPU test
        // exercises, but asserted here at the primitive level with no GPU.
        let mut sys = CompositeTextSystem::new();
        let font_id = sys
            .register_sfnt_face(std::sync::Arc::from(ABEL.to_vec().into_boxed_slice()), 0)
            .unwrap();
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let (_, primitives) = text_primitive("M中", 16.0);

        let expanded = expand_slug_text(&primitives, font_id, &sys, &mut cache);
        let slug_count = expanded
            .iter()
            .filter(|p| matches!(p, Primitive::SlugGlyph(_)))
            .count();
        assert_eq!(slug_count, 1, "M is the single Slug glyph");

        let bitmap_texts: Vec<&Text> = expanded
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .collect();
        assert_eq!(
            bitmap_texts.len(),
            1,
            "中 is the single bitmap-fallback span"
        );
        assert_eq!(bitmap_texts[0].content, "中");
        // The narrowed primitive starts to the right of the Slug glyph's
        // advance, inside the original box — never re-anchored at the box
        // origin (which would overlap the Slug glyph).
        assert!(bitmap_texts[0].rect.origin.x > 4.0);
    }

    #[test]
    fn no_slug_font_registered_passes_the_primitive_through_unchanged() {
        // FontId::BITMAP never carries an SfntFace, so every run comes back
        // Bitmap-class and the primitive must be pushed through byte-for-byte
        // — the #171 acceptance criterion that the bitmap path stays the
        // unconditional fallback when there's nothing to route to Slug.
        let sys = CompositeTextSystem::new();
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let (text, primitives) = text_primitive("Mag", 16.0);

        let expanded = expand_slug_text(&primitives, FontId::BITMAP, &sys, &mut cache);
        assert_eq!(expanded, vec![Primitive::Text(text)]);
    }

    #[test]
    fn non_text_primitives_pass_through_unchanged() {
        let sys = CompositeTextSystem::new();
        let mut cache = SlugBlobCache::new(SlugConfig::new(16, 16, 1));
        let quad = Primitive::Quad(crate::types::Quad {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(1.0, 1.0)),
            fill: Color::rgb(1.0, 1.0, 1.0),
            corner_radii: crate::types::CornerRadii::all(0.0),
            stroke: None,
        });
        let primitives = vec![quad.clone()];
        let expanded = expand_slug_text(&primitives, FontId::BITMAP, &sys, &mut cache);
        assert_eq!(expanded, vec![quad]);
    }
}
