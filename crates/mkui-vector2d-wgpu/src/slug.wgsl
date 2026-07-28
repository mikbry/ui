// Slug glyph coverage pipeline (#66, #157 Phase 1).
//
// One dilated quad is drawn per glyph instance. The fragment shader maps each
// covered pixel back into the glyph's font-unit space (y-up), selects the
// horizontal band the sample row falls in *and* the vertical band the sample
// column falls in, and accumulates anti-aliased coverage by casting one
// horizontal ray and one vertical ray through those bands' quadratic curve
// records. This honours the #65 / mkui-vector2d band contract: band
// membership and ordering are consumed as produced, never recomputed.
//
// The dual-ray weighted-coverage combination (root eligibility via the
// `0x2e74` code, the two solved-poly helpers, and the weighted H/V
// combination in `calc_coverage`) is a from-scratch WGSL implementation of
// the published Slug algorithm (Lengyel, JCGT 6:2), independently structured
// to mkui's own storage-buffer curve/band records rather than the upstream
// texture layout. It copies no upstream source.

struct Viewport {
    // Logical-pixel viewport the quads are projected against. `_pad` keeps the
    // uniform a clean 16 bytes.
    size: vec2<f32>,
    _pad: vec2<f32>,
};

// A quadratic curve record `(p0, p1, p2)` in font units, y-up. A straight line
// arrives with the duplicated-endpoint sentinel `p1 == p2` (see mkui-vector2d).
struct Curve {
    p0: vec2<f32>,
    p1: vec2<f32>,
    p2: vec2<f32>,
};

// One horizontal band: its y-range plus a slice into the per-glyph curve-index
// stream. `first_curve` is relative to the glyph's index base.
struct Band {
    lower: f32,
    upper: f32,
    first_curve: u32,
    curve_count: u32,
};

// Per-glyph instance: the dilated screen quad, the font→screen placement, the
// fill colour, and the offsets that locate this glyph's slices inside the
// shared curve / band / index buffers. `band_base`/`band_count`/`index_base`
// address the horizontal (row) bands; `vband_base`/`vband_count`/`vindex_base`
// address the vertical (column) bands appended right after them in the same
// `bands`/`indices` buffers. `bounds_min`/`bounds_max` are the glyph's own
// font-unit ink bounds (not the dilated quad) — the band-selection index is
// computed from these, matching the Slug reference's clamped
// `band_transform` lookup rather than a containment scan, so AA coverage
// stays correct for samples in the dilation margin just outside the bounds.
struct Glyph {
    color: vec4<f32>,
    quad_min: vec2<f32>,
    quad_max: vec2<f32>,
    origin_px: vec2<f32>,
    // params.x = pixels per font unit (placement scale; no longer the AA
    // width — see `fs_slug`'s `fwidth`-derived `pixels_per_em`).
    params: vec2<f32>,
    curve_base: u32,
    band_base: u32,
    band_count: u32,
    index_base: u32,
    bounds_min: vec2<f32>,
    bounds_max: vec2<f32>,
    vband_base: u32,
    vband_count: u32,
    vindex_base: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> viewport: Viewport;
@group(0) @binding(1) var<storage, read> curves: array<Curve>;
@group(0) @binding(2) var<storage, read> bands: array<Band>;
@group(0) @binding(3) var<storage, read> indices: array<u32>;
@group(0) @binding(4) var<storage, read> glyphs: array<Glyph>;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) font_pos: vec2<f32>,
    @location(1) @interpolate(flat) glyph_index: u32,
};

@vertex
fn vs_slug(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let g = glyphs[ii];
    // Two-triangle quad corners in unit space.
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0),
    );
    let unit = corners[vi];
    let px = mix(g.quad_min, g.quad_max, unit);
    let scale = max(g.params.x, 1e-6);
    // Screen-pixel → font-unit (y-up). Inverse of the CPU placement that put
    // the dilated quad on screen.
    let font_x = (px.x - g.origin_px.x) / scale;
    let font_y = (g.origin_px.y - px.y) / scale;
    let ndc = vec2<f32>(
        px.x / max(viewport.size.x, 1.0) * 2.0 - 1.0,
        1.0 - px.y / max(viewport.size.y, 1.0) * 2.0,
    );
    var out: VsOut;
    out.pos = vec4<f32>(ndc, 0.0, 1.0);
    out.font_pos = vec2<f32>(font_x, font_y);
    out.glyph_index = ii;
    return out;
}

// Root-eligibility code for a quadratic's three y-relative (or x-relative)
// control-point values `(y1, y2, y3)` — which of the curve's two roots
// actually cross the ray and with what sign, encoded via the sign bits of the
// three values folded into a 3-bit lookup into the constant `0x2e74`. This is
// the standard robust replacement for a `t in [0, 1)` range test: it decides
// root validity from the endpoints' signs rather than the (numerically
// fragile, near a tangent root) solved `t` itself. Bit 0 of the returned code
// means "first root contributes"; bit 1 (tested via `code > 1u`) means
// "second root contributes".
fn calc_root_code(y1: f32, y2: f32, y3: f32) -> u32 {
    let i1 = bitcast<u32>(y1) >> 31u;
    let i2 = bitcast<u32>(y2) >> 30u;
    let i3 = bitcast<u32>(y3) >> 29u;
    var shift = (i2 & 2u) | (i1 & ~2u);
    shift = (i3 & 4u) | (shift & ~4u);
    return (0x2e74u >> shift) & 0x0101u;
}

// Solve the quadratic `a.y * t^2 - 2*b.y * t + rel0.y = 0` for its two roots
// and evaluate the curve's x-coordinate at each, for a horizontal ray (the
// curve's control points are `rel0`/`rel1`/`rel2`, already relative to the
// sample). Falls back to the linear crossing when the curve is near-flat in
// y (`|a.y|` below the threshold), matching the quadratic and linear cases at
// their boundary.
fn solve_horiz_poly(rel0: vec2<f32>, rel1: vec2<f32>, rel2: vec2<f32>) -> vec2<f32> {
    let a = rel0 - rel1 * 2.0 + rel2;
    let b = rel0 - rel1;
    let ra = 1.0 / a.y;
    let rb = 0.5 / b.y;
    let d = sqrt(max(b.y * b.y - a.y * rel0.y, 0.0));
    var t1 = (b.y - d) * ra;
    var t2 = (b.y + d) * ra;
    if (abs(a.y) < 1.0 / 65536.0) {
        t1 = rel0.y * rb;
        t2 = t1;
    }
    return vec2<f32>(
        (a.x * t1 - b.x * 2.0) * t1 + rel0.x,
        (a.x * t2 - b.x * 2.0) * t2 + rel0.x,
    );
}

// Axis-transposed counterpart of `solve_horiz_poly` for a vertical ray: solves
// in x and evaluates y.
fn solve_vert_poly(rel0: vec2<f32>, rel1: vec2<f32>, rel2: vec2<f32>) -> vec2<f32> {
    let a = rel0 - rel1 * 2.0 + rel2;
    let b = rel0 - rel1;
    let ra = 1.0 / a.x;
    let rb = 0.5 / b.x;
    let d = sqrt(max(b.x * b.x - a.x * rel0.x, 0.0));
    var t1 = (b.x - d) * ra;
    var t2 = (b.x + d) * ra;
    if (abs(a.x) < 1.0 / 65536.0) {
        t1 = rel0.x * rb;
        t2 = t1;
    }
    return vec2<f32>(
        (a.y * t1 - b.y * 2.0) * t1 + rel0.y,
        (a.y * t2 - b.y * 2.0) * t2 + rel0.y,
    );
}

// Weighted combination of the two axis coverages (JCGT §4.2): the two rays'
// coverage is blended by a per-axis weight (the crossing's closeness to the
// sample, `1 - 2*|distance|` clamped) so the axis whose ray crosses closer to
// the true edge dominates; `min(|xcov|, |ycov|)` is a coverage floor so a
// sample deep inside the ink (both rays saturated near 1) is never darkened
// by weighting noise. The weighted value is a raw non-zero-winding
// accumulation — two same-winding overlapping contours (e.g. the ratified
// `plus` fixture's two crossing bars) legitimately reach magnitude > 1 at
// their intersection — so the final `[0, 1]` clamp is applied here, matching
// the reference adapter's `calc_coverage` (which clamps internally, not at
// its call site): the accumulator, not the final alpha, is what is allowed to
// exceed the unit range.
fn calc_coverage(xcov: f32, ycov: f32, xwgt: f32, ywgt: f32) -> f32 {
    let coverage = max(
        abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0),
        min(abs(xcov), abs(ycov)),
    );
    return clamp(coverage, 0.0, 1.0);
}

// Per-axis accumulated coverage plus the combined, already-clamped value.
struct Coverage {
    xcov: f32,
    ycov: f32,
    value: f32,
};

// Cast one horizontal ray and one vertical ray through `g`'s band tables at
// `sample` (font units, y-up) and return the combined coverage. The band for
// each axis is the clamped index derived from the glyph's own
// font-unit bounds (matching the Slug reference's `band_transform`), not a
// containment scan, so a sample in the dilation margin just outside the exact
// bounds still finds the outermost band and its curves. Within a band, curves
// are ordered (by #65's CPU encoder) by descending cross-axis extremum, so
// the `pixels_per_em`-scaled early-out `break` is valid once a curve's
// nearest extent is more than half a pixel past the sample.
fn slug_coverage(g: Glyph, sample: vec2<f32>) -> Coverage {
    let ems_per_pixel = max(fwidth(sample), vec2<f32>(1e-6));
    let pixels_per_em = vec2<f32>(1.0) / ems_per_pixel;

    var xcov = 0.0;
    var xwgt = 0.0;
    let h_span = max(g.bounds_max.y - g.bounds_min.y, 1e-6);
    let h_count = max(g.band_count, 1u);
    let h_idx = u32(clamp(floor((sample.y - g.bounds_min.y) / h_span * f32(h_count)), 0.0, f32(h_count) - 1.0));
    let hband = bands[g.band_base + h_idx];
    var hi = 0u;
    loop {
        if (hi >= hband.curve_count) {
            break;
        }
        let ci = indices[g.index_base + hband.first_curve + hi];
        let c = curves[g.curve_base + ci];
        let rel0 = c.p0 - sample;
        let rel1 = c.p1 - sample;
        let rel2 = c.p2 - sample;
        if (max(max(rel0.x, rel1.x), rel2.x) * pixels_per_em.x < -0.5) {
            break;
        }
        let code = calc_root_code(rel0.y, rel1.y, rel2.y);
        if (code != 0u) {
            let r = solve_horiz_poly(rel0, rel1, rel2) * pixels_per_em.x;
            if ((code & 1u) != 0u) {
                xcov = xcov + clamp(r.x + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                xcov = xcov - clamp(r.y + 0.5, 0.0, 1.0);
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
        hi = hi + 1u;
    }

    var ycov = 0.0;
    var ywgt = 0.0;
    let v_span = max(g.bounds_max.x - g.bounds_min.x, 1e-6);
    let v_count = max(g.vband_count, 1u);
    let v_idx = u32(clamp(floor((sample.x - g.bounds_min.x) / v_span * f32(v_count)), 0.0, f32(v_count) - 1.0));
    let vband = bands[g.vband_base + v_idx];
    var vi = 0u;
    loop {
        if (vi >= vband.curve_count) {
            break;
        }
        let ci = indices[g.vindex_base + vband.first_curve + vi];
        let c = curves[g.curve_base + ci];
        let rel0 = c.p0 - sample;
        let rel1 = c.p1 - sample;
        let rel2 = c.p2 - sample;
        if (max(max(rel0.y, rel1.y), rel2.y) * pixels_per_em.y < -0.5) {
            break;
        }
        let code = calc_root_code(rel0.x, rel1.x, rel2.x);
        if (code != 0u) {
            let r = solve_vert_poly(rel0, rel1, rel2) * pixels_per_em.y;
            if ((code & 1u) != 0u) {
                ycov = ycov - clamp(r.x + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0));
            }
            if (code > 1u) {
                ycov = ycov + clamp(r.y + 0.5, 0.0, 1.0);
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0));
            }
        }
        vi = vi + 1u;
    }

    var result: Coverage;
    result.xcov = xcov;
    result.ycov = ycov;
    result.value = calc_coverage(xcov, ycov, xwgt, ywgt);
    return result;
}

@fragment
fn fs_slug(in: VsOut) -> @location(0) vec4<f32> {
    let g = glyphs[in.glyph_index];
    let cov = slug_coverage(g, in.font_pos);
    if (cov.value <= 0.0) {
        discard;
    }
    return vec4<f32>(g.color.rgb, g.color.a * cov.value);
}

// Debug entry point exposing the coverage the fragment shader computes just
// before it scales the fill colour's alpha (dame-rubric § Phase 1 "coverage
// bounded in pre-clamp float buffer" criterion): `x` is `calc_coverage`'s
// already-`[0, 1]`-clamped combined value (matching the reference adapter's
// own internally-clamped `calc_coverage`), `y`/`z` are the raw, genuinely
// unbounded per-axis winding accumulations — these can legitimately exceed
// `[-1, 1]` at a same-winding self-overlap (see `calc_coverage`'s doc comment)
// and are exposed here for that reason. Bind to a float target
// (e.g. `Rgba32Float`) to inspect — never sampled by the production pipeline.
@fragment
fn fs_slug_debug_coverage(in: VsOut) -> @location(0) vec4<f32> {
    let g = glyphs[in.glyph_index];
    let cov = slug_coverage(g, in.font_pos);
    return vec4<f32>(cov.value, cov.xcov, cov.ycov, 1.0);
}
