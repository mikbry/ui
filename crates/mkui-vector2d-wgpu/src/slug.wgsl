// Slug glyph coverage pipeline (#66).
//
// One dilated quad is drawn per glyph instance. The fragment shader maps each
// covered pixel back into the glyph's font-unit space (y-up), selects the
// horizontal band the sample row falls in, and accumulates anti-aliased
// coverage by casting a single horizontal ray through that band's quadratic
// curve records. This honours the #65 / mkui-vector2d band contract: band
// membership and ordering are consumed as produced, never recomputed.
//
// The coverage kernel reproduces the analytic single-ray accumulation used by
// public-domain GPU vector-text renderers (the Slug contract by Eric Lengyel,
// and Will Dobbie's vector-texture note): for each curve the two y-crossings of
// the ray contribute signed, pixel-footprint-clamped coverage. It copies no
// upstream source.

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
// shared curve / band / index buffers.
struct Glyph {
    color: vec4<f32>,
    quad_min: vec2<f32>,
    quad_max: vec2<f32>,
    origin_px: vec2<f32>,
    // params.x = pixels per font unit (placement scale + AA inverse-diameter).
    params: vec2<f32>,
    curve_base: u32,
    band_base: u32,
    band_count: u32,
    index_base: u32,
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

// Signed coverage contribution of one curve for a horizontal ray at `sample`,
// cast in +x. `inv_diam` is pixels-per-font-unit so the crossing position is
// clamped to a one-pixel anti-aliasing footprint.
fn eval_coverage(sample: vec2<f32>, inv_diam: f32, c: Curve) -> f32 {
    let p0 = c.p0 - sample;
    let p1 = c.p1 - sample;
    let p2 = c.p2 - sample;
    // Convex-hull early out: if every control point is on one side of the ray
    // the curve cannot cross it.
    if (p0.y > 0.0 && p1.y > 0.0 && p2.y > 0.0) {
        return 0.0;
    }
    if (p0.y < 0.0 && p1.y < 0.0 && p2.y < 0.0) {
        return 0.0;
    }
    let a = p0 - 2.0 * p1 + p2;
    let b = p0 - p1;
    let cc = p0;
    var t0 = -1.0;
    var t1 = -1.0;
    if (abs(a.y) >= 1e-5) {
        let rad = b.y * b.y - a.y * cc.y;
        if (rad < 0.0) {
            return 0.0;
        }
        let s = sqrt(rad);
        t0 = (b.y - s) / a.y;
        t1 = (b.y + s) / a.y;
    } else {
        // Near-linear in y: a single crossing. Order the slots by travel
        // direction so the winding sign stays consistent with the quadratic
        // branch (entering root positive, exiting root negative).
        let denom = p0.y - p2.y;
        if (abs(denom) < 1e-9) {
            return 0.0;
        }
        let t = p0.y / denom;
        if (p0.y < p2.y) {
            t1 = t;
        } else {
            t0 = t;
        }
    }
    var alpha = 0.0;
    if (t0 >= 0.0 && t0 < 1.0) {
        let x = (a.x * t0 - 2.0 * b.x) * t0 + cc.x;
        alpha = alpha + clamp(x * inv_diam + 0.5, 0.0, 1.0);
    }
    if (t1 >= 0.0 && t1 < 1.0) {
        let x = (a.x * t1 - 2.0 * b.x) * t1 + cc.x;
        alpha = alpha - clamp(x * inv_diam + 0.5, 0.0, 1.0);
    }
    return alpha;
}

@fragment
fn fs_slug(in: VsOut) -> @location(0) vec4<f32> {
    let g = glyphs[in.glyph_index];
    let sample = in.font_pos;
    let inv_diam = max(g.params.x, 1e-6);

    var coverage = 0.0;
    // Linear scan for the band whose y-range contains the sample row. Band
    // counts are small and the scan avoids any edge-case rounding in a
    // floor-division index.
    var bi = 0u;
    loop {
        if (bi >= g.band_count) {
            break;
        }
        let band = bands[g.band_base + bi];
        if (sample.y >= band.lower && sample.y <= band.upper) {
            var k = 0u;
            loop {
                if (k >= band.curve_count) {
                    break;
                }
                let ci = indices[g.index_base + band.first_curve + k];
                let curve = curves[g.curve_base + ci];
                coverage = coverage + eval_coverage(sample, inv_diam, curve);
                k = k + 1u;
            }
            break;
        }
        bi = bi + 1u;
    }

    let a = clamp(abs(coverage), 0.0, 1.0);
    if (a <= 0.0) {
        discard;
    }
    return vec4<f32>(g.color.rgb, g.color.a * a);
}
