//! Cubic → quadratic Bézier subdivision.
//!
//! Slug rasterizes **quadratic** Béziers only, so the general-path encoder
//! ([`crate::encode`]) cannot hand a cubic segment straight to the band builder.
//! This module lowers a cubic `(p0, c1, c2, p3)` into a chain of quadratics by
//! recursively halving it (de Casteljau) until each piece's single-quadratic
//! approximation is within a hard-coded angular tolerance of the source, then
//! emitting that quadratic — or returns a typed [`SubdivisionError`] when no
//! `f32` chain can honor the tolerance.
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
//! p3=(1,0)`). The criterion is an analytic bound built on the hodograph: the
//! cubic's derivative `B′(t)` is a quadratic Bézier, a quadratic candidate's
//! derivative `Q′(t)` is linear, and `cross(B′, Q′)` / `dot(B′, Q′)` are each
//! an *exact* cubic polynomial (product of a quadratic and a linear Bézier
//! function). [`general_sin_bound`] finds their global extrema exactly, via
//! the cubic's (quadratic) derivative roots — not a loose coefficient-hull
//! bound, which Codex round-5's investigation showed is too weak to certify
//! near a genuine high-curvature feature. Subdivision runs entirely in `f64`
//! ([`DVec2`]) — Codex round-2 showed `f32` de Casteljau collapses control
//! deltas into rounding noise before certification can succeed.
//!
//! # Certification targets the *emitted* `f32` quadratic
//!
//! Production emits `f32` control points, and Codex round-4 proved that
//! certifying the pre-rounding `f64` quadratic is not enough: rounding the
//! round-2 reproducer's certified chain to `f32` reintroduced ~75° of tangent
//! error. The criterion therefore measures the exact quadratic the caller will
//! receive: a candidate control point's start/control/end are rounded to
//! `f32` *first*, and the analytic bound is evaluated against that rounded
//! quadratic's derivative. A piece is emitted only when the rounded quadratic
//! itself certifies.
//!
//! # Control-point selection, not just depth
//!
//! `PrecisionUnderflow` is returned iff no `f32` chain within
//! [`MAX_SUBDIVISION_DEPTH`] can honor the 1° bound — this depth cap is the
//! *only* trigger for that error; there is no heuristic pre-check that could
//! false-positive (an earlier revision had one, and Codex round-5 found it
//! over-rejected ordinary, well-resolved curves). But removing the pre-check
//! alone is not sufficient: Codex round-5's `(999.65,1000.0), (999.4,999.1),
//! (999.95,999.65), (999.25,1000.55)` sits in a region of genuinely high
//! local curvature (~11° of true tangent rotation across a piece only ~1.5
//! millipixels wide at that offset), and the **averaged** degree-reduction
//! control's approximate endpoint-tangent match cannot reach 1° there —
//! empirically, its error *increases* monotonically on further subdivision
//! (1.2° → 15° over the next three levels) as `f32` rounding of the
//! ever-smaller piece overwhelms the true (shrinking) geometric error, so no
//! amount of depth alone fixes it.
//!
//! The fix is a second, independent control-point candidate: the
//! **tangent-line intersection** ([`tangent_intersection_control`]) —
//! intersecting the ray from `p0` along `c1 − p0` with the ray from `p3`
//! along `c2 − p3` — matches *both* endpoint tangent directions exactly (zero
//! angular error at `u = 0` and `u = 1` by construction), leaving only the
//! interior curvature mismatch to bound, typically far smaller than the
//! averaged formula's error. `flatten` evaluates every viable candidate with
//! [`general_sin_bound`] and keeps the tightest; a piece is emitted once
//! *any* candidate certifies. This resolves the round-5 reproducer at depth
//! 7 with true error ~0.02° (down from the averaged formula's 1.2°) — see
//! `codex_r5_normal_scale_cubic_certifies_not_underflow`. `PrecisionUnderflow`
//! remains reserved for genuine `f32`-resolution limits: the round-2
//! reproducer's pseudo-cusp still exhausts every candidate down to the depth
//! cap and returns `Err` (see its regression) — `f32`-scale non-convergence
//! localizes to a shrinking sub-interval around the pathological point, so at
//! most two branches (not a full `2^depth` fan-out) ever run that deep.
//!
//! # Endpoint-stationary cubics are valid input
//!
//! A cubic with `c1 == p0` (or `c2 == p3`) has `B′ = 0` at that endpoint —
//! common in real paths and *not* a tangent-flip cusp (Codex round-4's
//! counterexample `(0,0), (0,0), (50,100), (100,0)` is a perfectly smooth
//! arch). [`general_sin_bound`]'s denominator floor collapses for such pieces
//! (nothing on the `B′` side partially cancels the `Q′` hull term at the
//! vanishing endpoint), but the derivative factors exactly: with `c1 == p0`,
//! `B′(u) = u·ψ(u)` where `ψ(u) = 6(1−u)(c2−c1) + 3u(p3−c2)` is **linear**,
//! so for `u > 0` the tangent direction is ψ's direction and the limit
//! tangent at the stationary endpoint is `ψ(0)` — well-defined geometry, the
//! direction toward the next distinct control point. [`stationary_sin_bound`]
//! dispatches on exact endpoint-stationarity and bounds `angle(ψ, Q′)`
//! (linear vs linear: exact extrema of the resulting quadratic cross and dot
//! polynomials) instead of `angle(B′, Q′)`. Stationary pieces use only the
//! **chord midpoint** as their sole candidate — the averaged formula is
//! numerically catastrophic there (on the stationary spine `p3 → 3·c2`, so
//! `q = (3·c2 − p3)/4` cancels to noise), and the tangent-line intersection is
//! undefined (one of the two directions is zero). Mirrored for `c2 == p3`; a
//! doubly-stationary cubic (`c1 == p0` and `c2 == p3`) has constant
//! derivative direction `c2 − c1` and uses the same machinery with a constant
//! ψ. Only a cubic with **no** direction anywhere — all four control points
//! equal, `B′ ≡ 0` — is rejected as [`SubdivisionError::DegenerateInput`].
//!
//! There are no panics, assertions, or uncertified emissions anywhere in the
//! subdivision path: every failure mode is a typed error.
//!
//! The tolerance is hard-coded per Sprint 8 §3.1 Wave 1; exposing it as an
//! opt-in encoder knob is a Sprint 9+ candidate.

use crate::path::Vec2;

/// Hard-coded angular-error tolerance for cubic→quadratic subdivision, in
/// degrees (Sprint 8 §3.1 Wave 1). A cubic piece is emitted as a single
/// quadratic once that quadratic's tangent direction — as actually emitted in
/// `f32` — stays within this angle of the source cubic across the piece.
pub const CUBIC_SUBDIVISION_TOLERANCE_DEG: f32 = 1.0;

/// Backstop on recursion depth. **Tightened invariant (Codex round-5):
/// [`SubdivisionError::PrecisionUnderflow`] is returned iff no `f32` chain
/// within this depth can honor the 1° bound** — this cap is the *only*
/// trigger for that error (see module docs); there is no heuristic pre-check,
/// so reaching it is not a panic and not a prediction, only a report that
/// certification was tried at every level up to here and never succeeded.
///
/// In `f64` the bound shrinks ~4× per level wherever the derivative direction
/// is resolvable, so real inputs finish far below this cap. Observed on the
/// three Codex reproducers (measured by this module's regression tests): the
/// round-1 near-cusp loop certifies every emitted piece by depth 6; the
/// round-5 high-curvature cubic (resolved by the tangent-intersection
/// candidate — see module docs) certifies by depth 7; the round-2
/// `f32`-collapsing cubic exhausts recursion and returns
/// `PrecisionUnderflow` — its pseudo-cusp is genuinely below `f32`
/// resolution, and no amount of splitting or control-point choice fixes
/// that. The cap bounds the work spent proving that.
const MAX_SUBDIVISION_DEPTH: u32 = 48;

/// Typed failure of [`subdivide_cubic`] / [`subdivide_pieces`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SubdivisionError {
    /// The emitted `f32` quadratic chain cannot honor the 1° tangent bound.
    /// This happens on numerically-tiny cubics (path scale near `f32` epsilon,
    /// or curvature features finer than one `f32` ULP) where the `f32`
    /// representation of control points cannot support the bound. Callers
    /// should either work at a larger scale or accept the loss of precision.
    #[error("cubic needs pieces below f32 resolution to honor the 1° tangent bound")]
    PrecisionUnderflow,
    /// The input cubic's derivative vanishes identically (all four control
    /// points equal — a point, not a curve). There is no tangent direction to
    /// subdivide against. Merely endpoint-stationary cubics (`c1 == p0` or
    /// `c2 == p3`) are valid and never produce this error.
    #[error("cubic is fully degenerate (all control points equal): no tangent direction")]
    DegenerateInput,
}

/// Internal double-precision point. All subdivision arithmetic runs on this
/// type; `f32` appears only at the API boundary (lossless in, rounded out) and
/// inside the certifier, which rounds each emission candidate to `f32` and
/// certifies the rounded quadratic.
#[derive(Debug, Clone, Copy)]
struct DVec2 {
    x: f64,
    y: f64,
}

impl DVec2 {
    fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn from_vec2(v: Vec2) -> Self {
        Self {
            x: f64::from(v.x),
            y: f64::from(v.y),
        }
    }

    fn to_vec2(self) -> Vec2 {
        Vec2::new(self.x as f32, self.y as f32)
    }

    /// The value production will actually emit: rounded to `f32`, re-widened.
    fn f32_rounded(self) -> Self {
        Self::from_vec2(self.to_vec2())
    }

    fn is_zero(self) -> bool {
        self.x == 0.0 && self.y == 0.0
    }
}

fn sub(a: DVec2, b: DVec2) -> DVec2 {
    DVec2::new(a.x - b.x, a.y - b.y)
}

fn scale(a: DVec2, s: f64) -> DVec2 {
    DVec2::new(a.x * s, a.y * s)
}

fn cross(a: DVec2, b: DVec2) -> f64 {
    a.x * b.y - a.y * b.x
}

fn dot(a: DVec2, b: DVec2) -> f64 {
    a.x * b.x + a.y * b.y
}

fn norm(a: DVec2) -> f64 {
    (a.x * a.x + a.y * a.y).sqrt()
}

/// Lower a cubic Bézier `(p0, c1, c2, p3)` into a chain of quadratic segments.
///
/// Each returned `(control, end)` pair is one quadratic whose implicit start
/// point is the previous segment's `end` (the first segment starts at `p0`).
/// The chain is ordered start→end and always covers the full `t ∈ [0, 1]`
/// span. Every emitted piece — as `f32`, exactly as returned — is certified
/// against the source cubic by the analytic tangent bound (see module docs).
///
/// # Errors
///
/// - [`SubdivisionError::DegenerateInput`] if all four control points are
///   equal (the "cubic" is a point with no tangent direction). Cubics that are
///   merely endpoint-stationary (`c1 == p0` and/or `c2 == p3`) are valid.
/// - [`SubdivisionError::PrecisionUnderflow`] if no `f32` quadratic chain can
///   honor the tolerance — the input needs pieces below `f32` resolution
///   (numerically-tiny curvature features, e.g. the Codex round-2 reproducer's
///   pseudo-cusp, or an interior true cusp). Nothing is emitted on error.
pub fn subdivide_cubic(
    p0: Vec2,
    c1: Vec2,
    c2: Vec2,
    p3: Vec2,
) -> Result<Vec<(Vec2, Vec2)>, SubdivisionError> {
    Ok(subdivide_pieces(p0, c1, c2, p3)?
        .into_iter()
        .map(|p| (p.control, p.end))
        .collect())
}

/// One emitted quadratic piece plus the source-cubic parameter interval
/// `[t0, t1]` it covers. Production only needs `(control, end)`; the exact
/// dyadic parameter span lets the tests establish a fold-free correspondence
/// against the source cubic (subdivision is adaptive, so spans are unequal).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Piece {
    pub control: Vec2,
    pub end: Vec2,
    #[cfg_attr(not(test), allow(dead_code))]
    pub t0: f64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub t1: f64,
}

/// See [`subdivide_cubic`] (including the error conditions); this variant also
/// reports each piece's source-parameter span.
pub(crate) fn subdivide_pieces(
    p0: Vec2,
    c1: Vec2,
    c2: Vec2,
    p3: Vec2,
) -> Result<Vec<Piece>, SubdivisionError> {
    if p0 == c1 && c1 == c2 && c2 == p3 {
        return Err(SubdivisionError::DegenerateInput);
    }
    let tolerance_rad = f64::from(CUBIC_SUBDIVISION_TOLERANCE_DEG).to_radians();
    let mut out = Vec::new();
    flatten(
        DVec2::from_vec2(p0),
        DVec2::from_vec2(c1),
        DVec2::from_vec2(c2),
        DVec2::from_vec2(p3),
        tolerance_rad,
        0,
        0.0,
        1.0,
        &mut out,
    )?;
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn flatten(
    p0: DVec2,
    c1: DVec2,
    c2: DVec2,
    p3: DVec2,
    tolerance_rad: f64,
    depth: u32,
    t0: f64,
    t1: f64,
    out: &mut Vec<Piece>,
) -> Result<(), SubdivisionError> {
    // Emission-control candidates (see module docs, "Control-point selection,
    // not just depth"). Endpoint-stationary pieces get a single candidate,
    // the chord midpoint, certified by [`stationary_sin_bound`] — the
    // averaged degree-reduction formula is numerically catastrophic there
    // (on the stationary spine `p3 → 3·c2`, `q = (3·c2 − p3)/4` cancels to
    // noise), and [`general_sin_bound`]'s denominator floor collapses
    // whenever the true derivative is genuinely zero at an endpoint (nothing
    // on the `B′` side partially cancels the `Q̃′` hull term), so neither
    // applies. Generic pieces get two candidates evaluated by
    // [`general_sin_bound`]: the averaged formula, and — whenever the two
    // tangent lines are well-defined and not near-parallel — their
    // intersection, which exactly matches both endpoint tangent directions.
    // The latter is what actually resolves Codex round-5's high-curvature
    // reproducer, where the averaged formula's *approximate* endpoint-tangent
    // match alone exceeds 1° and only gets worse under further subdivision
    // (verified: its error strictly *increases*, 1.2° → 15°, over the next
    // three levels — not a case depth alone can fix). The tightest
    // certifying candidate wins.
    let start_stationary = sub(c1, p0).is_zero();
    let end_stationary = sub(p3, c2).is_zero();
    let eq0 = p0.f32_rounded();
    let eq1 = p3.f32_rounded();
    let bound_for = |q: DVec2| -> f64 {
        let eqc = q.f32_rounded();
        let te0 = scale(sub(eqc, eq0), 2.0);
        let te1 = scale(sub(eq1, eqc), 2.0);
        if start_stationary || end_stationary {
            stationary_sin_bound(c1, p0, c2, p3, start_stationary, end_stationary, te0, te1)
        } else {
            let (d0, d1, d2) = hodograph(p0, c1, c2, p3);
            general_sin_bound(d0, d1, d2, te0, te1)
        }
    };
    let mut candidates = [None; 3];
    let mut n = 0;
    if !start_stationary && !end_stationary {
        candidates[n] = Some(quadratic_control(p0, c1, c2, p3));
        n += 1;
        if let Some(tq) = tangent_intersection_control(p0, c1, c2, p3) {
            candidates[n] = Some(tq);
            n += 1;
        }
    }
    candidates[n] = Some(midpoint(p0, p3));
    n += 1;

    let mut best: Option<(f64, DVec2)> = None;
    for q in candidates.into_iter().take(n).flatten() {
        let bound = bound_for(q);
        if best.is_none_or(|(b, _)| bound < b) {
            best = Some((bound, q));
        }
    }
    #[allow(clippy::unwrap_used)]
    let (bound, q) = best.unwrap(); // candidates always yields at least the chord midpoint

    if bound <= tolerance_rad {
        out.push(Piece {
            control: q.to_vec2(),
            end: p3.to_vec2(),
            t0,
            t1,
        });
        return Ok(());
    }
    // Tightened invariant (Codex round-5): PrecisionUnderflow is returned iff
    // no f32 chain within the depth cap can honor the 1° bound, for any of
    // the candidate controls above. There is no heuristic pre-check — this
    // depth-cap check is the *only* trigger for the error, so reaching it
    // cannot false-positive (see module docs and [`MAX_SUBDIVISION_DEPTH`]).
    if depth >= MAX_SUBDIVISION_DEPTH {
        return Err(SubdivisionError::PrecisionUnderflow);
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
    )?;
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
    )
}

/// Bernstein control points of the cubic's derivative hodograph: `B′(t)` is a
/// **quadratic** Bézier with these three control points, for *any* cubic —
/// including endpoint-stationary ones, where `d0` or `d2` is simply `(0, 0)`
/// rather than requiring special-case dispatch.
fn hodograph(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> (DVec2, DVec2, DVec2) {
    (
        scale(sub(c1, p0), 3.0),
        scale(sub(c2, c1), 3.0),
        scale(sub(p3, c2), 3.0),
    )
}

/// Certified upper bound (radians) on the tangent-direction error between the
/// true cubic hodograph `(d0, d1, d2)` (see [`hodograph`]) and an emitted
/// quadratic's linear derivative `(1−u)·te0 + u·te1`; `INFINITY` when no
/// finite certification holds (the recursion then tries another candidate,
/// subdivides, or fails fast). One formula for *any* control-point choice —
/// no dispatch on endpoint-stationarity, unlike an earlier revision.
///
/// `cross(B′, Q̃′)` and `dot(B′, Q̃′)` are each the product of a quadratic and
/// a linear Bézier function, hence an *exact* cubic polynomial in Bernstein
/// form (product-of-Bernstein-forms coefficients). Their global extrema are
/// found exactly via the cubic's (quadratic) derivative roots — not a loose
/// coefficient-hull bound, which round 5's investigation showed is too weak
/// to certify near a real high-curvature feature. `sin θ(t) ≤
/// max|cross(t)| / (min|B′|·min|Q̃′|)`, with `min|B′| ≥ min|Q̃′| −
/// max|B′ − Q̃′|` (a plain Bernstein coefficient-hull bound suffices for the
/// *speed* floor, which only needs to stay positive, not be tight). The
/// `dot` minimum must also stay positive — forcing `θ < 90°`, so `asin` is
/// the correct branch.
fn general_sin_bound(d0: DVec2, d1: DVec2, d2: DVec2, te0: DVec2, te1: DVec2) -> f64 {
    let min_emitted_speed = min_segment_norm(te0, te1);
    if min_emitted_speed == 0.0 {
        return f64::INFINITY;
    }
    // |B′(t) − Q̃′(t)|, Q̃′ degree-elevated to quadratic Bernstein form
    // `(te0, (te0+te1)/2, te1)`: a valid (if not maximally tight) hull bound.
    let mid_te = scale(DVec2::new(te0.x + te1.x, te0.y + te1.y), 0.5);
    let g0 = sub(d0, te0);
    let g1 = sub(d1, mid_te);
    let g2 = sub(d2, te1);
    let min_cubic_speed = min_emitted_speed - norm(g0).max(norm(g1)).max(norm(g2));
    if min_cubic_speed <= 0.0 {
        return f64::INFINITY;
    }
    let dot_k = (
        dot(d0, te0),
        (2.0 * dot(d1, te0) + dot(d0, te1)) / 3.0,
        (dot(d2, te0) + 2.0 * dot(d1, te1)) / 3.0,
        dot(d2, te1),
    );
    if cubic_bernstein_extrema(dot_k.0, dot_k.1, dot_k.2, dot_k.3).0 <= 0.0 {
        return f64::INFINITY;
    }
    let cross_k = (
        cross(d0, te0),
        (2.0 * cross(d1, te0) + cross(d0, te1)) / 3.0,
        (cross(d2, te0) + 2.0 * cross(d1, te1)) / 3.0,
        cross(d2, te1),
    );
    let cross_max = cubic_bernstein_abs_max(cross_k.0, cross_k.1, cross_k.2, cross_k.3);
    asin_or_infinity(cross_max / (min_cubic_speed * min_emitted_speed))
}

/// The quadratic control point whose tangent *lines* exactly match the
/// cubic's entry and exit tangent directions: the intersection of the ray
/// from `p0` along `c1 − p0` with the ray from `p3` along `c2 − p3`. `None`
/// when either direction is zero (an endpoint-stationary piece — the caller
/// never calls this then) or the two tangent lines are nearly parallel (the
/// intersection point runs off to a distant, numerically unstable location
/// that amplifies rather than reduces error; the caller falls back to the
/// averaged or chord-midpoint candidate instead).
///
/// This is what resolves Codex round-5: the averaged degree-reduction
/// control only *approximately* matches the endpoint tangents, and for a
/// piece with strong local curvature that approximation error alone can
/// exceed 1° well before `f32` rounding of the (tiny) piece becomes the
/// limiting factor. Matching both endpoint tangents exactly (zero angular
/// error at `u = 0` and `u = 1` by construction) leaves only the interior
/// curvature mismatch to bound, which is generally far smaller.
fn tangent_intersection_control(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> Option<DVec2> {
    let d0 = sub(c1, p0);
    let d1 = sub(c2, p3);
    if d0.is_zero() || d1.is_zero() {
        return None;
    }
    let det = cross(d1, d0);
    if det.abs() <= 1e-9 * norm(d0) * norm(d1) {
        return None;
    }
    let s = cross(d1, sub(p3, p0)) / det;
    Some(DVec2::new(p0.x + s * d0.x, p0.y + s * d0.y))
}

/// Evaluate a scalar cubic in Bernstein form `(k0, k1, k2, k3)` at `u`.
fn cubic_bernstein_eval(k0: f64, k1: f64, k2: f64, k3: f64, u: f64) -> f64 {
    let mu = 1.0 - u;
    mu * mu * mu * k0 + 3.0 * mu * mu * u * k1 + 3.0 * mu * u * u * k2 + u * u * u * k3
}

/// Exact global extrema `(min, max)` of a scalar cubic in Bernstein form over
/// `u ∈ [0, 1]`. The derivative is `3×` a quadratic Bernstein polynomial with
/// control points `(k1−k0, k2−k1, k3−k2)`; its roots — a plain quadratic
/// formula — are the only possible *interior* extrema, checked alongside the
/// two endpoints.
fn cubic_bernstein_extrema(k0: f64, k1: f64, k2: f64, k3: f64) -> (f64, f64) {
    let (mut lo, mut hi) = (k0.min(k3), k0.max(k3));
    let mut consider = |u: f64| {
        if u > 0.0 && u < 1.0 {
            let v = cubic_bernstein_eval(k0, k1, k2, k3, u);
            lo = lo.min(v);
            hi = hi.max(v);
        }
    };
    let (a0, a1, a2) = (k1 - k0, k2 - k1, k3 - k2);
    let a = a0 - 2.0 * a1 + a2;
    let b = 2.0 * (a1 - a0);
    let c = a0;
    if a == 0.0 {
        if b != 0.0 {
            consider(-c / b);
        }
    } else {
        let disc = b * b - 4.0 * a * c;
        if disc >= 0.0 {
            let sq = disc.sqrt();
            consider((-b + sq) / (2.0 * a));
            consider((-b - sq) / (2.0 * a));
        }
    }
    (lo, hi)
}

/// Maximum of `|p(u)|` over `[0, 1]` (see [`cubic_bernstein_extrema`]).
fn cubic_bernstein_abs_max(k0: f64, k1: f64, k2: f64, k3: f64) -> f64 {
    let (lo, hi) = cubic_bernstein_extrema(k0, k1, k2, k3);
    lo.abs().max(hi.abs())
}

/// Exact global extrema of the scalar quadratic in Bernstein form
/// `p(u) = (1−u)²·k0 + 2u(1−u)·kmid + u²·k2`: the extreme values sit at the
/// endpoints or the single interior vertex.
fn quadratic_bernstein_extrema(k0: f64, kmid: f64, k2: f64) -> (f64, f64) {
    let (mut lo, mut hi) = (k0.min(k2), k0.max(k2));
    // p′(u) = 2·[(kmid − k0) + (k0 − 2·kmid + k2)·u]
    let a = k0 - 2.0 * kmid + k2;
    if a != 0.0 {
        let u = (k0 - kmid) / a;
        if u > 0.0 && u < 1.0 {
            let mu = 1.0 - u;
            let v = mu * mu * k0 + 2.0 * u * mu * kmid + u * u * k2;
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo, hi)
}

fn quadratic_bernstein_abs_max(k0: f64, kmid: f64, k2: f64) -> f64 {
    let (lo, hi) = quadratic_bernstein_extrema(k0, kmid, k2);
    lo.abs().max(hi.abs())
}

/// Certified bound (radians) on `max_u angle(ψ(u), Q̃′(u))` for a piece whose
/// true derivative `B′(u)` factors as a **linear** direction carrier `ψ`
/// times a vanishing scalar (an endpoint-stationary piece — see module
/// docs): `ψ(u) = (1−u)·psi0 + u·psi1` against the emitted quadratic's linear
/// derivative `(1−u)·te0 + u·te1`. `INFINITY` when certification degenerates.
///
/// This exists *in addition to* [`general_sin_bound`] because that generic
/// formula's denominator floor, `min|Q̃′| − max|B′ − Q̃′|`, collapses whenever
/// `B′` is genuinely zero at an endpoint (`d0` or `d2` `= (0, 0)`): the hull
/// term there is as large as `Q̃′` itself, since there is nothing on the `B′`
/// side to partially cancel it, so the bound can never certify — regardless
/// of how well the piece actually converges. Dividing out the vanishing
/// scalar factor first (leaving the well-behaved linear `ψ`) avoids that
/// artifact entirely: `ψ × Q̃′` and `ψ · Q̃′` are quadratics in `u`, with exact
/// extrema via [`quadratic_bernstein_extrema`], and the dot's positivity
/// forces the angle below 90° so `asin` is valid.
fn psi_error_bound(psi0: DVec2, psi1: DVec2, te0: DVec2, te1: DVec2) -> f64 {
    let min_emitted_speed = min_segment_norm(te0, te1);
    if min_emitted_speed == 0.0 {
        return f64::INFINITY;
    }
    // A one-sided-zero ψ endpoint (e.g. p0 == c1 == c2: the derivative is
    // 3u²·(p3 − c2), direction constant) collapses to a constant carrier.
    let (psi0, psi1) = match (psi0.is_zero(), psi1.is_zero()) {
        (true, true) => return f64::INFINITY,
        (true, false) => (psi1, psi1),
        (false, true) => (psi0, psi0),
        (false, false) => (psi0, psi1),
    };
    let min_psi = min_segment_norm(psi0, psi1);
    if min_psi == 0.0 {
        return f64::INFINITY;
    }
    let (dot_min, _) = quadratic_bernstein_extrema(
        dot(psi0, te0),
        0.5 * (dot(psi0, te1) + dot(psi1, te0)),
        dot(psi1, te1),
    );
    if dot_min <= 0.0 {
        return f64::INFINITY;
    }
    let cross_max = quadratic_bernstein_abs_max(
        cross(psi0, te0),
        0.5 * (cross(psi0, te1) + cross(psi1, te0)),
        cross(psi1, te1),
    );
    asin_or_infinity(cross_max / (min_psi * min_emitted_speed))
}

/// Certified bound (radians) for an endpoint-stationary piece's chord-midpoint
/// candidate (see module docs and [`psi_error_bound`]): factors out the
/// vanishing derivative scalar at the stationary endpoint(s) and bounds the
/// remaining linear direction carrier `ψ`.
#[allow(clippy::too_many_arguments)]
fn stationary_sin_bound(
    c1: DVec2,
    p0: DVec2,
    c2: DVec2,
    p3: DVec2,
    start_stationary: bool,
    end_stationary: bool,
    te0: DVec2,
    te1: DVec2,
) -> f64 {
    match (start_stationary, end_stationary) {
        (true, false) => {
            // B′(u) = u·ψ(u), ψ(u) = 6(1−u)(c2 − c1) + 3u(p3 − c2).
            psi_error_bound(scale(sub(c2, c1), 6.0), scale(sub(p3, c2), 3.0), te0, te1)
        }
        (false, true) => {
            // B′(u) = (1−u)·ψ(u), ψ(u) = 3(1−u)(c1 − p0) + 6u(c2 − c1).
            psi_error_bound(scale(sub(c1, p0), 3.0), scale(sub(c2, c1), 6.0), te0, te1)
        }
        (true, true) => {
            // B′(u) = 6(1−u)u·(c2 − c1): constant direction (a straight,
            // doubly-stationary parameterisation). Zero direction means a
            // point piece, uncertifiable.
            let dir = sub(c2, c1);
            if dir.is_zero() {
                f64::INFINITY
            } else {
                psi_error_bound(dir, dir, te0, te1)
            }
        }
        (false, false) => {
            unreachable!("stationary_sin_bound requires at least one stationary endpoint")
        }
    }
}

fn asin_or_infinity(sin_bound: f64) -> f64 {
    if sin_bound >= 1.0 {
        f64::INFINITY
    } else {
        sin_bound.asin()
    }
}

/// Minimum norm of the linear interpolation `(1 − t)·e0 + t·e1` over
/// `t ∈ [0, 1]`, computed exactly: `|·|²` is quadratic in `t`, so the minimum
/// is at the clamped vertex.
fn min_segment_norm(e0: DVec2, e1: DVec2) -> f64 {
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
/// the cubic: `Q = (3·c1 − p0 + 3·c2 − p3) / 4`. (For a doubly-stationary
/// straight cubic this lands exactly on the chord midpoint.)
fn quadratic_control(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> DVec2 {
    DVec2::new(
        (3.0 * c1.x - p0.x + 3.0 * c2.x - p3.x) / 4.0,
        (3.0 * c1.y - p0.y + 3.0 * c2.y - p3.y) / 4.0,
    )
}

/// A cubic's four control points `(p0, c1, c2, p3)`.
type CubicPoints = (DVec2, DVec2, DVec2, DVec2);

/// de Casteljau split of a cubic at `t = 0.5` into its left and right sub-cubics.
fn split_at_half(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> (CubicPoints, CubicPoints) {
    let m01 = midpoint(p0, c1);
    let m12 = midpoint(c1, c2);
    let m23 = midpoint(c2, p3);
    let m012 = midpoint(m01, m12);
    let m123 = midpoint(m12, m23);
    let mid = midpoint(m012, m123);
    ((p0, m01, m012, mid), (mid, m123, m23, p3))
}

fn midpoint(a: DVec2, b: DVec2) -> DVec2 {
    DVec2::new((a.x + b.x) * 0.5, (a.y + b.y) * 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(v: Vec2) -> DVec2 {
        DVec2::from_vec2(v)
    }

    /// Tangent (derivative direction) of a cubic Bézier at `t`, in f64.
    fn cubic_tangent(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2, t: f64) -> DVec2 {
        let mt = 1.0 - t;
        DVec2::new(
            3.0 * mt * mt * (c1.x - p0.x)
                + 6.0 * mt * t * (c2.x - c1.x)
                + 3.0 * t * t * (p3.x - c2.x),
            3.0 * mt * mt * (c1.y - p0.y)
                + 6.0 * mt * t * (c2.y - c1.y)
                + 3.0 * t * t * (p3.y - c2.y),
        )
    }

    /// Tangent (derivative direction) of a quadratic Bézier at `t`, in f64.
    fn quad_tangent(p0: DVec2, q: DVec2, p2: DVec2, t: f64) -> DVec2 {
        let mt = 1.0 - t;
        DVec2::new(
            2.0 * mt * (q.x - p0.x) + 2.0 * t * (p2.x - q.x),
            2.0 * mt * (q.y - p0.y) + 2.0 * t * (p2.y - q.y),
        )
    }

    /// Unsigned angle (radians) between two direction vectors; `0` if either is
    /// (near-)zero-length so a degenerate tangent never inflates the error.
    fn angle_between(a: DVec2, b: DVec2) -> f64 {
        let cross = a.x * b.y - a.y * b.x;
        let dot = a.x * b.x + a.y * b.y;
        if cross == 0.0 && dot == 0.0 {
            0.0
        } else {
            cross.abs().atan2(dot)
        }
    }

    /// Worst tangent-direction error (radians) of the **actual emitted f32
    /// chain** against the source cubic — the production-visible contract —
    /// dense-sampling `samples_per_piece` uniform `t` values on every emitted
    /// piece via the exact `[t0, t1]` correspondence. Quad tangents are
    /// computed from the emitted f32 control/end values (widened to f64 for
    /// the arithmetic only); cubic tangents from the f32 source coordinates.
    fn worst_emitted_chain_error(
        p0: Vec2,
        c1: Vec2,
        c2: Vec2,
        p3: Vec2,
        samples_per_piece: u32,
    ) -> f64 {
        let pieces = subdivide_pieces(p0, c1, c2, p3).expect("subdivision must succeed");
        assert!(!pieces.is_empty());
        let (sp0, sc1, sc2, sp3) = (d(p0), d(c1), d(c2), d(p3));
        let mut start = d(p0);
        let mut worst = 0.0_f64;
        for piece in &pieces {
            for k in 0..=samples_per_piece {
                let u = f64::from(k) / f64::from(samples_per_piece);
                let t = piece.t0 + (piece.t1 - piece.t0) * u;
                let ct = cubic_tangent(sp0, sc1, sc2, sp3, t);
                let qt = quad_tangent(start, d(piece.control), d(piece.end), u);
                worst = worst.max(angle_between(ct, qt));
            }
            start = d(piece.end);
        }
        worst
    }

    /// Deepest subdivision level of a successful chain (spans are exact
    /// dyadics, so the level is `−log2(t1 − t0)`).
    fn max_depth(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> u32 {
        let pieces = subdivide_pieces(p0, c1, c2, p3).expect("subdivision must succeed");
        pieces
            .iter()
            .map(|p| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let depth = (-(p.t1 - p.t0).log2()).round() as u32;
                depth
            })
            .max()
            .unwrap()
    }

    /// Reconstruct the quad chain as (start, control, end) triples.
    fn quad_chain(p0: Vec2, c1: Vec2, c2: Vec2, p3: Vec2) -> Vec<(Vec2, Vec2, Vec2)> {
        let mut start = p0;
        subdivide_cubic(p0, c1, c2, p3)
            .expect("subdivision must succeed")
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
    /// 1° angular error of the source cubic, measured on the emitted f32
    /// output. Subdivision is *adaptive*, so each piece reports the
    /// source-cubic parameter interval `[t0, t1]` it covers; we compare the
    /// quad's tangent at local `u` against the source cubic's tangent at the
    /// corresponding global `t`. Correspondence is monotone within a piece, so
    /// — unlike nearest-point matching — a self-approaching S or wiggle cannot
    /// fold one branch onto another.
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
        let one_degree = 1.0_f64.to_radians();
        for &(p0, c1, c2, p3) in &cubics {
            let worst = worst_emitted_chain_error(p0, c1, c2, p3, 24);
            assert!(
                worst <= one_degree + 1e-3,
                "worst angular error {} rad exceeds 1°",
                worst
            );
        }
    }

    /// Regression for the Codex round-1 finding: the fixed-sample criterion let
    /// this near-cusp cubic emit a piece for `[0.375, 0.5]` with ~43.7° of
    /// tangent error near `t = 0.494375`. Subdivision must succeed and the
    /// dense-checked **emitted f32** chain must stay inside the advertised 1°
    /// contract.
    #[test]
    fn codex_r1_pathological_cubic_stays_within_bound() {
        let (p0, c1, c2, p3) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(9.371965, 0.099994),
            Vec2::new(8.02181, 0.048572),
            Vec2::new(1.0, 0.0),
        );
        let depth = max_depth(p0, c1, c2, p3);
        println!("codex r1 loop cubic: max subdivision depth {depth}");
        assert!(depth < MAX_SUBDIVISION_DEPTH);
        let worst = worst_emitted_chain_error(p0, c1, c2, p3, 400);
        assert!(
            worst <= 1.0_f64.to_radians() + 1e-3,
            "worst angular error {} rad ({}°) exceeds 1°",
            worst,
            worst.to_degrees()
        );
    }

    /// Regression for the Codex round-2/3/4 reproducer: a finite non-cusp
    /// cubic whose pseudo-cusp needs pieces below f32 resolution — every
    /// earlier round emitted some uncertified f32 chain for it (~90°, ~89.94°,
    /// ~75.31° measured across rounds). The honest outcomes are exactly two:
    /// either a chain whose **emitted f32** pieces all pass 1°, or
    /// `Err(PrecisionUnderflow)`. Both are asserted; an uncertified emission
    /// or a `DegenerateInput` misclassification fails the test.
    #[test]
    fn codex_r2_f32_collapsed_cubic_does_not_emit_uncertified_piece() {
        let p0 = Vec2::new(0.0, 0.0);
        let c1 = Vec2::new(1.0 / 12.0, 1e-6 / 3.0);
        let c2 = Vec2::new(0.0, 2e-6 / 3.0);
        let p3 = Vec2::new(1.0 / 12.0, 1e-6);
        match subdivide_cubic(p0, c1, c2, p3) {
            Ok(_) => {
                println!("codex r2 collapsed cubic: emitted a certified chain");
                let worst = worst_emitted_chain_error(p0, c1, c2, p3, 400);
                assert!(
                    worst <= 1.0_f64.to_radians() + 1e-3,
                    "emitted chain claims success but shows {}° tangent error",
                    worst.to_degrees()
                );
            }
            Err(e) => {
                println!("codex r2 collapsed cubic: {e:?}");
                assert_eq!(
                    e,
                    SubdivisionError::PrecisionUnderflow,
                    "an f32-resolution failure must be PrecisionUnderflow, not {e:?}"
                );
            }
        }
    }

    /// Regression for the Codex round-4 finding: an ordinary start-stationary
    /// cubic (`c1 == p0`, a smooth arch) must not panic and must not error —
    /// its tangent at the stationary endpoint is the well-defined direction
    /// toward the next control point. The emitted f32 chain passes 1°.
    #[test]
    fn codex_r4_start_stationary_cubic_subdivides_within_bound() {
        let (p0, c1, c2, p3) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(50.0, 100.0),
            Vec2::new(100.0, 0.0),
        );
        let worst = worst_emitted_chain_error(p0, c1, c2, p3, 400);
        assert!(
            worst <= 1.0_f64.to_radians() + 1e-3,
            "worst angular error {} rad ({}°) exceeds 1°",
            worst,
            worst.to_degrees()
        );
    }

    /// Regression for the Codex round-5 finding: the (now-removed) noise-floor
    /// pre-check heuristic over-rejected this ordinary, well-resolved cubic —
    /// coordinates near 1000, an unremarkable scale — declaring
    /// `PrecisionUnderflow` even though recursive dyadic subdivision produces
    /// a certified 11-piece `f32` chain (worst error ~0.958°). Subdivision
    /// must now succeed, and the emitted f32 chain must pass 1° under dense
    /// sampling.
    #[test]
    fn codex_r5_normal_scale_cubic_certifies_not_underflow() {
        let (p0, c1, c2, p3) = (
            Vec2::new(999.65, 1000.0),
            Vec2::new(999.4, 999.1),
            Vec2::new(999.95, 999.65),
            Vec2::new(999.25, 1000.55),
        );
        let worst = worst_emitted_chain_error(p0, c1, c2, p3, 200);
        assert!(
            worst <= 1.0_f64.to_radians() + 1e-3,
            "worst angular error {} rad ({}°) exceeds 1°",
            worst,
            worst.to_degrees()
        );
    }

    /// Sibling of the round-4 case: end-stationary (`c2 == p3`). Same
    /// acceptance — no panic, no error, emitted f32 chain within 1°.
    #[test]
    fn end_stationary_cubic_subdivides_within_bound() {
        let (p0, c1, c2, p3) = (
            Vec2::new(0.0, 0.0),
            Vec2::new(50.0, 100.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(100.0, 0.0),
        );
        let worst = worst_emitted_chain_error(p0, c1, c2, p3, 400);
        assert!(
            worst <= 1.0_f64.to_radians() + 1e-3,
            "worst angular error {} rad ({}°) exceeds 1°",
            worst,
            worst.to_degrees()
        );
    }

    /// A doubly-stationary cubic (`c1 == p0`, `c2 == p3`, distinct endpoints)
    /// is a straight segment with degenerate parameterisation — valid input,
    /// emitted as a certified chain.
    #[test]
    fn doubly_stationary_straight_cubic_subdivides() {
        let worst = worst_emitted_chain_error(
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, 0.0),
            Vec2::new(100.0, 100.0),
            Vec2::new(100.0, 100.0),
            100,
        );
        assert!(worst <= 1.0_f64.to_radians() + 1e-3);
    }

    /// Fully-degenerate input (all four control points equal) has no tangent
    /// direction anywhere: typed rejection, not a panic and not an emission.
    #[test]
    fn fully_degenerate_cubic_is_rejected() {
        let p = Vec2::new(7.0, 7.0);
        assert_eq!(
            subdivide_cubic(p, p, p, p),
            Err(SubdivisionError::DegenerateInput)
        );
    }

    /// Property test: 50 deterministic pseudo-random well-scaled cubics, swept
    /// across scales 1, 1e2, 1e4 — subdivision succeeds and every emitted f32
    /// chain passes the 1° bound under dense sampling.
    #[test]
    fn random_well_scaled_cubics_emit_certified_chains() {
        let mut state = 0x137_u64;
        let mut next_coord = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            #[allow(clippy::cast_precision_loss)]
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
            (unit * 200.0 - 100.0) as f32
        };
        let mut bases = Vec::new();
        for _ in 0..50 {
            bases.push((
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
            ));
        }
        let one_degree = 1.0_f64.to_radians();
        for &(p0, c1, c2, p3) in &bases {
            for &s in &[1.0_f32, 1e2, 1e4] {
                let sc = |p: Vec2| Vec2::new(p.x * s, p.y * s);
                let worst = worst_emitted_chain_error(sc(p0), sc(c1), sc(c2), sc(p3), 40);
                assert!(
                    worst <= one_degree + 1e-3,
                    "cubic {:?} scale {} worst angular error {} rad ({}°) exceeds 1°",
                    (p0, c1, c2, p3),
                    s,
                    worst,
                    worst.to_degrees()
                );
            }
        }
    }

    /// Regression for the Codex round-5 completeness finding: at ordinary
    /// scales (1e0 to 1e4, where the round-5 reproducer itself sits), no
    /// well-scaled cubic should ever hit `PrecisionUnderflow` — that error is
    /// reserved for genuine `f32`-resolution limits (round-2's pseudo-cusp),
    /// not routine subdivision. 100 deterministic pseudo-random cubics swept
    /// across four scales must all subdivide successfully with an emitted f32
    /// chain inside the 1° bound.
    #[test]
    fn ordinary_scale_cubics_never_underflow() {
        let mut state = 0x1379_u64;
        let mut next_coord = move || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            #[allow(clippy::cast_precision_loss)]
            let unit = (state >> 11) as f64 / (1u64 << 53) as f64;
            (unit * 2.0 - 1.0) as f32
        };
        let mut bases = Vec::new();
        for _ in 0..100 {
            bases.push((
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
                Vec2::new(next_coord(), next_coord()),
            ));
        }
        let one_degree = 1.0_f64.to_radians();
        for &(p0, c1, c2, p3) in &bases {
            for &s in &[1.0_f32, 1e1, 1e2, 1e4] {
                let sc = |p: Vec2| Vec2::new(p.x * s, p.y * s);
                let (sp0, sc1, sc2, sp3) = (sc(p0), sc(c1), sc(c2), sc(p3));
                match subdivide_pieces(sp0, sc1, sc2, sp3) {
                    Ok(_) => {
                        let worst = worst_emitted_chain_error(sp0, sc1, sc2, sp3, 40);
                        assert!(
                            worst <= one_degree + 1e-3,
                            "cubic {:?} scale {} worst angular error {} rad ({}°) exceeds 1°",
                            (p0, c1, c2, p3),
                            s,
                            worst,
                            worst.to_degrees()
                        );
                    }
                    Err(SubdivisionError::DegenerateInput) => {
                        // Astronomically unlikely with this generator, but not
                        // a violation of this test's claim (no direction to
                        // certify in the first place).
                    }
                    Err(e @ SubdivisionError::PrecisionUnderflow) => {
                        panic!(
                            "ordinary-scale cubic {:?} at scale {} spuriously hit {:?}",
                            (p0, c1, c2, p3),
                            s,
                            e
                        );
                    }
                }
            }
        }
    }

    /// Dense sampling of the emitted f32 chains on hard cubics — near-cusp
    /// loops, sharp inflections, retrogrades, tight loops, uneven
    /// parameterisations, at several scales: the 1° contract holds on the
    /// production-visible output everywhere subdivision reports success.
    #[test]
    fn dense_sampling_confirms_one_degree_bound_on_hard_cubics() {
        let base_cubics = [
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
        ];
        let one_degree = 1.0_f64.to_radians();
        for &(p0, c1, c2, p3) in &base_cubics {
            for &s in &[1e-2_f32, 1.0, 1e3] {
                let sc = |p: Vec2| Vec2::new(p.x * s, p.y * s);
                let worst = worst_emitted_chain_error(sc(p0), sc(c1), sc(c2), sc(p3), 200);
                assert!(
                    worst <= one_degree + 1e-3,
                    "cubic {:?} scale {} worst angular error {} rad ({}°) exceeds 1°",
                    (p0, c1, c2, p3),
                    s,
                    worst,
                    worst.to_degrees()
                );
            }
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
        let quads = subdivide_cubic(p0, c1, c2, p2).expect("subdivision must succeed");
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
        )
        .expect("subdivision must succeed");
        assert_eq!(quads.len(), 1);
    }
}
