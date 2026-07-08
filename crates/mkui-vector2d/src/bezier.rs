//! Cubic → quadratic Bézier subdivision.
//!
//! Slug rasterizes **quadratic** Béziers only, so the general-path encoder
//! ([`crate::encode`]) cannot hand a cubic segment straight to the band builder.
//! This module lowers a cubic `(p0, c1, c2, p3)` into a chain of quadratics by
//! recursively halving it (de Casteljau) until each piece's single-quadratic
//! approximation is within a hard-coded angular tolerance of the source, then
//! emitting that quadratic.
//!
//! # Why an angular tolerance
//!
//! The subdivision test is the **tangent-direction error** between the cubic and
//! its single-quadratic approximation, sampled across the piece. A piece is
//! emitted once that error drops below [`CUBIC_SUBDIVISION_TOLERANCE_DEG`].
//! Bounding the tangent (not the positional error) is what keeps the recovered
//! outline visually faithful to the source cubic — the property the #137
//! acceptance test checks. The error is exactly zero when the cubic is already a
//! degree-elevated quadratic, so smooth-but-curvy quadratics never subdivide.
//!
//! The tolerance is hard-coded per Sprint 8 §3.1 Wave 1; exposing it as an
//! opt-in encoder knob is a Sprint 9+ candidate.

use crate::path::Vec2;

/// Hard-coded angular-error tolerance for cubic→quadratic subdivision, in
/// degrees (Sprint 8 §3.1 Wave 1). A cubic piece is emitted as a single
/// quadratic once that quadratic's tangent direction stays within this angle of
/// the source cubic across the piece.
pub const CUBIC_SUBDIVISION_TOLERANCE_DEG: f32 = 1.0;

/// Safety backstop on recursion depth. A well-behaved cubic converges in a
/// handful of splits; this only bounds pathological inputs (e.g. a cusp) so the
/// encoder always terminates. Sprint 8 §9 policy is garbage-in-garbage-out on
/// degenerate geometry — we cap rather than validate.
const MAX_SUBDIVISION_DEPTH: u32 = 18;

/// Sample count for the tangent-error estimate over `t ∈ [0, 1]`. Nine samples
/// (endpoints + interior) reliably capture the worst-case tangent deviation of
/// the near-flat pieces the recursion produces.
const ERROR_SAMPLES: u32 = 8;

/// Lower a cubic Bézier `(p0, c1, c2, p3)` into a chain of quadratic segments.
///
/// Each returned `(control, end)` pair is one quadratic whose implicit start
/// point is the previous segment's `end` (the first segment starts at `p0`).
/// The chain is ordered start→end and always covers the full `t ∈ [0, 1]` span.
pub(crate) fn subdivide_cubic(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> Vec<(Vec2, Vec2)> {
    subdivide_pieces(p0, c1, c2, p3)
        .into_iter()
        .map(|p| (p.control, p.end))
        .collect()
}

/// One emitted quadratic piece plus the source-cubic parameter interval
/// `[t0, t1]` it covers. Production only needs `(control, end)`; the parameter
/// span lets the acceptance test establish an exact, fold-free correspondence
/// against the source cubic (subdivision is adaptive, so spans are unequal).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Piece {
    pub control: Vec2,
    pub end: Vec2,
    // Source-cubic parameter span, read only by the acceptance test's exact
    // correspondence check; production consumes just `control`/`end`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub t0: f32,
    #[cfg_attr(not(test), allow(dead_code))]
    pub t1: f32,
}

pub(crate) fn subdivide_pieces(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> Vec<Piece> {
    let tolerance_rad = CUBIC_SUBDIVISION_TOLERANCE_DEG.to_radians();
    let mut out = Vec::new();
    flatten(p0, c1, c2, p3, tolerance_rad, 0, 0.0, 1.0, &mut out);
    out
}

#[allow(clippy::too_many_arguments)]
fn flatten(
    p0: Vec2,
    c1: Vec2,
    c2: Vec2,
    p3: Vec2,
    tolerance_rad: f32,
    depth: u32,
    t0: f32,
    t1: f32,
    out: &mut Vec<Piece>,
) {
    if depth >= MAX_SUBDIVISION_DEPTH || max_angular_error(p0, c1, c2, p3) <= tolerance_rad {
        out.push(Piece {
            control: quadratic_control(p0, c1, c2, p3),
            end: p3,
            t0,
            t1,
        });
        return;
    }
    let (left, right) = split_at_half(p0, c1, c2, p3);
    let tm = 0.5 * (t0 + t1);
    flatten(
        left.0,
        left.1,
        left.2,
        left.3,
        tolerance_rad,
        depth + 1,
        t0,
        tm,
        out,
    );
    flatten(
        right.0,
        right.1,
        right.2,
        right.3,
        tolerance_rad,
        depth + 1,
        tm,
        t1,
        out,
    );
}

/// Maximum tangent-direction error (radians) between the cubic and its averaged
/// single-quadratic approximation, sampled across `t ∈ [0, 1]`. Zero when the
/// cubic is already a degree-elevated quadratic, so exact quadratics never
/// subdivide; grows with the cubic's departure from quadratic form.
fn max_angular_error(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> f32 {
    let q = quadratic_control(p0, c1, c2, p3);
    let mut worst = 0.0_f32;
    for i in 0..=ERROR_SAMPLES {
        let t = i as f32 / ERROR_SAMPLES as f32;
        let ct = cubic_tangent(p0, c1, c2, p3, t);
        let qt = quad_tangent(p0, q, p3, t);
        worst = worst.max(angle_between(ct, qt));
    }
    worst
}

/// Tangent (derivative direction) of a cubic Bézier at `t`.
fn cubic_tangent(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2, t: f32) -> Vec2 {
    let mt = 1.0 - t;
    Vec2::new(
        3.0 * mt * mt * (c1.x - p0.x) + 6.0 * mt * t * (c2.x - c1.x) + 3.0 * t * t * (p3.x - c2.x),
        3.0 * mt * mt * (c1.y - p0.y) + 6.0 * mt * t * (c2.y - c1.y) + 3.0 * t * t * (p3.y - c2.y),
    )
}

/// Tangent (derivative direction) of a quadratic Bézier at `t`.
fn quad_tangent(p0: Vec2, q: Vec2, p2: Vec2, t: f32) -> Vec2 {
    let mt = 1.0 - t;
    Vec2::new(
        2.0 * mt * (q.x - p0.x) + 2.0 * t * (p2.x - q.x),
        2.0 * mt * (q.y - p0.y) + 2.0 * t * (p2.y - q.y),
    )
}

/// Unsigned angle (radians) between two direction vectors; `0` if either is
/// (near-)zero-length so a cusp or degenerate tangent never inflates the error.
fn angle_between(a: Vec2, b: Vec2) -> f32 {
    let cross = a.x * b.y - a.y * b.x;
    let dot = a.x * b.x + a.y * b.y;
    if cross == 0.0 && dot == 0.0 {
        0.0
    } else {
        cross.abs().atan2(dot)
    }
}

/// The single quadratic control point approximating this cubic, taken as the
/// average of the two control points implied by degree-reducing each half of
/// the cubic: `Q = (3·c1 − p0 + 3·c2 − p3) / 4`. For the near-flat pieces this
/// is only invoked on, `Q` sits essentially on the segment, so the resulting
/// quadratic reproduces both endpoint tangents to within the tolerance.
fn quadratic_control(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> Vec2 {
    Vec2::new(
        (3.0 * c1.x - p0.x + 3.0 * c2.x - p3.x) / 4.0,
        (3.0 * c1.y - p0.y + 3.0 * c2.y - p3.y) / 4.0,
    )
}

/// A cubic's four control points `(p0, c1, c2, p3)`.
type CubicPoints = (Vec2, Vec2, Vec2, Vec2);

/// de Casteljau split of a cubic at `t = 0.5` into its left and right sub-cubics.
fn split_at_half(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> (CubicPoints, CubicPoints) {
    let m01 = midpoint(p0, c1);
    let m12 = midpoint(c1, c2);
    let m23 = midpoint(c2, p3);
    let m012 = midpoint(m01, m12);
    let m123 = midpoint(m12, m23);
    let mid = midpoint(m012, m123);
    ((p0, m01, m012, mid), (mid, m123, m23, p3))
}

fn midpoint(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reconstruct the quad chain as (start, control, end) triples.
    fn quad_chain(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> Vec<(Vec2, Vec2, Vec2)> {
        let mut start = p0;
        subdivide_cubic(p0, c1, c2, p3)
            .into_iter()
            .map(|(q, end)| {
                let seg = (start, q, end);
                start = end;
                seg
            })
            .collect()
    }

    /// The chain endpoints must exactly bracket the source cubic.
    #[test]
    fn chain_covers_the_full_span() {
        let (p0, c1, c2, p3) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 100.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 0.0),
        );
        let chain = quad_chain(p0, c1, c2, p3);
        assert!(!chain.is_empty());
        assert_eq!(chain.first().unwrap().0, p0);
        assert_eq!(chain.last().unwrap().2, p3);
        // Segments are contiguous: each start equals the previous end.
        for w in chain.windows(2) {
            assert_eq!(w[0].2, w[1].0);
        }
    }

    /// Acceptance criterion (#137): the recovered quadratic chain stays within
    /// 1° angular error of the source cubic. Subdivision is *adaptive*, so each
    /// piece reports the source-cubic parameter interval `[t0, t1]` it covers;
    /// we compare the quad's tangent at local `u` against the source cubic's
    /// tangent at the corresponding global `t`. Correspondence is monotone
    /// within a piece, so — unlike nearest-point matching — a self-approaching S
    /// or wiggle cannot fold one branch onto another.
    #[test]
    fn recovered_curve_within_one_degree_of_source_cubic() {
        // Each stresses subdivision differently: an arch, a strong S, a wiggle.
        let cubics = [
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(0.0, 100.0),
                Vec2::new(100.0, 100.0),
                Vec2::new(100.0, 0.0),
            ),
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(120.0, 40.0),
                Vec2::new(-20.0, 40.0),
                Vec2::new(100.0, 0.0),
            ),
            (
                Vec2::new(10.0, 10.0),
                Vec2::new(40.0, 200.0),
                Vec2::new(160.0, -120.0),
                Vec2::new(200.0, 60.0),
            ),
        ];
        let one_degree = 1.0_f32.to_radians();
        for &(p0, c1, c2, p3) in &cubics {
            let pieces = subdivide_pieces(p0, c1, c2, p3);
            let mut start = p0;
            let mut worst = 0.0_f32;
            for piece in &pieces {
                for k in 0..=24 {
                    let u = k as f32 / 24.0;
                    let t = piece.t0 + (piece.t1 - piece.t0) * u;
                    let ct = cubic_tangent(p0, c1, c2, p3, t);
                    let qt = quad_tangent(start, piece.control, piece.end, u);
                    worst = worst.max(angle_between(ct, qt));
                }
                start = piece.end;
            }
            assert!(
                worst <= one_degree + 1e-3,
                "worst angular error {} rad exceeds 1°",
                worst
            );
        }
    }

    /// A cubic that is already a degree-elevated quadratic collapses to a single
    /// quadratic with no subdivision.
    #[test]
    fn exact_quadratic_needs_no_subdivision() {
        // Elevate the quadratic (p0, q, p2) to a cubic and check round-trip.
        let p0 = Vec2::new(0.0, 0.0);
        let q = Vec2::new(50.0, 80.0);
        let p2 = Vec2::new(100.0, 0.0);
        let c1 = Vec2::new((p0.x + 2.0 * q.x) / 3.0, (p0.y + 2.0 * q.y) / 3.0);
        let c2 = Vec2::new((p2.x + 2.0 * q.x) / 3.0, (p2.y + 2.0 * q.y) / 3.0);
        let quads = subdivide_cubic(p0, c1, c2, p2);
        assert_eq!(quads.len(), 1);
        let (control, end) = quads[0];
        assert!((control.x - q.x).abs() < 1e-3 && (control.y - q.y).abs() < 1e-3);
        assert_eq!(end, p2);
    }

    /// A straight cubic (all controls collinear) never over-subdivides.
    #[test]
    fn straight_cubic_stays_flat() {
        let quads = subdivide_cubic(
            Vec2::new(0.0, 0.0),
            Vec2::new(25.0, 0.0),
            Vec2::new(75.0, 0.0),
            Vec2::new(100.0, 0.0),
        );
        assert_eq!(quads.len(), 1);
    }
}
