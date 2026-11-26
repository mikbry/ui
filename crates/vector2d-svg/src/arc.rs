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

/// Convert an SVG elliptical arc from `start` to `end` into a chain of cubic
/// Bézier segments, all in the same (SVG, y-down) coordinate space as
/// `start`/`end`. `x_axis_rotation_deg` is the ellipse's own rotation.
/// Returns `None` only for a degenerate case the caller should special-case
/// instead of calling this: `start == end` (spec: omit the arc) is handled
/// by the caller, not here.
pub fn arc_to_cubics(
    start: Vec2,
    rx: f32,
    ry: f32,
    x_axis_rotation_deg: f32,
    large_arc: bool,
    sweep: bool,
    end: Vec2,
) -> Vec<CubicSegment> {
    // Spec F.6.2: a zero radius is equivalent to a straight line. A cubic
    // with both control points pinned to the endpoints traces exactly that
    // line (a non-uniform but still straight parametrization).
    if rx == 0.0 || ry == 0.0 {
        return vec![(start, end, end)];
    }
    let rx = rx.abs();
    let ry = ry.abs();
    let phi = x_axis_rotation_deg.to_radians();
    let (sin_phi, cos_phi) = phi.sin_cos();

    // F.6.5 step 1: (x1', y1') = R(-phi) * (start - end) / 2
    let dx2 = (start.x - end.x) / 2.0;
    let dy2 = (start.y - end.y) / 2.0;
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
    let cx = cos_phi * cxp - sin_phi * cyp + (start.x + end.x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (start.y + end.y) / 2.0;

    // step 5: start/sweep angles (F.6.5 steps 3-4).
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;
    let theta1 = angle_between((1.0, 0.0), (ux, uy));
    let mut dtheta = angle_between((ux, uy), (vx, vy));
    if !sweep && dtheta > 0.0 {
        dtheta -= std::f32::consts::TAU;
    } else if sweep && dtheta < 0.0 {
        dtheta += std::f32::consts::TAU;
    }

    // Split into segments of at most 90 degrees for an accurate cubic fit.
    let segment_count = (dtheta.abs() / (std::f32::consts::FRAC_PI_2))
        .ceil()
        .max(1.0) as usize;
    let seg_sweep = dtheta / segment_count as f32;
    let mut segments = Vec::with_capacity(segment_count);
    let ellipse = RotatedEllipse {
        cx,
        cy,
        rx,
        ry,
        cos_phi,
        sin_phi,
    };
    let mut a0 = theta1;
    for _ in 0..segment_count {
        let a1 = a0 + seg_sweep;
        segments.push(cubic_for_arc_segment(&ellipse, a0, a1));
        a0 = a1;
    }
    segments
}

/// An ellipse centered at `(cx, cy)` with semi-axes `(rx, ry)`, rotated by
/// `(cos_phi, sin_phi)` — bundled so [`cubic_for_arc_segment`] takes one
/// argument instead of six.
struct RotatedEllipse {
    cx: f32,
    cy: f32,
    rx: f32,
    ry: f32,
    cos_phi: f32,
    sin_phi: f32,
}

/// Signed angle (radians) from vector `u` to vector `v`.
fn angle_between(u: (f32, f32), v: (f32, f32)) -> f32 {
    let dot = u.0 * v.0 + u.1 * v.1;
    let len = ((u.0 * u.0 + u.1 * u.1) * (v.0 * v.0 + v.1 * v.1)).sqrt();
    let cos_a = (dot / len).clamp(-1.0, 1.0);
    let sign = if u.0 * v.1 - u.1 * v.0 < 0.0 {
        -1.0
    } else {
        1.0
    };
    sign * cos_a.acos()
}

/// Cubic Bézier approximation of one `ellipse` arc segment spanning
/// `[a0, a1]` (radians, `|a1 - a0| <= pi/2`).
fn cubic_for_arc_segment(ellipse: &RotatedEllipse, a0: f32, a1: f32) -> CubicSegment {
    let &RotatedEllipse {
        cx,
        cy,
        rx,
        ry,
        cos_phi,
        sin_phi,
    } = ellipse;
    let point = |a: f32| -> Vec2 {
        let (sin_a, cos_a) = a.sin_cos();
        let ex = rx * cos_a;
        let ey = ry * sin_a;
        Vec2::new(
            cos_phi * ex - sin_phi * ey + cx,
            sin_phi * ex + cos_phi * ey + cy,
        )
    };
    let tangent = |a: f32| -> Vec2 {
        let (sin_a, cos_a) = a.sin_cos();
        let dx = -rx * sin_a;
        let dy = ry * cos_a;
        Vec2::new(cos_phi * dx - sin_phi * dy, sin_phi * dx + cos_phi * dy)
    };
    let d = a1 - a0;
    let t = 4.0 / 3.0 * (d / 4.0).tan();
    let p0 = point(a0);
    let p1 = point(a1);
    let t0 = tangent(a0);
    let t1 = tangent(a1);
    let c1 = Vec2::new(p0.x + t * t0.x, p0.y + t * t0.y);
    let c2 = Vec2::new(p1.x - t * t1.x, p1.y - t * t1.y);
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
        );
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
        );
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
        let small = arc_to_cubics(start, 10.0, 10.0, 0.0, false, true, end);
        let large = arc_to_cubics(start, 10.0, 10.0, 0.0, true, true, end);
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
        );
        for (c1, c2, end) in segs {
            assert!(c1.x.is_finite() && c1.y.is_finite());
            assert!(c2.x.is_finite() && c2.y.is_finite());
            assert!(end.x.is_finite() && end.y.is_finite());
        }
    }
}
