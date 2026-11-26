//! Elliptical-arc (`A`/`a` path command) → cubic Bézier conversion (#201).
//!
//! `mkui_vector2d::PathCommand` has no arc primitive — only line/quad/cubic —
//! so an SVG arc must be lowered to one or more [`mkui_vector2d::PathCommand::CubicTo`]
//! segments before it can be represented at all. This is the standard
//! endpoint-to-center parameterization from the SVG 1.1 spec, Appendix F.6,
//! hand-written against that spec text (not vendored from `svgtypes`/`usvg`,
//! per the issue's "hand-write value parsing" requirement — an arc is a
//! value within the `d` grammar, not XML tokenization).

use mkui_vector2d::Vec2;

/// One cubic segment: `(control1, control2, end)`. The start point is
/// whatever the caller's pen position already is.
pub type CubicSegment = (Vec2, Vec2, Vec2);

/// The largest `max(rx, ry) / chord` this module will attempt to compute
/// rather than reject outright.
///
/// Neither `f32` nor `f64` is the real constraint here — *no* amount of
/// precision fixes an ill-conditioned problem, because the conditioning is
/// about the *ratio* between two scales, not either one's absolute
/// magnitude (a radius of `2_000_000` over a chord of comparable length is
/// just as well-behaved as a radius of `20` over a chord of `10` — floating
/// point is relative, not absolute). What actually breaks the
/// endpoint-to-center construction below is a radius *disproportionately*
/// larger than the chord it spans: resolving the resulting center offset
/// needs roughly `(chord / radius)² / 8` (the sagitta, relative to the
/// radius) worth of relative precision, and `f64` only has ~2.22e-16 of
/// that to spend. At this threshold the sagitta is still ~564× larger than
/// that epsilon — comfortable margin — while well-conditioned arcs at any
/// absolute scale (this subset's `d`-grammar numbers are plain `f32`s, so
/// nothing here is bounded by the *document's* own coordinate magnitude)
/// stay far below it. An earlier revision tried clamping the radius itself
/// to an absolute cap instead: that neither caught an extreme ratio
/// produced by a tiny chord at a merely large-but-capped radius, nor left
/// alone a huge-but-well-conditioned radius/chord pair — this ratio is the
/// actual conditioning measure, an absolute magnitude never was.
const MAX_RADIUS_TO_CHORD_RATIO: f64 = 1e6;

/// Convert an SVG elliptical arc from `start` to `end` into a chain of cubic
/// Bézier segments, all in the same (SVG, y-down) coordinate space as
/// `start`/`end`. `x_axis_rotation_deg` is the ellipse's own rotation.
///
/// `None` for two distinct reasons: `start == end` (spec: omit the arc) is
/// the caller's special case, never reached here — and a radius so
/// disproportionate to the chord it spans that computing it would be a
/// best-effort guess at numerical noise rather than a representable curve
/// (see `MAX_RADIUS_TO_CHORD_RATIO`) is this function's own typed
/// rejection, matching this parser's "anything outside what can be safely
/// represented is rejected, not guessed at" rule everywhere else (DOCTYPE,
/// foreign namespaces, non-finite numbers, ...).
pub fn arc_to_cubics(
    start: Vec2,
    rx: f32,
    ry: f32,
    x_axis_rotation_deg: f32,
    large_arc: bool,
    sweep: bool,
    end: Vec2,
) -> Option<Vec<CubicSegment>> {
    // Spec F.6.2: a zero radius is equivalent to a straight line. A cubic
    // with both control points pinned to the endpoints traces exactly that
    // line (a non-uniform but still straight parametrization).
    if rx == 0.0 || ry == 0.0 {
        return Some(vec![(start, end, end)]);
    }

    // Everything below runs in `f64`, not `f32`: even a well-conditioned
    // ratio (see `MAX_RADIUS_TO_CHORD_RATIO`) needs more than `f32`'s ~7
    // decimal digits to resolve the center/angle math precisely once the
    // radius and chord are at noticeably different scales. Only the final
    // control points/endpoint are narrowed back to `f32`, at the public
    // API boundary (`Vec2` is `f32`).
    let rx = (rx as f64).abs();
    let ry = (ry as f64).abs();
    let phi = (x_axis_rotation_deg as f64).to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();
    let (x1, y1) = (start.x as f64, start.y as f64);
    let (x2, y2) = (end.x as f64, end.y as f64);

    // F.6.5 step 1: (x1', y1') = R(-phi) * (start - end) / 2
    let dx2 = (x1 - x2) / 2.0;
    let dy2 = (y1 - y2) / 2.0;
    let x1p = cos_phi * dx2 + sin_phi * dy2;
    let y1p = -sin_phi * dx2 + cos_phi * dy2;

    // step 2: radii correction (F.6.6)
    let lambda = (x1p * x1p) / (rx * rx) + (y1p * y1p) / (ry * ry);
    let (rx, ry) = if lambda > 1.0 {
        let s = lambda.sqrt();
        (rx * s, ry * s)
    } else {
        (rx, ry)
    };

    // Reject rather than compute an ill-conditioned arc — see
    // `MAX_RADIUS_TO_CHORD_RATIO`'s docs. Using the (possibly step-2-
    // enlarged) radius and the direct start→end distance: step 2 only ever
    // *enlarges* a too-small radius to fit the chord, so by this point
    // `max(rx, ry)` is already the effective radius the rest of this
    // function will actually use.
    let chord = (dx2 * dx2 + dy2 * dy2).sqrt() * 2.0;
    if rx.max(ry) / chord > MAX_RADIUS_TO_CHORD_RATIO {
        return None;
    }

    // step 3: center (cx', cy') (F.6.5 step 2)
    let rx2 = rx * rx;
    let ry2 = ry * ry;
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let num = rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2;
    let den = rx2 * y1p2 + ry2 * x1p2;
    let sq = (num / den).max(0.0);
    let coef = if large_arc != sweep {
        sq.sqrt()
    } else {
        -sq.sqrt()
    };
    let cxp = coef * (rx * y1p / ry);
    let cyp = coef * (-ry * x1p / rx);

    // step 4: center in the original coordinate space.
    let cx = cos_phi * cxp - sin_phi * cyp + (x1 + x2) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (y1 + y2) / 2.0;

    // step 5: start/sweep angles (F.6.5 steps 3-4).
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle_between((1.0, 0.0), (ux, uy));
    let mut dtheta = angle_between((ux, uy), (vx, vy));
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f64::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f64::consts::TAU;
    }

    // Split into segments of at most 90 degrees for an accurate cubic fit.
    let segment_count = (dtheta.abs() / (std::f64::consts::FRAC_PI_2))
        .ceil()
        .max(1.0) as usize;
    let seg_sweep = dtheta / segment_count as f64;
    let ellipse = RotatedEllipse {
        cx,
        cy,
        rx,
        ry,
        cos_phi,
        sin_phi,
    };
    let mut segments = Vec::with_capacity(segment_count);
    let mut a0 = theta1;
    for i in 0..segment_count {
        let a1 = a0 + seg_sweep;
        let (c1, c2, computed_end) = cubic_for_arc_segment(&ellipse, a0, a1);
        // The parametric reconstruction of the *very last* segment's
        // on-curve endpoint should equal the authored `end` exactly, up to
        // floating error — but at extreme radius/chord ratios that error
        // is still large enough in `f32` terms to visibly miss. Pin just
        // that endpoint (not the control points, which stay as the now
        // much more accurate `f64` computation) to the caller's own `end`:
        // any downstream consumer (a following `L`, a `Z` back to the
        // subpath's start) depends on it being exact, not merely close.
        let on_curve_end = if i == segment_count - 1 {
            end
        } else {
            computed_end
        };
        segments.push((c1, c2, on_curve_end));
        a0 = a1;
    }
    Some(segments)
}

/// An ellipse centered at `(cx, cy)` with semi-axes `(rx, ry)`, rotated by
/// `(cos_phi, sin_phi)` — bundled so [`cubic_for_arc_segment`] takes one
/// argument instead of six. `f64` throughout; see [`arc_to_cubics`]'s docs
/// for why.
struct RotatedEllipse {
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    cos_phi: f64,
    sin_phi: f64,
}

/// Signed angle (radians) from vector `u` to vector `v`.
///
/// `atan2(cross, dot)`, not `acos(dot / (|u||v|))`: for a shallow arc (a
/// chord tiny relative to its radius) the two vectors are nearly parallel,
/// `dot / (|u||v|)` lands within epsilon of `1.0`, and `acos` near its
/// domain edge amplifies that rounding into a wildly wrong angle. `atan2`
/// has no such singularity at `0`/`±π` and needs neither a normalization
/// `sqrt` nor the resulting division.
fn angle_between(u: (f64, f64), v: (f64, f64)) -> f64 {
    let cross = u.0 * v.1 - u.1 * v.0;
    let dot = u.0 * v.0 + u.1 * v.1;
    cross.atan2(dot)
}

/// Cubic Bézier approximation of one `ellipse` arc segment spanning
/// `[a0, a1]` (radians, `|a1 - a0| <= pi/2`). Narrows to `f32` only in the
/// returned [`Vec2`]s.
fn cubic_for_arc_segment(ellipse: &RotatedEllipse, a0: f64, a1: f64) -> CubicSegment {
    let &RotatedEllipse {
        cx,
        cy,
        rx,
        ry,
        cos_phi,
        sin_phi,
    } = ellipse;
    let point = |a: f64| -> (f64, f64) {
        let (sin_a, cos_a) = a.sin_cos();
        let ex = rx * cos_a;
        let ey = ry * sin_a;
        (
            cos_phi * ex - sin_phi * ey + cx,
            sin_phi * ex + cos_phi * ey + cy,
        )
    };
    let tangent = |a: f64| -> (f64, f64) {
        let (sin_a, cos_a) = a.sin_cos();
        let dx = -rx * sin_a;
        let dy = ry * cos_a;
        (cos_phi * dx - sin_phi * dy, sin_phi * dx + cos_phi * dy)
    };
    let d = a1 - a0;
    let t = 4.0 / 3.0 * (d / 4.0).tan();
    let (p0x, p0y) = point(a0);
    let (p1x, p1y) = point(a1);
    let (t0x, t0y) = tangent(a0);
    let (t1x, t1y) = tangent(a1);
    let c1 = Vec2::new((p0x + t * t0x) as f32, (p0y + t * t0y) as f32);
    let c2 = Vec2::new((p1x - t * t1x) as f32, (p1y - t * t1y) as f32);
    let p1 = Vec2::new(p1x as f32, p1y as f32);
    (c1, c2, p1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semicircle_ends_at_the_right_point() {
        // A 180-degree arc of radius 10 from (-10, 0) to (10, 0), center (0, 0).
        let segs = arc_to_cubics(
            Vec2::new(-10.0, 0.0),
            10.0,
            10.0,
            0.0,
            false,
            true,
            Vec2::new(10.0, 0.0),
        )
        .unwrap();
        assert!(!segs.is_empty());
        let (_, _, end) = *segs.last().unwrap();
        assert!((end.x - 10.0).abs() < 1e-3);
        assert!((end.y - 0.0).abs() < 1e-3);
    }

    #[test]
    fn quarter_circle_passes_near_the_true_arc_midpoint() {
        let segs = arc_to_cubics(
            Vec2::new(10.0, 0.0),
            10.0,
            10.0,
            0.0,
            false,
            true,
            Vec2::new(0.0, 10.0),
        )
        .unwrap();
        assert_eq!(segs.len(), 1);
        // Midpoint of the cubic (De Casteljau at t=0.5) should be close to the
        // true circle point at 45 degrees: (10/sqrt(2), 10/sqrt(2)).
        let (c1, c2, end) = segs[0];
        let start = Vec2::new(10.0, 0.0);
        let mid = cubic_at_half(start, c1, c2, end);
        let expected = 10.0 / std::f32::consts::SQRT_2;
        assert!((mid.x - expected).abs() < 0.2);
        assert!((mid.y - expected).abs() < 0.2);
    }

    fn cubic_at_half(p0: Vec2, p1: Vec2, p2: Vec2, p3: Vec2) -> Vec2 {
        let lerp = |a: Vec2, b: Vec2| Vec2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        let ab = lerp(p0, p1);
        let bc = lerp(p1, p2);
        let cd = lerp(p2, p3);
        let abc = lerp(ab, bc);
        let bcd = lerp(bc, cd);
        lerp(abc, bcd)
    }

    #[test]
    fn large_vs_small_arc_flag_changes_the_path() {
        // Same chord (a 90-degree arc either way round the circle), same
        // sweep direction: `large_arc` alone picks the short (90°, one
        // segment) way round vs. the long (270°, three 90° segments) way.
        let start = Vec2::new(10.0, 0.0);
        let end = Vec2::new(0.0, 10.0);
        let small = arc_to_cubics(start, 10.0, 10.0, 0.0, false, true, end).unwrap();
        let large = arc_to_cubics(start, 10.0, 10.0, 0.0, true, true, end).unwrap();
        assert_eq!(small.len(), 1);
        assert_eq!(large.len(), 3);
        let mid_small = cubic_at_half(start, small[0].0, small[0].1, small[0].2);
        let mid_large = cubic_at_half(start, large[0].0, large[0].1, large[0].2);
        // The short way bulges away from the origin on the same side as the
        // chord; the long way's first segment heads off the opposite side.
        assert!((mid_small.x - mid_large.x).abs() > 5.0);
    }

    #[test]
    fn radii_too_small_are_scaled_up_rather_than_producing_nan() {
        // Chord length 20 but radius 1: geometrically impossible without
        // scaling — the spec's step 2 correction must kick in.
        let segs = arc_to_cubics(
            Vec2::new(-10.0, 0.0),
            1.0,
            1.0,
            0.0,
            false,
            true,
            Vec2::new(10.0, 0.0),
        )
        .unwrap();
        for (c1, c2, end) in segs {
            assert!(c1.x.is_finite() && c1.y.is_finite());
            assert!(c2.x.is_finite() && c2.y.is_finite());
            assert!(end.x.is_finite() && end.y.is_finite());
        }
    }

    #[test]
    fn a_shallow_arc_reaches_its_true_endpoint_instead_of_collapsing() {
        // Regression: the old `acos(dot / (|u||v|))` angle formula loses
        // precision catastrophically when the two vectors it compares are
        // nearly parallel — exactly the case for a shallow arc (chord tiny
        // relative to radius). A radius-100 arc spanning an endpoint just
        // 0.01 away used to collapse to a near-zero sweep (observed:
        // landing around (7.6e-7, 0) instead of (0.01, 0)). Ratio here is
        // only 1e4 — well inside `MAX_RADIUS_TO_CHORD_RATIO` — so this is
        // still an *accepted*, computed arc, not a rejection.
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(0.01, 0.0);
        let segs = arc_to_cubics(start, 100.0, 100.0, 0.0, false, true, end).unwrap();
        let (_, _, reached_end) = *segs.last().unwrap();
        assert!(
            (reached_end.x - end.x).abs() < 1e-4,
            "expected to reach x ≈ {}, got {}",
            end.x,
            reached_end.x
        );
        assert!((reached_end.y - end.y).abs() < 1e-4);
        // The arc must have actually swept *something* nonzero — not
        // collapsed to an exact straight line (a radius-100 arc over a
        // 0.01 chord has a genuinely tiny sagitta, ~chord²/(8·r) ≈ 1.25e-7,
        // so this only checks "not exactly zero", not a specific magnitude).
        let (c1, _, _) = segs[0];
        let midpoint_deviation = (c1.y - start.y).abs();
        assert!(
            midpoint_deviation > 0.0 && midpoint_deviation.is_finite(),
            "control point collapsed onto the chord: {c1:?}"
        );
    }

    #[test]
    fn a_moderately_large_radius_arc_reaches_its_exact_endpoint() {
        // `M0,0 A10000 10000 0 0 1 10 0` — ratio 1e3, comfortably inside
        // `MAX_RADIUS_TO_CHORD_RATIO`. `f64` intermediates plus pinning the
        // final on-curve point to the authored `end` land exactly on it.
        let start = Vec2::new(0.0, 0.0);
        let end = Vec2::new(10.0, 0.0);
        let segs = arc_to_cubics(start, 1e4, 1e4, 0.0, false, true, end).unwrap();
        let (_, _, reached_end) = *segs.last().unwrap();
        assert_eq!(
            reached_end, end,
            "must land exactly on the authored endpoint"
        );
    }

    #[test]
    fn well_conditioned_arcs_are_accurate_at_any_absolute_scale() {
        // Review round 4, case 2: `M2000000,0 A2000000 2000000 0 0 1
        // 0,2000000` is geometrically identical (just scaled ×100000) to
        // the `quarter_circle_passes_near_the_true_arc_midpoint` case
        // above — radius/chord ratio ≈0.707 either way — and must be just
        // as accurate. Floating point is a relative, not absolute,
        // precision budget: a well-conditioned ratio stays well-
        // conditioned regardless of the absolute magnitude of the
        // numbers involved, which is exactly why `MAX_RADIUS_TO_CHORD_RATIO`
        // checks the *ratio*, not either value's magnitude (an earlier,
        // wrong revision clamped the absolute radius instead, and this
        // case is exactly what that wrongly broke).
        let start = Vec2::new(2_000_000.0, 0.0);
        let end = Vec2::new(0.0, 2_000_000.0);
        let segs = arc_to_cubics(start, 2_000_000.0, 2_000_000.0, 0.0, false, true, end).unwrap();
        assert_eq!(segs.len(), 1);
        let (c1, c2, reached_end) = segs[0];
        assert_eq!(reached_end, end);
        let mid = cubic_at_half(start, c1, c2, reached_end);
        let expected = 2_000_000.0 / std::f32::consts::SQRT_2;
        // Same relative tolerance as the human-scale quarter-circle test,
        // scaled up: 0.2 / 10.0 of the radius.
        let tolerance = expected * 0.02;
        assert!((mid.x - expected).abs() < tolerance, "mid = {mid:?}");
        assert!((mid.y - expected).abs() < tolerance, "mid = {mid:?}");
    }

    #[test]
    fn an_extreme_radius_to_chord_ratio_is_rejected_not_distorted() {
        // Review round 3's repro (radius 1e18 over a 10-unit chord, ratio
        // 1e17) is a textbook case for `MAX_RADIUS_TO_CHORD_RATIO`:
        // rejected outright now, instead of a round-3-style clamp that
        // would have silently changed the authored geometry.
        let segs = arc_to_cubics(
            Vec2::new(0.0, 0.0),
            1e18,
            1e18,
            0.0,
            false,
            true,
            Vec2::new(10.0, 0.0),
        );
        assert!(segs.is_none());
    }

    #[test]
    fn a_huge_radius_over_a_tiny_chord_is_rejected_even_at_a_merely_large_absolute_radius() {
        // Review round 4, case 1: radius 1e6 over a chord of 1e-12 has
        // ratio 1e18 — just as ill-conditioned as the 1e18-radius case
        // above, but reached through a *tiny chord* instead of a huge
        // radius. A radius-only check (round 3's mistake) would have let
        // this straight through since 1e6 isn't an unusually large radius
        // on its own; only the ratio catches it.
        let segs = arc_to_cubics(
            Vec2::new(0.0, 0.0),
            1e6,
            1e6,
            0.0,
            false,
            true,
            Vec2::new(1e-12, 0.0),
        );
        assert!(segs.is_none());
    }
}
