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
//! its single-quadratic approximation. A piece is emitted once that error is
//! provably below [`CUBIC_SUBDIVISION_TOLERANCE_DEG`]. Bounding the tangent (not
//! the positional error) is what keeps the recovered outline visually faithful
//! to the source cubic — the property the #137 acceptance test checks. The error
//! is exactly zero when the cubic is already a degree-elevated quadratic, so
//! smooth-but-curvy quadratics never subdivide.
//!
//! # Why a hodograph bound, not sampling
//!
//! An earlier revision *sampled* the tangent error at 9 fixed `t` values, which
//! cannot certify a bound: a near-cusp cubic can swing its tangent tens of
//! degrees between adjacent samples (Codex round-1 reproduced 43.7° of missed
//! error on `p0=(0,0), c1=(9.371965,0.099994), c2=(8.021810,0.048572),
//! p3=(1,0)`). The criterion is now an analytic bound built on the hodograph:
//!
//! - The cubic's derivative `B′(t)` is a quadratic Bézier; the averaged
//!   quadratic's derivative `Q′(t)` is linear. Their difference works out to
//!   `D(t) = φ(t)·v` where `v = 3(c2 − c1) + p0 − p3` (a fixed vector,
//!   `−B‴/6`) and `φ(t) = −3t² + 3t − ½` with `max|φ| = ½` on `[0, 1]` — the
//!   deviation is *exact*, not merely convex-hull bounded, and points in a
//!   single direction.
//! - Hence `sin θ(t) = |φ(t)|·|v × Q′(t)| / (|B′(t)|·|Q′(t)|)`. Bounding each
//!   factor (`|v × Q′|` is linear in `t`; `|Q′|` minimises in closed form;
//!   `|B′| ≥ min|Q′| − |v|/2`) yields a certified worst-case angle. Because the
//!   numerator carries the *cross* product with `v`, purely tangential
//!   deviation (e.g. a straight cubic with uneven parameterisation) costs 0°.
//! - Convergence: halving a cubic scales `v` by 1/8 (it is the constant third
//!   derivative) while speeds scale by 1/2, so the bound shrinks ~4× per level
//!   wherever the curve speed is bounded away from zero. Only an exact cusp
//!   (`B′ = 0`) defeats it, and there the depth cap applies (Sprint 8 §9
//!   garbage-in-garbage-out policy).
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
    if depth >= MAX_SUBDIVISION_DEPTH || angular_error_bound(p0, c1, c2, p3) <= tolerance_rad {
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

/// Certified upper bound (radians) on the tangent-direction error between the
/// cubic and its averaged single-quadratic approximation over all `t ∈ [0, 1]`.
///
/// Derivation (see module docs): with `v = 3(c2 − c1) + p0 − p3`, the
/// derivative difference is exactly `B′(t) − Q′(t) = φ(t)·v`, `max|φ| = ½`.
/// Then for every `t`
///
/// ```text
/// sin θ(t) = |B′ × Q′| / (|B′|·|Q′|) = |φ(t)|·|v × Q′(t)| / (|B′(t)|·|Q′(t)|)
///          ≤ ½·max(|v × E0|, |v × E1|) / ((min|Q′| − |v|/2)·min|Q′|)
/// ```
///
/// using that `v × Q′(t)` is linear in `t` (so extremal at the endpoints
/// `E0 = Q′(0)`, `E1 = Q′(1)`), that `|Q′(t)|` has a closed-form minimum, and
/// that `|B′| ≥ |Q′| − |φ·v| ≥ min|Q′| − |v|/2`. The same inequality
/// (`|B′ − Q′| < |Q′|` pointwise) also forces `B′·Q′ > 0`, so `θ < 90°` and
/// `asin` of the bound is valid. Returns exactly `0` when `v = 0` (the cubic is
/// a degree-elevated quadratic) and `INFINITY` whenever the guard
/// `min|Q′| > |v|/2` fails — e.g. anti-parallel end tangents driving `Q′`
/// through zero — so the recursion always subdivides toward a regime where the
/// bound is finite.
fn angular_error_bound(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> f32 {
    let v = Vec2::new(
        3.0 * (c2.x - c1.x) + p0.x - p3.x,
        3.0 * (c2.y - c1.y) + p0.y - p3.y,
    );
    let v_len = (v.x * v.x + v.y * v.y).sqrt();
    if v_len == 0.0 {
        return 0.0;
    }
    let q = quadratic_control(p0, c1, c2, p3);
    let e0 = Vec2::new(2.0 * (q.x - p0.x), 2.0 * (q.y - p0.y));
    let e1 = Vec2::new(2.0 * (p3.x - q.x), 2.0 * (p3.y - q.y));
    let min_quad_speed = min_segment_norm(e0, e1);
    let min_cubic_speed = min_quad_speed - 0.5 * v_len;
    if min_cubic_speed <= 0.0 {
        return f32::INFINITY;
    }
    let cross_e0 = (v.x * e0.y - v.y * e0.x).abs();
    let cross_e1 = (v.x * e1.y - v.y * e1.x).abs();
    let sin_bound = 0.5 * cross_e0.max(cross_e1) / (min_cubic_speed * min_quad_speed);
    if sin_bound >= 1.0 {
        f32::INFINITY
    } else {
        sin_bound.asin()
    }
}

/// Minimum norm of the linear interpolation `(1 − t)·e0 + t·e1` over
/// `t ∈ [0, 1]`, computed exactly: `|·|²` is quadratic in `t`, so the minimum
/// is at the clamped vertex.
fn min_segment_norm(e0: Vec2, e1: Vec2) -> f32 {
    let dx = e1.x - e0.x;
    let dy = e1.y - e0.y;
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (-(e0.x * dx + e0.y * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = e0.x + t * dx;
    let y = e0.y + t * dy;
    (x * x + y * y).sqrt()
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

    /// Tangent (derivative direction) of a cubic Bézier at `t`.
    fn cubic_tangent(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2, t: f32) -> Vec2 {
        let mt = 1.0 - t;
        Vec2::new(
            3.0 * mt * mt * (c1.x - p0.x)
                + 6.0 * mt * t * (c2.x - c1.x)
                + 3.0 * t * t * (p3.x - c2.x),
            3.0 * mt * mt * (c1.y - p0.y)
                + 6.0 * mt * t * (c2.y - c1.y)
                + 3.0 * t * t * (p3.y - c2.y),
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
    /// (near-)zero-length so a degenerate tangent never inflates the error.
    fn angle_between(a: Vec2, b: Vec2) -> f32 {
        let cross = a.x * b.y - a.y * b.x;
        let dot = a.x * b.x + a.y * b.y;
        if cross == 0.0 && dot == 0.0 {
            0.0
        } else {
            cross.abs().atan2(dot)
        }
    }

    /// Worst tangent-direction error (radians) of the emitted chain against the
    /// source cubic, dense-sampling `samples_per_piece` uniform `t` values on
    /// every emitted piece via the exact `[t0, t1]` correspondence.
    fn worst_chain_error(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2, samples_per_piece: u32) -> f32 {
        let pieces = subdivide_pieces(p0, c1, c2, p3);
        assert!(!pieces.is_empty());
        let mut start = p0;
        let mut worst = 0.0_f32;
        for piece in &pieces {
            for k in 0..=samples_per_piece {
                let u = k as f32 / samples_per_piece as f32;
                let t = piece.t0 + (piece.t1 - piece.t0) * u;
                let ct = cubic_tangent(p0, c1, c2, p3, t);
                let qt = quad_tangent(start, piece.control, piece.end, u);
                worst = worst.max(angle_between(ct, qt));
            }
            start = piece.end;
        }
        worst
    }

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
        // Each stresses subdivision differently: an arch, a strong S, a wiggle,
        // and the Codex round-1 near-cusp loop cubic.
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
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(9.371965, 0.099994),
                Vec2::new(8.02181, 0.048572),
                Vec2::new(1.0, 0.0),
            ),
        ];
        let one_degree = 1.0_f32.to_radians();
        for &(p0, c1, c2, p3) in &cubics {
            let worst = worst_chain_error(p0, c1, c2, p3, 24);
            assert!(
                worst <= one_degree + 1e-3,
                "worst angular error {} rad exceeds 1°",
                worst
            );
        }
    }

    /// Regression for the Codex round-1 finding: the fixed-sample criterion let
    /// this near-cusp cubic emit a piece for `[0.375, 0.5]` with ~43.7° of
    /// tangent error near `t = 0.494375`. The hodograph bound must keep the
    /// dense-checked error inside the advertised 1° contract.
    #[test]
    fn codex_r1_pathological_cubic_stays_within_bound() {
        let worst = worst_chain_error(
            Vec2::new(0.0, 0.0),
            Vec2::new(9.371965, 0.099994),
            Vec2::new(8.02181, 0.048572),
            Vec2::new(1.0, 0.0),
            400,
        );
        assert!(
            worst <= 1.0_f32.to_radians() + 1e-3,
            "worst angular error {} rad ({}°) exceeds 1°",
            worst,
            worst.to_degrees()
        );
    }

    /// Property test: the 1° contract holds under dense sampling (200+ uniform
    /// `t` values per emitted piece) across a suite of hard cubics — near-cusp
    /// loops, sharp-inflection S-curves, cusp-adjacent curves, retrograde and
    /// nearly-collinear controls. Exact cusps (`B′ = 0`) are excluded: the
    /// tangent flips 180° instantaneously there and no finite quadratic chain
    /// can bound it (Sprint 8 §9 garbage-in-garbage-out, depth-capped).
    #[test]
    fn dense_sampling_confirms_one_degree_bound_on_hard_cubics() {
        let cubics = [
            // Codex round-1 near-cusp loop.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(9.371965, 0.099994),
                Vec2::new(8.02181, 0.048572),
                Vec2::new(1.0, 0.0),
            ),
            // Same family, controls pulled even further past the endpoints.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(12.0, 0.2),
                Vec2::new(10.5, -0.15),
                Vec2::new(1.0, 0.0),
            ),
            // Sharp-inflection S: long opposing control arms.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(150.0, 5.0),
                Vec2::new(-50.0, -5.0),
                Vec2::new(100.0, 0.0),
            ),
            // Near the cusp boundary: retrograde controls, tiny transverse
            // offset keeps the speed nonzero.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(100.0, 100.5),
                Vec2::new(0.0, 99.5),
                Vec2::new(100.0, 100.0),
            ),
            // Tight loop with crossing control arms.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(200.0, 150.0),
                Vec2::new(-100.0, 150.0),
                Vec2::new(100.0, 0.0),
            ),
            // Nearly collinear with strongly uneven parameterisation.
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(90.0, 0.9),
                Vec2::new(95.0, 0.95),
                Vec2::new(100.0, 1.0),
            ),
            // Micro-scale copy of the Codex curve (f32 robustness at small
            // magnitudes).
            (
                Vec2::new(0.0, 0.0),
                Vec2::new(0.09371965, 0.00099994),
                Vec2::new(0.0802181, 0.00048572),
                Vec2::new(0.01, 0.0),
            ),
        ];
        let one_degree = 1.0_f32.to_radians();
        for &(p0, c1, c2, p3) in &cubics {
            let worst = worst_chain_error(p0, c1, c2, p3, 200);
            assert!(
                worst <= one_degree + 1e-3,
                "cubic {:?} worst angular error {} rad ({}°) exceeds 1°",
                (p0, c1, c2, p3),
                worst,
                worst.to_degrees()
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
