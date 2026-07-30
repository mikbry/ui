//! Reference-adapter parity self-check for #157 Phase 1.
//!
//! Renders mkui's `SlugAdapter` output for the eight ratified glyph fixtures
//! at three DPIs and diffs against the ratified reference adapter's committed
//! known-good goldens (`docs/chevalier/mkui-slug-rewrite/reference-harness/`,
//! read-only — this module never writes there). This is the chevalier's own
//! pre-dispatch numeric gate mirroring dame-rubric.md § Phase 1 (N) criteria;
//! it is not itself the ratified adapter and does not replace dame's own
//! Lavapipe-side regeneration.
//!
//! mkui's production pipeline emits straight-alpha (`color.rgb`,
//! `color.a * coverage`) and blends it with standard alpha blending. Drawn
//! once, with an opaque-white fill, over a transparent-black clear, that
//! blend arithmetic reduces to `(coverage, coverage, coverage, coverage)` —
//! exactly the reference adapter's premultiplied `color * coverage` output —
//! so the two are byte-comparable with no separate premultiply step and no
//! change to the production blend state.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use mkui_vector2d::{BandRange, GlyphBounds, SlugCurve, SlugGlyph, Vec2};
use mkui_vector2d_wgpu::{PlacedSlugGlyph, SlugAdapter};

use super::offscreen::{OffscreenRenderer, BYTES_PER_PIXEL};

const GLYPH_NAMES: [&str; 8] = ["H", "A", "V", "M", "g", "o", "plus", "pipe"];
const DPI_CASES: [(f32, &str); 3] = [(1.0, "1x"), (1.5, "1.5x"), (2.0, "2x")];

// Reference-harness camera constants (reference-harness/src/main.rs).
const BASE_EM_PIXELS: f32 = 96.0;
const BASE_CANVAS_PIXELS: f32 = 128.0;
const BASE_PADDING_PIXELS: f32 = 16.0;

// dame-rubric.md § Threshold calibration — Phase 1 BLESS thresholds.
const MAX_CHANNEL_DELTA: u8 = 4;
const MAX_DIFFERING_PIXELS: usize = 10;
const MIN_SSIM: f64 = 0.995;

fn harness_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/chevalier/mkui-slug-rewrite/reference-harness")
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> u32 {
    let end = *cursor + 4;
    let value = u32::from_le_bytes(bytes[*cursor..end].try_into().unwrap());
    *cursor = end;
    value
}

fn read_f32(bytes: &[u8], cursor: &mut usize) -> f32 {
    f32::from_bits(read_u32(bytes, cursor))
}

fn read_band_table(bytes: &[u8], cursor: &mut usize) -> (Vec<BandRange>, Vec<u32>) {
    let band_count = read_u32(bytes, cursor) as usize;
    let mut bands = Vec::with_capacity(band_count);
    for _ in 0..band_count {
        bands.push(BandRange {
            lower: read_f32(bytes, cursor),
            upper: read_f32(bytes, cursor),
            first_curve: read_u32(bytes, cursor),
            curve_count: read_u32(bytes, cursor),
        });
    }
    let index_count = read_u32(bytes, cursor) as usize;
    let mut indices = Vec::with_capacity(index_count);
    for _ in 0..index_count {
        indices.push(read_u32(bytes, cursor));
    }
    (bands, indices)
}

/// Parse a `.slug` fixture: the public `SlugGlyph::to_le_bytes` layout
/// (revision, bounds, curves, horizontal table, vertical table). Deliberately
/// test-local rather than a `from_le_bytes` addition to `mkui-vector2d`'s
/// public API, which is frozen for this mission.
fn read_glyph(path: &Path) -> SlugGlyph {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
    let mut cursor = 0usize;
    let revision = read_u32(&bytes, &mut cursor);
    let bounds = GlyphBounds {
        x_min: read_f32(&bytes, &mut cursor),
        y_min: read_f32(&bytes, &mut cursor),
        x_max: read_f32(&bytes, &mut cursor),
        y_max: read_f32(&bytes, &mut cursor),
    };
    let curve_count = read_u32(&bytes, &mut cursor) as usize;
    let mut curves = Vec::with_capacity(curve_count);
    for _ in 0..curve_count {
        let p0 = Vec2::new(read_f32(&bytes, &mut cursor), read_f32(&bytes, &mut cursor));
        let p1 = Vec2::new(read_f32(&bytes, &mut cursor), read_f32(&bytes, &mut cursor));
        let p2 = Vec2::new(read_f32(&bytes, &mut cursor), read_f32(&bytes, &mut cursor));
        curves.push(SlugCurve { p0, p1, p2 });
    }
    let (horizontal_bands, horizontal_curve_indices) = read_band_table(&bytes, &mut cursor);
    let (vertical_bands, vertical_curve_indices) = read_band_table(&bytes, &mut cursor);
    assert_eq!(cursor, bytes.len(), "trailing bytes in {}", path.display());
    SlugGlyph {
        revision,
        bounds,
        curves,
        horizontal_bands,
        horizontal_curve_indices,
        vertical_bands,
        vertical_curve_indices,
    }
}

fn read_known_good_png(name: &str, dpi_label: &str) -> (u32, u32, Vec<u8>) {
    let path = harness_dir()
        .join("goldens/known-good")
        .join(format!("{name}_{dpi_label}.png"));
    let file = fs::File::open(&path).unwrap_or_else(|e| panic!("opening {}: {e}", path.display()));
    let decoder = png::Decoder::new(file);
    let mut reader = decoder.read_info().expect("reading PNG header");
    let mut buffer = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buffer).expect("decoding PNG frame");
    buffer.truncate(info.buffer_size());
    (info.width, info.height, buffer)
}

/// Render `glyph` at `dpi` with mkui's `SlugAdapter`, using the exact camera
/// the reference adapter uses (reference-harness/src/main.rs `render_async`):
/// a `round(128*dpi)` square canvas, `em_pixels = 96*dpi` as the placement
/// scale, and the glyph's font-unit origin landing at
/// `(padding, canvas - padding)` with `padding = 16*dpi`. Matching this
/// exactly is what makes a byte-level comparison against the reference PNGs
/// meaningful.
fn render_mkui(glyph: &SlugGlyph, dpi: f32) -> (u32, Vec<u8>) {
    let canvas = (BASE_CANVAS_PIXELS * dpi).round() as u32;
    let em_pixels = BASE_EM_PIXELS * dpi;
    let padding = BASE_PADDING_PIXELS * dpi;

    let renderer = OffscreenRenderer::new(canvas, canvas)
        .expect("offscreen adapter/device must be available on the CI Vulkan/Lavapipe runner");
    let adapter = SlugAdapter::new(renderer.device(), renderer.format());
    let placed = PlacedSlugGlyph {
        blob: Arc::new(glyph.clone()),
        origin_px: [padding, canvas as f32 - padding],
        scale_px_per_unit: em_pixels,
        color: [1.0, 1.0, 1.0, 1.0],
    };
    let prepared = adapter.prepare(
        renderer.device(),
        renderer.queue(),
        [canvas as f32, canvas as f32],
        1.0,
        &[placed],
    );
    let mut encoder = renderer
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("slug reference parity encoder"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("slug reference parity pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: renderer.view(),
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(prepared) = prepared.as_ref() {
            adapter.draw(&mut pass, prepared);
        }
    }
    renderer.queue().submit(Some(encoder.finish()));
    let pixels = renderer.read_rgba().expect("readback must succeed");
    (canvas, pixels)
}

/// Rubric-shaped diff: max per-channel delta across every texel, plus the
/// count of texels that differ *at all* (any channel off by at least one
/// byte). These are the rubric's two independent (N) criteria — a pixel with
/// delta 1 still counts toward the differing-pixel budget even though it
/// never approaches the separate `MAX_CHANNEL_DELTA` magnitude bound; folding
/// the two together would make the pixel-count criterion vacuous (whenever
/// the magnitude check passes, a `delta > MAX_CHANNEL_DELTA` count is
/// necessarily zero).
fn diff(a: &[u8], b: &[u8]) -> (u8, usize) {
    let mut max_delta = 0u8;
    let mut differing = 0usize;
    for (pa, pb) in a
        .chunks_exact(BYTES_PER_PIXEL as usize)
        .zip(b.chunks_exact(BYTES_PER_PIXEL as usize))
    {
        let delta = pa
            .iter()
            .zip(pb)
            .map(|(x, y)| x.abs_diff(*y))
            .max()
            .unwrap_or(0);
        max_delta = max_delta.max(delta);
        if delta > 0 {
            differing += 1;
        }
    }
    (max_delta, differing)
}

/// Windowed SSIM (8×8 non-overlapping blocks; every rubric canvas size —
/// 128/192/256 — divides evenly, so no partial-block handling is needed) on
/// the R channel. Both the reference adapter and mkui emit premultiplied
/// white-on-transparent coverage for these fixtures (`color = [1,1,1,1]`), so
/// `R == G == B == A` and the R channel alone carries the full signal.
fn ssim_r(a: &[u8], b: &[u8], width: u32, height: u32) -> f64 {
    const WINDOW: usize = 8;
    const K1: f64 = 0.01;
    const K2: f64 = 0.03;
    const DYNAMIC_RANGE: f64 = 255.0;
    let c1 = (K1 * DYNAMIC_RANGE).powi(2);
    let c2 = (K2 * DYNAMIC_RANGE).powi(2);

    let width = width as usize;
    let height = height as usize;
    let ra: Vec<f64> = a
        .chunks_exact(BYTES_PER_PIXEL as usize)
        .map(|p| p[0] as f64)
        .collect();
    let rb: Vec<f64> = b
        .chunks_exact(BYTES_PER_PIXEL as usize)
        .map(|p| p[0] as f64)
        .collect();

    let mut total = 0.0;
    let mut windows = 0.0;
    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            let win_h = WINDOW.min(height - y);
            let win_w = WINDOW.min(width - x);
            let n = (win_h * win_w) as f64;

            let mut sum_a = 0.0;
            let mut sum_b = 0.0;
            for dy in 0..win_h {
                for dx in 0..win_w {
                    let idx = (y + dy) * width + (x + dx);
                    sum_a += ra[idx];
                    sum_b += rb[idx];
                }
            }
            let mean_a = sum_a / n;
            let mean_b = sum_b / n;

            let mut var_a = 0.0;
            let mut var_b = 0.0;
            let mut covar = 0.0;
            for dy in 0..win_h {
                for dx in 0..win_w {
                    let idx = (y + dy) * width + (x + dx);
                    let da = ra[idx] - mean_a;
                    let db = rb[idx] - mean_b;
                    var_a += da * da;
                    var_b += db * db;
                    covar += da * db;
                }
            }
            var_a /= n;
            var_b /= n;
            covar /= n;

            let ssim = ((2.0 * mean_a * mean_b + c1) * (2.0 * covar + c2))
                / ((mean_a * mean_a + mean_b * mean_b + c1) * (var_a + var_b + c2));
            total += ssim;
            windows += 1.0;
            x += WINDOW;
        }
        y += WINDOW;
    }
    total / windows
}

#[test]
fn phase1_matches_reference_adapter_within_rubric_thresholds() {
    let mut failures = Vec::new();
    for name in GLYPH_NAMES {
        let glyph = read_glyph(&harness_dir().join("glyphs").join(format!("{name}.slug")));
        for (dpi, label) in DPI_CASES {
            let (canvas, rendered) = render_mkui(&glyph, dpi);
            let (golden_w, golden_h, golden) = read_known_good_png(name, label);
            assert_eq!(
                (canvas, canvas),
                (golden_w, golden_h),
                "{name}_{label}: canvas size must match the reference adapter's"
            );
            let (max_delta, differing) = diff(&rendered, &golden);
            let ssim = ssim_r(&rendered, &golden, canvas, canvas);
            eprintln!(
                "{name}_{label}: max_channel_delta={max_delta} differing_pixels={differing} ssim={ssim:.6}"
            );
            if max_delta > MAX_CHANNEL_DELTA || differing > MAX_DIFFERING_PIXELS || ssim < MIN_SSIM
            {
                failures.push(format!(
                    "{name}_{label}: max_channel_delta={max_delta} (limit {MAX_CHANNEL_DELTA}), \
                     differing_pixels={differing} (limit {MAX_DIFFERING_PIXELS}), \
                     ssim={ssim:.6} (floor {MIN_SSIM})"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Phase 1 rubric comparisons failed:\n{}",
        failures.join("\n")
    );
}

/// A zero-alpha texel bordered on either axis by texels at ≥50% alpha — the
/// literal thin-gap signature specified by dame-rubric.md § Phase 2 "No
/// thin-gap regressions": "Pixel-scan for zero-alpha pixels bordered by ≥50%
/// alpha on both sides (thin-gap signature). Zero such pixels required."
///
/// This threshold is intentionally the rubric's literal value, not a tuned
/// one. An earlier revision raised it to 250 to dodge a known false positive
/// at the ratified `g` fixture (2x DPI, texel (157,156): neighbours 206/248,
/// ordinary AA, not a defect) — Codex round 3 correctly rejected that as
/// redefining a frozen criterion rather than satisfying it. The ratified
/// reference adapter's own output trips this literal heuristic at that exact
/// texel (Δ=0 vs. mkui, per the Phase 1 parity comparison); dame-rubric.md
/// v1.2.1's byte-identical exception (see `texel_delta` below) resolves that
/// tension without loosening this constant.
const HALF_ALPHA: u8 = 128;

fn thin_gap_coordinates(pixels: &[u8], width: u32, height: u32) -> Vec<(u32, u32)> {
    let width = width as usize;
    let height = height as usize;
    let alpha = |x: usize, y: usize| pixels[(y * width + x) * BYTES_PER_PIXEL as usize + 3];
    let mut hits = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if alpha(x, y) != 0 {
                continue;
            }
            let horiz_gap = x > 0
                && x + 1 < width
                && alpha(x - 1, y) >= HALF_ALPHA
                && alpha(x + 1, y) >= HALF_ALPHA;
            let vert_gap = y > 0
                && y + 1 < height
                && alpha(x, y - 1) >= HALF_ALPHA
                && alpha(x, y + 1) >= HALF_ALPHA;
            if horiz_gap || vert_gap {
                hits.push((x as u32, y as u32));
            }
        }
    }
    hits
}

/// Per-channel delta between `rendered` and `golden` at a single texel — the
/// coordinate-scoped counterpart to [`diff`]'s whole-image scan, used to
/// decide whether a thin-gap hit is a real regression (dame-rubric.md
/// v1.2.1 § Phase 2 "No thin-gap regressions" byte-identical exception).
fn texel_delta(rendered: &[u8], golden: &[u8], width: u32, x: u32, y: u32) -> u8 {
    let idx = (y as usize * width as usize + x as usize) * BYTES_PER_PIXEL as usize;
    rendered[idx..idx + BYTES_PER_PIXEL as usize]
        .iter()
        .zip(&golden[idx..idx + BYTES_PER_PIXEL as usize])
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

#[test]
fn phase2_no_thin_gap_regressions_on_curve_heavy_glyphs() {
    // dame-rubric.md § Phase 2: `o` and `g` are the curve-heavy, band-epsilon-
    // sensitive fixtures — their bowls cross many band boundaries at
    // near-tangent angles, exactly where a floating-point gap would open a
    // one-pixel hole through solid ink. The rubric requires zero *regression*
    // texels at the literal `HALF_ALPHA` threshold.
    //
    // v1.2.1 amendment (ratified 07926f0): a thin-gap hit that is
    // byte-identical to the ratified reference-harness golden at the same
    // coordinate is not a regression — the reference oracle itself produces
    // this pattern at tight curve intersections (e.g. `g_2x` texel
    // (157,156), Δ=0 vs mkui). A hit only counts if it also diverges from
    // the reference (Δ > 0) at that coordinate. See dame-rubric.md v1.2.1
    // (ratified `07926f0`) for the amendment; the original oracle-ambiguity
    // finding this resolves was added at sha `0411739`.
    let mut failures = Vec::new();
    for name in ["o", "g"] {
        let glyph = read_glyph(&harness_dir().join("glyphs").join(format!("{name}.slug")));
        for (dpi, label) in DPI_CASES {
            let (canvas, rendered) = render_mkui(&glyph, dpi);
            let (golden_w, golden_h, golden) = read_known_good_png(name, label);
            assert_eq!(
                (canvas, canvas),
                (golden_w, golden_h),
                "{name}_{label}: canvas size must match the reference adapter's"
            );
            let gaps = thin_gap_coordinates(&rendered, canvas, canvas);
            let regressions: Vec<(u32, u32)> = gaps
                .into_iter()
                .filter(|&(x, y)| texel_delta(&rendered, &golden, canvas, x, y) > 0)
                .collect();
            if !regressions.is_empty() {
                failures.push(format!(
                    "{name}_{label}: thin-gap texels at {regressions:?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "Phase 2 thin-gap regressions found:\n{}",
        failures.join("\n")
    );
}
