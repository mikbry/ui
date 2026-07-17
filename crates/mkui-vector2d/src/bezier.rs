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
//! cubic's derivative `B′(t)` is a quadratic Bézier, the emitted quadratic's
//! derivative is linear, and every factor of `sin θ(t) = |B′ × Q′|/(|B′|·|Q′|)`
//! can be bounded in closed form (cross products of linear vector functions are
//! polynomials with computable Bernstein coefficient hulls; minimum speeds have
//! closed-form segment minima). Subdivision runs entirely in `f64`
//! ([`DVec2`]) — Codex round-2 showed `f32` de Casteljau collapses control
//! deltas into rounding noise before certification can succeed.
//!
//! # Certification targets the *emitted* `f32` quadratic
//!
//! Production emits `f32` control points, and Codex round-4 proved that
//! certifying the pre-rounding `f64` quadratic is not enough: rounding the
//! round-2 reproducer's certified chain to `f32` reintroduced ~75° of tangent
//! error. The criterion therefore measures the exact quadratic the caller will
//! receive: each candidate piece's start/control/end are rounded to `f32`
//! first, and the analytic bound is evaluated **against that rounded
//! quadratic**, with the rounding perturbation (a linear-in-`t` derivative
//! term, computed from the actual rounded values, not a worst-case ULP model)
//! folded into both the numerator and the speed lower bounds. A piece is
//! emitted only when the rounded quadratic itself certifies.
//!
//! Because coordinate ULPs are fixed by `f32` representation while piece
//! extents shrink under subdivision, splitting *cannot* rescue a piece whose
//! quantization jitter alone exceeds the tolerance — deeper pieces are
//! strictly noisier. When one ULP of perpendicular-axis rounding, measured
//! against the piece's own hull extent, already out-jitters the tolerance
//! (see `f32_noise_floor_sin`), recursion stops and the whole call returns
//! [`SubdivisionError::PrecisionUnderflow`]: the input needs pieces finer than
//! `f32` can represent (curvature features at the `f32` resolution limit),
//! and emitting an uncertified chain or grinding to the depth cap would be
//! dishonest and wasteful respectively. This is exactly the fate of the Codex
//! round-2 reproducer — its pseudo-cusp needs pieces whose `f32`
//! x-coordinates quantize to garbage directions — and why [`subdivide_cubic`]
//! returns a `Result` rather than pretending every finite cubic has a
//! certified `f32` chain. The floor deliberately measures quantization
//! against piece extent rather than the current candidate quadratic's speed:
//! a *geometrically* degenerate candidate (e.g. a retrograde averaged control
//! on a strongly uneven parameterisation) also has tiny speeds, but splitting
//! fixes geometry, so it must split, not bail.
//!
//! # Endpoint-stationary cubics are valid input
//!
//! A cubic with `c1 == p0` (or `c2 == p3`) has `B′ = 0` at that endpoint —
//! common in real paths and *not* a tangent-flip cusp (Codex round-4's
//! counterexample `(0,0), (0,0), (50,100), (100,0)` is a perfectly smooth
//! arch). The generic bound cannot certify such pieces (its speed lower bound
//! degenerates), but the derivative factors exactly: with `c1 == p0`,
//! `B′(u) = u·ψ(u)` where `ψ(u) = 6(1−u)(c2−c1) + 3u(p3−c2)` is **linear**,
//! so for `u > 0` the tangent direction is ψ's direction and the limit tangent
//! at the stationary endpoint is `ψ(0)` — well-defined geometry, the direction
//! toward the next distinct control point. The certifier dispatches on exact
//! endpoint-stationarity and bounds `angle(ψ, Q̃′)` (linear vs linear: exact
//! extrema of the quadratic cross and dot polynomials) instead of
//! `angle(B′, Q̃′)`. Stationary pieces also emit the **chord midpoint** as
//! their control rather than the averaged degree-reduction control: on the
//! stationary spine `p3 → 3·c2`, so the averaged `q = (3·c2 − p3)/4` cancels
//! catastrophically and its direction — the emitted initial tangent — becomes
//! numerical noise, while the pieces themselves are asymptotically straight
//! and the chord certifies. Mirrored for `c2 == p3`; a doubly-stationary
//! cubic (`c1 == p0` and `c2 == p3`) has constant derivative direction
//! `c2 − c1` and uses the same machinery with a constant ψ. Only a cubic with
//! **no** direction anywhere — all four control points equal, `B′ ≡ 0` — is
//! rejected as [`SubdivisionError::DegenerateInput`].
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

/// Backstop on recursion depth; reaching it returns
/// [`SubdivisionError::PrecisionUnderflow`] — it is **not** an emission path
/// and does not panic. In `f64` the bound shrinks ~4× per level wherever the
/// derivative direction is resolvable, so real inputs finish far below this
/// cap. Observed on the Codex reproducers (measured by this module's
/// regression tests): the round-1 near-cusp loop certifies every emitted
/// piece by depth 7; the round-2 `f32`-collapsing cubic exits early with
/// `PrecisionUnderflow` via the quantization-noise floor rather than
/// grinding to any depth. The cap only backstops inputs that evade the noise
/// floor, e.g. interior true cusps, and bounds the work done before the typed
/// error is returned.
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
    // Emission control: the averaged degree-reduction control for generic
    // pieces. For endpoint-stationary pieces the averaged formula is
    // numerically catastrophic — on the stationary spine `p3 → 3·c2`, so
    // `q = (3·c2 − p3)/4` cancels to noise and its direction (the emitted
    // initial tangent) becomes garbage. Those pieces are asymptotically
    // straight (their tangent spread shrinks with the piece), so the chord
    // midpoint is the correct control: constant chord direction, certified by
    // the ψ bound once the piece's tangent spread is inside tolerance.
    let start_stationary = sub(c1, p0).is_zero();
    let end_stationary = sub(p3, c2).is_zero();
    let q = if start_stationary || end_stationary {
        midpoint(p0, p3)
    } else {
        quadratic_control(p0, c1, c2, p3)
    };
    // The exact f32 quadratic production would emit for this piece: its start
    // is the previous piece's rounded end (bit-identical to rounding p0, since
    // both round the same shared f64 split point).
    let eq0 = p0.f32_rounded();
    let eqc = q.f32_rounded();
    let eq1 = p3.f32_rounded();
    if emitted_error_bound(p0, c1, c2, p3, eq0, eqc, eq1) <= tolerance_rad {
        out.push(Piece {
            control: q.to_vec2(),
            end: p3.to_vec2(),
            t0,
            t1,
        });
        return Ok(());
    }
    // Rounding-noise floor: if even the ideal averaged quadratic, compared
    // against its own f32 rounding, already exceeds the tolerance, then f32
    // cannot represent any quadratic this piece needs — and children are
    // strictly worse (their speeds halve while rounding jitter is fixed by the
    // coordinate magnitudes). Fail fast with a typed error instead of
    // splitting toward the depth cap.
    // Precision floor: if f32 coordinate quantization jitters this piece's
    // tangent directions by more than the tolerance, no emitted quadratic —
    // for this piece or any descendant, whose extents only shrink while
    // coordinate ULPs stay fixed — can be certified. Fail fast with a typed
    // error instead of splitting toward the depth cap. (This deliberately
    // measures quantization against the piece's own extent, not the current
    // quadratic's speed: a *geometrically* degenerate candidate — e.g. a
    // retrograde averaged control on an uneven parameterisation — has tiny
    // speed too, but splitting fixes geometry, so it must split, not bail.)
    if f32_noise_floor_sin(p0, c1, c2, p3) > tolerance_rad.sin() {
        return Err(SubdivisionError::PrecisionUnderflow);
    }
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

/// Certified upper bound (radians) on the tangent-direction error between the
/// `f64` cubic and the **actual emitted `f32` quadratic** `(eq0, eqc, eq1)`
/// over all `u ∈ [0, 1]`; `INFINITY` when no finite certification holds at
/// this piece's scale (the recursion then subdivides or fails fast).
///
/// Dispatches on exact endpoint-stationarity (see module docs):
///
/// - Generic pieces (`c1 ≠ p0`, `c2 ≠ p3`) use the hodograph decomposition
///   `B′ − Q̃′ = φ(u)·v + L(u)` where `v = 3(c2 − c1) + p0 − p3` (constant,
///   `max|φ| = ½`) and `L = Q′ − Q̃′` is the *actual* linear rounding
///   perturbation. Then `sin θ ≤ [½·max|v × Q̃′| + max|L × Q̃′|] /
///   ((min|Q̃′| − ½|v| − max|L|)·min|Q̃′|)`, with each `max` over a linear or
///   quadratic polynomial bounded by its Bernstein coefficient hull and each
///   `min` a closed-form segment minimum. The denominator guard also forces
///   `B′·Q̃′ > 0`, so `θ < 90°` and `asin` is valid.
/// - Endpoint-stationary pieces factor the derivative, `B′(u) = u·ψ(u)` (or
///   `(1−u)·ψ(u)`), and bound `angle(ψ, Q̃′)` for the linear ψ via
///   `psi_error_bound` — the scalar factor cannot change direction.
fn emitted_error_bound(
    p0: DVec2,
    c1: DVec2,
    c2: DVec2,
    p3: DVec2,
    eq0: DVec2,
    eqc: DVec2,
    eq1: DVec2,
) -> f64 {
    let te0 = scale(sub(eqc, eq0), 2.0);
    let te1 = scale(sub(eq1, eqc), 2.0);
    let min_emitted_speed = min_segment_norm(te0, te1);
    if min_emitted_speed == 0.0 {
        return f64::INFINITY;
    }
    let start_stationary = sub(c1, p0).is_zero();
    let end_stationary = sub(p3, c2).is_zero();
    match (start_stationary, end_stationary) {
        (false, false) => {
            let v = DVec2::new(
                3.0 * (c2.x - c1.x) + p0.x - p3.x,
                3.0 * (c2.y - c1.y) + p0.y - p3.y,
            );
            let q = quadratic_control(p0, c1, c2, p3);
            let e0 = scale(sub(q, p0), 2.0);
            let e1 = scale(sub(p3, q), 2.0);
            let l0 = sub(e0, te0);
            let l1 = sub(e1, te1);
            let l_max = norm(l0).max(norm(l1));
            let min_cubic_speed = min_emitted_speed - 0.5 * norm(v) - l_max;
            if min_cubic_speed <= 0.0 {
                return f64::INFINITY;
            }
            let v_cross = 0.5 * cross(v, te0).abs().max(cross(v, te1).abs());
            let l_cross = quadratic_bernstein_abs_max(
                cross(l0, te0),
                0.5 * (cross(l0, te1) + cross(l1, te0)),
                cross(l1, te1),
            );
            let sin_bound = (v_cross + l_cross) / (min_cubic_speed * min_emitted_speed);
            asin_or_infinity(sin_bound)
        }
        (true, false) => {
            // B′(u) = u·ψ(u), ψ(u) = 6(1−u)(c2 − c1) + 3u(p3 − c2).
            let psi0 = scale(sub(c2, c1), 6.0);
            let psi1 = scale(sub(p3, c2), 3.0);
            psi_error_bound(psi0, psi1, te0, te1, min_emitted_speed)
        }
        (false, true) => {
            // B′(u) = (1−u)·ψ(u), ψ(u) = 3(1−u)(c1 − p0) + 6u(c2 − c1).
            let psi0 = scale(sub(c1, p0), 3.0);
            let psi1 = scale(sub(c2, c1), 6.0);
            psi_error_bound(psi0, psi1, te0, te1, min_emitted_speed)
        }
        (true, true) => {
            // B′(u) = 6(1−u)u·(c2 − c1): constant direction (a straight,
            // doubly-stationary parameterisation). Zero direction means a
            // point piece, uncertifiable.
            let dir = sub(c2, c1);
            if dir.is_zero() {
                return f64::INFINITY;
            }
            psi_error_bound(dir, dir, te0, te1, min_emitted_speed)
        }
    }
}

/// Certified bound (radians) on `max_u angle(ψ(u), Q̃′(u))` for the linear
/// direction carrier `ψ(u) = (1−u)·psi0 + u·psi1` against the emitted
/// quadratic's linear derivative `(1−u)·te0 + u·te1`; `INFINITY` when the
/// certification degenerates. `ψ × Q̃′` and `ψ·Q̃′` are quadratics in `u`; the
/// cross maximum uses the Bernstein coefficient hull and the dot's hull
/// positivity forces the angle below 90° so `asin` is valid.
///
/// A one-sided-zero ψ endpoint (e.g. `p0 == c1 == c2`: the derivative is
/// `3u²·(p3 − c2)`, direction constant) collapses to a constant carrier.
fn psi_error_bound(
    psi0: DVec2,
    psi1: DVec2,
    te0: DVec2,
    te1: DVec2,
    min_emitted_speed: f64,
) -> f64 {
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
    let dot_min = quadratic_bernstein_min(
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

/// Estimated sine of the tangent-direction jitter that `f32` coordinate
/// quantization imposes on this piece: one ULP of perpendicular-axis rounding
/// against the piece's own hull extent. Used as the fail-fast precision floor
/// — subdividing shrinks extents while coordinate ULPs stay fixed, so once
/// quantization alone out-jitters the tolerance, no descendant's emitted
/// quadratic can carry a certifiable direction. Deliberately independent of
/// the current candidate quadratic's speed (a geometrically degenerate
/// candidate must split, not bail; see the call site).
fn f32_noise_floor_sin(p0: DVec2, c1: DVec2, c2: DVec2, p3: DVec2) -> f64 {
    let xs = [p0.x, c1.x, c2.x, p3.x];
    let ys = [p0.y, c1.y, c2.y, p3.y];
    let spread = |v: &[f64; 4]| {
        v.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
            - v.iter().cloned().fold(f64::INFINITY, f64::min)
    };
    let (ex, ey) = (spread(&xs), spread(&ys));
    let den = ex * ex + ey * ey;
    if den == 0.0 {
        // A point piece: no direction at all.
        return f64::INFINITY;
    }
    // Per-axis quantization scale: one f32 ULP at the coordinate magnitude
    // (≈ |coord|·2⁻²³; exact-zero coordinates round exactly).
    let mag = |v: &[f64; 4]| v.iter().fold(0.0_f64, |m, c| m.max(c.abs()));
    let ux = mag(&xs) * (f64::from(f32::EPSILON) * 0.5);
    let uy = mag(&ys) * (f64::from(f32::EPSILON) * 0.5);
    // Tangent vectors live at the piece-extent scale (ex, ey); perpendicular
    // quantization jitter of one axis against the other axis's extent gives
    // the direction noise: sin θ ≈ |jitter × dir| / |dir|².
    (ux * ey).max(uy * ex) / den
}

/// Exact extrema of the scalar quadratic in Bernstein form
/// `p(u) = (1−u)²·k0 + 2u(1−u)·kmid + u²·k2` over `u ∈ [0, 1]`: the extreme
/// values sit at the endpoints or the single interior vertex. Exact evaluation
/// matters — the coefficient-hull bound is too loose here, because `kmid`
/// often carries a cancellation the polynomial itself performs (e.g. the
/// `ψ × Q̃′` cross of an endpoint-stationary piece, where the hull bound stops
/// shrinking under subdivision but the true maximum keeps converging).
fn quadratic_bernstein_eval(k0: f64, kmid: f64, k2: f64, u: f64) -> f64 {
    let mu = 1.0 - u;
    mu * mu * k0 + 2.0 * u * mu * kmid + u * u * k2
}

fn quadratic_bernstein_vertex(k0: f64, kmid: f64, k2: f64) -> Option<f64> {
    // p′(u) = 2·[(kmid − k0) + (k0 − 2·kmid + k2)·u]
    let a = k0 - 2.0 * kmid + k2;
    if a == 0.0 {
        return None;
    }
    let u = (k0 - kmid) / a;
    (u > 0.0 && u < 1.0).then_some(u)
}

/// Maximum of `|p(u)|` over `[0, 1]` (see [`quadratic_bernstein_eval`]).
fn quadratic_bernstein_abs_max(k0: f64, kmid: f64, k2: f64) -> f64 {
    let mut m = k0.abs().max(k2.abs());
    if let Some(u) = quadratic_bernstein_vertex(k0, kmid, k2) {
        m = m.max(quadratic_bernstein_eval(k0, kmid, k2, u).abs());
    }
    m
}

/// Minimum of `p(u)` over `[0, 1]` (see [`quadratic_bernstein_eval`]).
fn quadratic_bernstein_min(k0: f64, kmid: f64, k2: f64) -> f64 {
    let mut m = k0.min(k2);
    if let Some(u) = quadratic_bernstein_vertex(k0, kmid, k2) {
        m = m.min(quadratic_bernstein_eval(k0, kmid, k2, u));
    }
    m
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
