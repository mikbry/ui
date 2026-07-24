//! Backend-neutral stroke expansion: a stroked [`VectorPath`] → a fillable one.
//!
//! [`stroke_to_fill`] turns a centreline [`VectorPath`] plus a [`Stroke`]
//! descriptor into a **fillable** [`VectorPath`] (non-zero winding) that, once
//! filled, paints the stroke. The Sprint 8 §3.1 Wave 1 shape is deliberately
//! CPU-side and simple: curved segments are flattened to polylines, an optional
//! dash pattern splits contours by arc length, and each contour is expanded into
//! a set of **overlapping convex polygons** — a rectangle per segment, a filler
//! per corner join, and a piece per end cap.
//!
//! # Why overlapping convex pieces are seam-free here
//!
//! Every emitted sub-contour is wound the same way (counter-clockwise, positive
//! area). Under the non-zero winding rule a union of equally-wound overlapping
//! polygons has winding ≥ 1 everywhere inside and 0 outside — no holes at the
//! overlaps. The Slug coverage pipeline this feeds ([`crate::encode`]) computes
//! anti-aliased coverage from the *winding number* of all edges in a pixel's
//! band, not by compositing each piece separately, so interior seams between
//! abutting pieces cancel exactly and never show. That is what lets the stroker
//! stamp simple convex pieces instead of solving a single offset outline with
//! its self-intersection bookkeeping.
//!
//! # Coordinate space and units
//!
//! Expansion happens entirely in the path's own coordinate space. [`Stroke`]
//! widths are consumed as lengths in **that** space (nominally pixels — the
//! screen-space intent of a stroke — when the path is authored in pixels). The
//! GPU adapter (#138) applies the placement scale to the whole filled outline,
//! so a stroke scales with zoom exactly like its fill. No GPU type appears here.

use crate::bezier::subdivide_cubic;
use crate::path::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
use crate::stroke::{DashPattern, LineCap, LineJoin, Stroke};

/// Below this length a direction/segment is treated as degenerate and skipped.
const EPS: f32 = 1e-6;

/// SVG's default miter limit: a miter longer than `miter_limit * width/2` from
/// the vertex falls back to a bevel. [`Stroke`] carries no miter-limit field, so
/// the stroker fixes the conventional value (Sprint 8 keeps it non-configurable;
/// a per-stroke limit is a Sprint 9+ candidate).
const MITER_LIMIT: f32 = 4.0;

/// Target angular step for tessellating round caps and joins, in radians (~9°).
/// The arc silhouette is the only faceting source (coverage is analytic per
/// edge), so this stays smooth well past 4× zoom for typical stroke widths.
const ROUND_STEP_RAD: f32 = 0.15;

/// Line segments per flattened quadratic/cubic piece. Curved centrelines are
/// lowered to polylines before offsetting; this fixes the polyline resolution.
const CURVE_FLATTEN_STEPS: u32 = 24;

/// Expand a centreline `path` stroked with `stroke` into a fillable
/// [`VectorPath`] (non-zero winding). Returns an empty path (no drawable
/// commands) when the stroke paints nothing — a non-positive width, an empty
/// input, or an all-degenerate contour — so a downstream encoder rejects it as
/// empty rather than emitting a stray quad.
pub fn stroke_to_fill(path: &VectorPath, stroke: &Stroke) -> VectorPath {
    let half = stroke.width_px * 0.5;
    // `half <= EPS` is false for a NaN width, but `!is_finite()` catches that.
    if half <= EPS || !stroke.is_finite() {
        return empty_path();
    }

    let contours = flatten_contours(path);

    // Dashing splits every contour into a set of open "on" runs before
    // expansion; a solid stroke keeps each contour as-is.
    let runs: Vec<Contour> = match stroke.dash.as_ref() {
        Some(dash) if dash_is_active(dash) => {
            contours.iter().flat_map(|c| apply_dash(c, dash)).collect()
        }
        _ => contours,
    };

    let mut pieces: Vec<Vec<Vec2>> = Vec::new();
    for contour in &runs {
        expand_contour(contour, half, stroke.cap, stroke.join, &mut pieces);
    }

    build_path(pieces)
}

/// A flattened contour: a polyline plus whether it is closed (a ring).
#[derive(Debug, Clone)]
struct Contour {
    points: Vec<Vec2>,
    closed: bool,
}

fn empty_path() -> VectorPath {
    VectorPath::new(
        Vec::new(),
        FillRule::NonZero,
        Bounds::new(Vec2::ZERO, Vec2::ZERO),
    )
}

// ---- vector helpers (Vec2 carries no arithmetic ops of its own) -------------

fn add(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x + b.x, a.y + b.y)
}
fn sub(a: Vec2, b: Vec2) -> Vec2 {
    Vec2::new(a.x - b.x, a.y - b.y)
}
fn scale(a: Vec2, s: f32) -> Vec2 {
    Vec2::new(a.x * s, a.y * s)
}
fn dot(a: Vec2, b: Vec2) -> f32 {
    a.x * b.x + a.y * b.y
}
fn cross(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}
fn length(a: Vec2) -> f32 {
    dot(a, a).sqrt()
}
/// Left normal (the direction rotated +90°).
fn perp(a: Vec2) -> Vec2 {
    Vec2::new(-a.y, a.x)
}
fn normalize(a: Vec2) -> Option<Vec2> {
    let l = length(a);
    if l > EPS {
        Some(scale(a, 1.0 / l))
    } else {
        None
    }
}

// ---- flattening -------------------------------------------------------------

/// Walk `path` into flattened contours, lowering quads/cubics to polylines and
/// dropping consecutive duplicate points.
///
/// A contour is only *seeded* (with the pen position) when a drawing command
/// actually follows — never eagerly on `MoveTo`/`Close`. Eager seeding leaked
/// the bare cursor out as a phantom one-point open contour, which a round or
/// square cap then painted as a ghost dot: after every `Close` (Codex round-3
/// finding on PR #150) and after a lone trailing `MoveTo` (same family; SVG
/// does not stroke a subpath consisting of only a moveto). A genuine
/// zero-length subpath (`MoveTo` + degenerate draw command) still comes out as
/// a one-point contour, which SVG *does* mark under round/square caps.
fn flatten_contours(path: &VectorPath) -> Vec<Contour> {
    let mut out: Vec<Contour> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let mut start = Vec2::ZERO;
    let mut pen = Vec2::ZERO;

    let flush = |out: &mut Vec<Contour>, pts: &mut Vec<Vec2>, closed: bool| {
        if !pts.is_empty() {
            out.push(Contour {
                points: std::mem::take(pts),
                closed,
            });
        }
    };
    let seed = |cur: &mut Vec<Vec2>, pen: Vec2| {
        if cur.is_empty() {
            cur.push(pen);
        }
    };

    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                flush(&mut out, &mut cur, false);
                start = p;
                pen = p;
            }
            PathCommand::LineTo(p) => {
                seed(&mut cur, pen);
                push_point(&mut cur, p);
                pen = p;
            }
            PathCommand::QuadTo { control, to } => {
                seed(&mut cur, pen);
                flatten_quad(&mut cur, pen, control, to);
                pen = to;
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                seed(&mut cur, pen);
                match subdivide_cubic(pen, control1, control2, to) {
                    Ok(chain) => {
                        let mut seg_start = pen;
                        for (control, end) in chain {
                            flatten_quad(&mut cur, seg_start, control, end);
                            seg_start = end;
                        }
                    }
                    Err(_) => {
                        // Numerically-degenerate cubic (see #148 R5
                        // `SubdivisionError::{PrecisionUnderflow, DegenerateInput}`):
                        // fall back to the chord so the contour stays continuous
                        // and downstream stroke expansion still produces geometry.
                        push_point(&mut cur, to);
                    }
                }
                pen = to;
            }
            PathCommand::Close => {
                // A closed contour is a ring; drop a trailing point coincident
                // with the start so the ring is not doubled.
                if cur.len() >= 2 && length(sub(*cur.last().unwrap(), start)) <= EPS {
                    cur.pop();
                }
                flush(&mut out, &mut cur, true);
                // The pen returns to the subpath start; a following drawing
                // command re-seeds a new contour from there (SVG semantics).
                pen = start;
            }
        }
    }
    flush(&mut out, &mut cur, false);
    out
}

/// Push `p` unless it duplicates the current last point.
fn push_point(pts: &mut Vec<Vec2>, p: Vec2) {
    if pts.last().is_none_or(|last| length(sub(p, *last)) > EPS) {
        pts.push(p);
    }
}

/// Sample a quadratic `(from, control, to)` into line points (skips `from`,
/// which the caller already holds).
fn flatten_quad(pts: &mut Vec<Vec2>, from: Vec2, control: Vec2, to: Vec2) {
    for i in 1..=CURVE_FLATTEN_STEPS {
        let t = i as f32 / CURVE_FLATTEN_STEPS as f32;
        let mt = 1.0 - t;
        let p = add(
            add(scale(from, mt * mt), scale(control, 2.0 * mt * t)),
            scale(to, t * t),
        );
        push_point(pts, p);
    }
}

// ---- dashing ----------------------------------------------------------------

/// A dash pattern draws something only if it has a finite, positive total.
fn dash_is_active(dash: &DashPattern) -> bool {
    dash.is_finite()
        && !dash.intervals.is_empty()
        && dash.intervals.iter().any(|&v| v > EPS)
        && dash.intervals.iter().all(|&v| v >= 0.0)
        && dash.intervals.iter().sum::<f32>() > EPS
}

/// Split a contour into its "on" runs per the dash pattern, walked by arc
/// length. A closed contour is walked including its closing edge; an "on" run
/// that wraps its closing seam comes back as **one** open run with the seam
/// vertex interior (so it takes the configured join, not two caps), and a
/// pattern that never switches off within the perimeter yields a single
/// *closed* run. All other runs are open.
fn apply_dash(contour: &Contour, dash: &DashPattern) -> Vec<Contour> {
    let mut pts = contour.points.clone();
    if contour.closed {
        if let Some(&first) = pts.first() {
            pts.push(first);
        }
    }

    let total: f32 = dash.intervals.iter().sum();
    // Phase: how far into the (cyclic) pattern drawing begins.
    let mut phase = dash.offset.rem_euclid(total);
    let mut idx = 0usize;
    let mut on = true;
    // Advance the pattern cursor to the offset position.
    loop {
        let seg = dash.intervals[idx];
        if phase < seg || (idx == dash.intervals.len() - 1 && phase <= seg) {
            break;
        }
        phase -= seg;
        idx = (idx + 1) % dash.intervals.len();
        on = !on;
    }
    let mut remaining = dash.intervals[idx] - phase;
    // Dash state at t=0, before the walk mutates `on`; a closed contour needs
    // it at the end to detect an "on" run wrapping the closing seam.
    let started_on = on;

    // A zero-length subpath has no arc to walk, but the pattern still decides
    // its visibility: keep the cap-dot contour when the pattern is "on" at its
    // position (a closed degenerate paints nothing downstream either way).
    if pts.len() < 2 {
        return if started_on && !contour.closed {
            vec![contour.clone()]
        } else {
            Vec::new()
        };
    }

    let mut runs: Vec<Contour> = Vec::new();
    let mut current: Vec<Vec2> = Vec::new();
    if on {
        current.push(pts[0]);
    }

    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg_vec = sub(b, a);
        let mut seg_len = length(seg_vec);
        if seg_len <= EPS {
            continue;
        }
        let dir = scale(seg_vec, 1.0 / seg_len);
        let mut pos = a;

        while seg_len > remaining + EPS {
            // A dash boundary falls inside this segment.
            pos = add(pos, scale(dir, remaining));
            seg_len -= remaining;
            if on {
                current.push(pos);
                if current.len() >= 2 {
                    runs.push(Contour {
                        points: std::mem::take(&mut current),
                        closed: false,
                    });
                } else {
                    current.clear();
                }
            } else {
                current.clear();
                current.push(pos);
            }
            on = !on;
            idx = (idx + 1) % dash.intervals.len();
            remaining = dash.intervals[idx];
        }

        remaining -= seg_len;
        if on {
            current.push(b);
        }

        // A dash boundary can land exactly on (or within EPS of) the segment
        // end. Advance the dash state *now*: deferring to the next segment's
        // boundary walk would re-emit this point as a zero-length edge at the
        // start of that segment, and `expand_contour` would then derive a
        // degenerate end-cap direction from the duplicated terminal points,
        // silently dropping square/round caps (Codex round-1 finding on
        // PR #150). Invariant restored: an emitted run never carries a
        // zero-length terminal edge from a boundary-on-vertex split. A single
        // advance suffices — any zero-length intervals that follow are
        // consumed by the next segment's boundary walk as before.
        if remaining <= EPS {
            if on {
                if current.len() >= 2 {
                    runs.push(Contour {
                        points: std::mem::take(&mut current),
                        closed: false,
                    });
                } else {
                    current.clear();
                }
            } else {
                current.clear();
                current.push(b);
            }
            on = !on;
            idx = (idx + 1) % dash.intervals.len();
            remaining = dash.intervals[idx];
        }
    }

    // Seam wrap-merge for closed contours (Codex round-2 finding on PR #150):
    // when the walk both starts and ends "on", the pattern's final on-run and
    // its first on-run are one continuous dash crossing the closing seam.
    // Stitch the pre-seam tail (`current`) onto the post-seam head (`runs[0]`,
    // which began at `pts[0]` because the walk started on) so the seam vertex
    // becomes an interior vertex of a single run — `expand_contour` then
    // paints the configured join there instead of two caps. The pattern
    // cursor/phase logic above is untouched: the walk stays linear in arc
    // length and only already-emitted geometry is stitched. `push_point`
    // dedupes the seam point the tail and head share. When no off-boundary
    // ever fired (`runs` is empty), the dash covers the whole ring: emit it as
    // a *closed* run — every vertex joined, no caps — dropping the duplicated
    // seam point the closing-edge walk appended.
    if on && !current.is_empty() {
        if contour.closed && started_on {
            if runs.is_empty() {
                if current.len() >= 2 && length(sub(*current.last().unwrap(), current[0])) <= EPS {
                    current.pop();
                }
                if current.len() >= 2 {
                    runs.push(Contour {
                        points: current,
                        closed: true,
                    });
                }
            } else {
                let head = runs.remove(0);
                for p in head.points {
                    push_point(&mut current, p);
                }
                if current.len() >= 2 {
                    runs.push(Contour {
                        points: current,
                        closed: false,
                    });
                }
            }
        } else if current.len() >= 2 {
            runs.push(Contour {
                points: current,
                closed: false,
            });
        }
    }
    runs
}

// ---- expansion --------------------------------------------------------------

/// Expand one flattened contour into convex pieces (all CCW), appended to `out`.
fn expand_contour(
    contour: &Contour,
    half: f32,
    cap: LineCap,
    join: LineJoin,
    out: &mut Vec<Vec<Vec2>>,
) {
    let pts = &contour.points;

    // A degenerate contour (a single point, or all points coincident): SVG
    // marks a zero-length open subpath under a round cap (a dot) or a square
    // cap (an axis-aligned width×width square — the subpath has no direction
    // to orient it by); butt caps and closed contours paint nothing.
    if segment_dirs(pts, contour.closed).is_empty() {
        if !contour.closed {
            if let Some(&c) = pts.first() {
                match cap {
                    LineCap::Round => out.push(circle(c, half)),
                    LineCap::Square => out.push(ccw(vec![
                        Vec2::new(c.x - half, c.y - half),
                        Vec2::new(c.x + half, c.y - half),
                        Vec2::new(c.x + half, c.y + half),
                        Vec2::new(c.x - half, c.y + half),
                    ])),
                    LineCap::Butt => {}
                }
            }
        }
        return;
    }

    let n = pts.len();
    let closed = contour.closed;

    // One rectangle per segment.
    let seg_count = if closed { n } else { n - 1 };
    for i in 0..seg_count {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        if let Some(rect) = segment_rect(a, b, half) {
            out.push(rect);
        }
    }

    // Joins at interior vertices (and the wrap vertex for a closed contour).
    let join_range = if closed { 0..n } else { 1..(n - 1) };
    for i in join_range {
        let prev = pts[(i + n - 1) % n];
        let v = pts[i];
        let next = pts[(i + 1) % n];
        add_join(prev, v, next, half, join, out);
    }

    // Caps at the two ends of an open contour. Each cap direction comes from
    // the nearest *non-degenerate* segment, not blindly from the two terminal
    // points: a zero-length terminal edge (e.g. a dash boundary duplicated
    // onto a path vertex) must not erase the cap. `apply_dash` no longer
    // produces such edges, but deriving the direction robustly here is a
    // one-line backstop that also covers any other producer of near-duplicate
    // endpoints. The `segment_dirs` guard above ensures a direction exists.
    if !closed {
        if let Some(d0) = pts.windows(2).find_map(|w| dir(w[0], w[1])) {
            add_cap(pts[0], scale(d0, -1.0), half, cap, out);
        }
        if let Some(dn) = pts.windows(2).rev().find_map(|w| dir(w[0], w[1])) {
            add_cap(pts[n - 1], dn, half, cap, out);
        }
    }
}

/// Unit directions of each non-degenerate segment; empty when the contour has no
/// drawable extent.
fn segment_dirs(pts: &[Vec2], closed: bool) -> Vec<Vec2> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let seg_count = if closed { n } else { n - 1 };
    let mut dirs = Vec::new();
    for i in 0..seg_count {
        if let Some(d) = dir(pts[i], pts[(i + 1) % n]) {
            dirs.push(d);
        }
    }
    dirs
}

fn dir(a: Vec2, b: Vec2) -> Option<Vec2> {
    normalize(sub(b, a))
}

/// The rectangle covering segment `a→b` at half-width `half`, wound CCW.
fn segment_rect(a: Vec2, b: Vec2, half: f32) -> Option<Vec<Vec2>> {
    let d = dir(a, b)?;
    let nrm = scale(perp(d), half);
    Some(ccw(vec![
        add(a, nrm),
        add(b, nrm),
        sub(b, nrm),
        sub(a, nrm),
    ]))
}

/// Fill the outer wedge of the corner `prev→v→next` per the join style.
fn add_join(prev: Vec2, v: Vec2, next: Vec2, half: f32, join: LineJoin, out: &mut Vec<Vec<Vec2>>) {
    let (d0, d1) = match (dir(prev, v), dir(v, next)) {
        (Some(a), Some(b)) => (a, b),
        _ => return,
    };
    let turn = cross(d0, d1);
    if turn.abs() <= EPS {
        // Straight-through: the two rectangles already meet flush. An exact
        // 180° reversal (a spike) has no outer side to wedge, but a round
        // join must still bulge past the turning point — the shape is exactly
        // a round cap pointing along the incoming direction. Miter (infinite
        // apex → limit fallback) and bevel both degenerate to zero area there.
        if join == LineJoin::Round && dot(d0, d1) < 0.0 {
            add_cap(v, d0, half, LineCap::Round, out);
        }
        return;
    }
    let sign = turn.signum();
    // Outer offset points: the outer corners of the two adjacent rectangles.
    let p_in = add(v, scale(perp(d0), -sign * half));
    let p_out = add(v, scale(perp(d1), -sign * half));

    match join {
        LineJoin::Bevel => out.push(ccw(vec![v, p_in, p_out])),
        LineJoin::Round => out.push(ccw(round_wedge(v, p_in, p_out, half))),
        LineJoin::Miter => {
            if let Some(apex) = miter_apex(v, p_in, d0, p_out, d1) {
                if length(sub(apex, v)) <= MITER_LIMIT * half {
                    out.push(ccw(vec![v, p_in, apex, p_out]));
                    return;
                }
            }
            out.push(ccw(vec![v, p_in, p_out]));
        }
    }
}

/// Intersection of the two outer edges (line through `p_in` along `d0`, line
/// through `p_out` along `d1`); `None` when they are parallel.
fn miter_apex(_v: Vec2, p_in: Vec2, d0: Vec2, p_out: Vec2, d1: Vec2) -> Option<Vec2> {
    let denom = cross(d0, d1);
    if denom.abs() <= EPS {
        return None;
    }
    let diff = sub(p_out, p_in);
    let t = cross(diff, d1) / denom;
    Some(add(p_in, scale(d0, t)))
}

/// A pie-slice fan from `from` to `to` around `centre` at radius `half`,
/// sweeping the minor (outer) arc; the first point is the centre.
fn round_wedge(centre: Vec2, from: Vec2, to: Vec2, half: f32) -> Vec<Vec2> {
    let a0 = angle(sub(from, centre));
    let a1 = angle(sub(to, centre));
    let mut sweep = a1 - a0;
    while sweep <= -std::f32::consts::PI {
        sweep += std::f32::consts::TAU;
    }
    while sweep > std::f32::consts::PI {
        sweep -= std::f32::consts::TAU;
    }
    let steps = ((sweep.abs() / ROUND_STEP_RAD).ceil() as u32).max(1);
    let mut pts = vec![centre, from];
    for i in 1..steps {
        let a = a0 + sweep * (i as f32 / steps as f32);
        pts.push(add(centre, Vec2::new(a.cos() * half, a.sin() * half)));
    }
    pts.push(to);
    pts
}

/// Add an end cap at `end`, where `outward` is the unit direction pointing away
/// from the contour.
fn add_cap(end: Vec2, outward: Vec2, half: f32, cap: LineCap, out: &mut Vec<Vec<Vec2>>) {
    match cap {
        LineCap::Butt => {}
        LineCap::Square => {
            let side = scale(perp(outward), half);
            let ext = scale(outward, half);
            out.push(ccw(vec![
                add(end, side),
                add(add(end, side), ext),
                add(sub(end, side), ext),
                sub(end, side),
            ]));
        }
        LineCap::Round => {
            let side = scale(perp(outward), half);
            // A semicircle from one side to the other, bulging outward.
            let from = add(end, side);
            let to = sub(end, side);
            // Force the sweep through the outward side by going the long way if
            // needed: build the arc explicitly across 180° toward `outward`.
            out.push(ccw(semicircle(end, from, to, outward, half)));
        }
    }
}

/// A semicircle fan centred at `centre` from `from` to `to` bulging toward
/// `outward`; the first point is the centre.
fn semicircle(centre: Vec2, from: Vec2, to: Vec2, outward: Vec2, half: f32) -> Vec<Vec2> {
    let a0 = angle(sub(from, centre));
    let ao = angle(outward);
    // Choose the sweep direction that passes through `outward`.
    let mut sweep = std::f32::consts::PI;
    if delta_angle(a0, ao) < 0.0 {
        sweep = -sweep;
    }
    let steps = ((std::f32::consts::PI / ROUND_STEP_RAD).ceil() as u32).max(2);
    let mut pts = vec![centre, from];
    for i in 1..steps {
        let a = a0 + sweep * (i as f32 / steps as f32);
        pts.push(add(centre, Vec2::new(a.cos() * half, a.sin() * half)));
    }
    pts.push(to);
    pts
}

/// A full circle polygon centred at `centre` (for a round-capped dot).
fn circle(centre: Vec2, half: f32) -> Vec<Vec2> {
    let steps = ((std::f32::consts::TAU / ROUND_STEP_RAD).ceil() as u32).max(8);
    let mut pts = Vec::with_capacity(steps as usize);
    for i in 0..steps {
        let a = std::f32::consts::TAU * (i as f32 / steps as f32);
        pts.push(add(centre, Vec2::new(a.cos() * half, a.sin() * half)));
    }
    ccw(pts)
}

fn angle(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}

/// Signed smallest angle from `a` to `b` in `(-π, π]`.
fn delta_angle(a: f32, b: f32) -> f32 {
    let mut d = b - a;
    while d <= -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    while d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    }
    d
}

/// Twice the signed area of a polygon (shoelace); positive when CCW.
fn signed_area2(pts: &[Vec2]) -> f32 {
    let n = pts.len();
    let mut s = 0.0;
    for i in 0..n {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        s += cross(a, b);
    }
    s
}

/// Return `pts` wound counter-clockwise (positive area), reversing if needed, so
/// every emitted piece shares one winding sense for the non-zero union.
fn ccw(mut pts: Vec<Vec2>) -> Vec<Vec2> {
    if signed_area2(&pts) < 0.0 {
        pts.reverse();
    }
    pts
}

/// Assemble the convex pieces into one non-zero-fill [`VectorPath`], each piece a
/// closed sub-contour, with bounds derived from all points.
fn build_path(pieces: Vec<Vec<Vec2>>) -> VectorPath {
    let mut commands = Vec::new();
    let mut min = Vec2::new(f32::INFINITY, f32::INFINITY);
    let mut max = Vec2::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for piece in pieces {
        if piece.len() < 3 {
            continue;
        }
        commands.push(PathCommand::MoveTo(piece[0]));
        for &p in &piece[1..] {
            commands.push(PathCommand::LineTo(p));
        }
        commands.push(PathCommand::Close);
        for p in piece {
            min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
            max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
        }
    }
    if commands.is_empty() {
        return empty_path();
    }
    VectorPath::new(commands, FillRule::NonZero, Bounds::new(min, max))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encode::encode_vector_path;
    use crate::slug::SlugConfig;

    fn line(a: Vec2, b: Vec2) -> VectorPath {
        VectorPath::new(
            vec![PathCommand::MoveTo(a), PathCommand::LineTo(b)],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        )
    }

    fn polyline(pts: &[Vec2]) -> VectorPath {
        let mut cmds = vec![PathCommand::MoveTo(pts[0])];
        for &p in &pts[1..] {
            cmds.push(PathCommand::LineTo(p));
        }
        VectorPath::new(cmds, FillRule::NonZero, Bounds::new(Vec2::ZERO, Vec2::ZERO))
    }

    /// Winding number of `p` w.r.t. all closed sub-contours of a fill path;
    /// non-zero means the non-zero rule fills that point.
    fn winding(path: &VectorPath, p: Vec2) -> i32 {
        let mut wind = 0;
        let mut contour: Vec<Vec2> = Vec::new();
        let flush = |contour: &mut Vec<Vec2>, wind: &mut i32| {
            let n = contour.len();
            for i in 0..n {
                let a = contour[i];
                let b = contour[(i + 1) % n];
                if (a.y <= p.y) != (b.y <= p.y) {
                    let t = (p.y - a.y) / (b.y - a.y);
                    let x = a.x + t * (b.x - a.x);
                    if x > p.x {
                        *wind += if b.y > a.y { 1 } else { -1 };
                    }
                }
            }
            contour.clear();
        };
        for cmd in &path.commands {
            match *cmd {
                PathCommand::MoveTo(v) => {
                    flush(&mut contour, &mut wind);
                    contour.push(v);
                }
                PathCommand::LineTo(v) => contour.push(v),
                PathCommand::Close => flush(&mut contour, &mut wind),
                _ => {}
            }
        }
        flush(&mut contour, &mut wind);
        wind
    }

    #[test]
    fn zero_width_paints_nothing() {
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(10.0, 0.0)), &Stroke::new(0.0));
        assert!(out.commands.is_empty());
    }

    #[test]
    fn butt_line_is_a_rectangle_bracketing_the_centreline() {
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(10.0, 0.0)), &Stroke::new(4.0));
        // Half-width 2: the outline spans y in [-2, 2] and x in [0, 10] (butt
        // caps add no x extent).
        assert!((out.bounds.min.y + 2.0).abs() < 1e-4);
        assert!((out.bounds.max.y - 2.0).abs() < 1e-4);
        assert!((out.bounds.min.x - 0.0).abs() < 1e-4);
        assert!((out.bounds.max.x - 10.0).abs() < 1e-4);
        // The centreline midpoint is covered; a point well above is not.
        assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0);
        assert_eq!(winding(&out, Vec2::new(5.0, 10.0)), 0);
    }

    #[test]
    fn square_cap_extends_beyond_the_endpoints() {
        let stroke = Stroke {
            cap: LineCap::Square,
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(10.0, 0.0)), &stroke);
        // Square caps extend by half-width (2) past each end.
        assert!(
            (out.bounds.min.x + 2.0).abs() < 1e-4,
            "min.x={}",
            out.bounds.min.x
        );
        assert!(
            (out.bounds.max.x - 12.0).abs() < 1e-4,
            "max.x={}",
            out.bounds.max.x
        );
        // A point just past the endpoint is now covered.
        assert_ne!(winding(&out, Vec2::new(11.0, 0.0)), 0);
    }

    #[test]
    fn round_cap_extends_and_is_smooth() {
        let stroke = Stroke {
            cap: LineCap::Round,
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(10.0, 0.0)), &stroke);
        // The tessellated apex lands within a chord of the true radius, so allow
        // a small slack below the exact ±half-width extent (butt would be 0/10).
        assert!(out.bounds.min.x <= -1.9, "min.x={}", out.bounds.min.x);
        assert!(out.bounds.max.x >= 11.9, "max.x={}", out.bounds.max.x);
        // Just inside the round cap at the end is covered; just outside is not.
        assert_ne!(winding(&out, Vec2::new(11.5, 0.0)), 0);
        assert_eq!(winding(&out, Vec2::new(12.5, 0.0)), 0);
    }

    #[test]
    fn miter_join_reaches_further_than_bevel_at_a_right_angle() {
        // An L-shape: (0,10)->(0,0)->(10,0). The outer corner is at (-h,-h)-ish.
        let corner = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 10.0)),
                PathCommand::LineTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(10.0, 0.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let miter = stroke_to_fill(
            &corner,
            &Stroke {
                join: LineJoin::Miter,
                ..Stroke::new(4.0)
            },
        );
        let bevel = stroke_to_fill(
            &corner,
            &Stroke {
                join: LineJoin::Bevel,
                ..Stroke::new(4.0)
            },
        );
        // The miter fills the sharp outer apex at (-2,-2); the bevel cuts it off.
        assert_ne!(
            winding(&miter, Vec2::new(-1.5, -1.5)),
            0,
            "miter fills the apex"
        );
        assert_eq!(
            winding(&bevel, Vec2::new(-1.5, -1.5)),
            0,
            "bevel cuts the apex"
        );
    }

    #[test]
    fn round_join_fills_the_arc_corner() {
        let corner = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 10.0)),
                PathCommand::LineTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(10.0, 0.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let round = stroke_to_fill(
            &corner,
            &Stroke {
                join: LineJoin::Round,
                ..Stroke::new(4.0)
            },
        );
        // A point on the arc (distance ~2 from the corner, diagonal) is covered;
        // the far apex the miter would reach is not.
        let d = 2.0 / (2.0_f32).sqrt();
        assert_ne!(winding(&round, Vec2::new(-d + 0.05, -d + 0.05)), 0);
        assert_eq!(
            winding(&round, Vec2::new(-1.9, -1.9)),
            0,
            "round stays inside the miter apex"
        );
    }

    #[test]
    fn dash_splits_into_multiple_runs() {
        // A length-100 line dashed 10-on/10-off yields 5 "on" runs.
        let stroke = Stroke {
            dash: Some(DashPattern::new(vec![10.0, 10.0], 0.0)),
            ..Stroke::new(2.0)
        };
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(100.0, 0.0)), &stroke);
        let moves = out
            .commands
            .iter()
            .filter(|c| matches!(c, PathCommand::MoveTo(_)))
            .count();
        // Each 10px "on" run is one rectangle → one sub-contour → one MoveTo.
        assert_eq!(moves, 5, "expected 5 dash runs, got {moves}");
        // The first dash [0,10] is covered; the first gap [10,20] is not.
        assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0);
        assert_eq!(winding(&out, Vec2::new(15.0, 0.0)), 0);
    }

    #[test]
    fn closed_square_strokes_a_ring() {
        let sq = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(20.0, 0.0)),
                PathCommand::LineTo(Vec2::new(20.0, 20.0)),
                PathCommand::LineTo(Vec2::new(0.0, 20.0)),
                PathCommand::Close,
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let out = stroke_to_fill(&sq, &Stroke::new(4.0));
        // On the edge (10,0) is inked; the hollow centre (10,10) and the far
        // exterior (10,-10) are not.
        assert_ne!(winding(&out, Vec2::new(10.0, 0.0)), 0, "edge is stroked");
        assert_eq!(winding(&out, Vec2::new(10.0, 10.0)), 0, "centre is hollow");
        assert_eq!(winding(&out, Vec2::new(10.0, -10.0)), 0, "outside is clear");
    }

    #[test]
    fn curved_centreline_flattens_and_strokes() {
        let curve = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::QuadTo {
                    control: Vec2::new(50.0, 50.0),
                    to: Vec2::new(100.0, 0.0),
                },
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let out = stroke_to_fill(&curve, &Stroke::new(6.0));
        assert!(!out.commands.is_empty());
        // The apex of the arch (25,25) sits on the centreline and is covered.
        assert_ne!(winding(&out, Vec2::new(50.0, 25.0)), 0);
    }

    #[test]
    fn expanded_outline_encodes_through_the_slug_pipeline() {
        // The whole point: a stroked path must feed the existing coverage
        // encoder and yield real curve/band records.
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(10.0, 0.0)), &Stroke::new(4.0));
        let glyph = encode_vector_path(&out, &SlugConfig::new(4, 4, 1)).unwrap();
        assert!(!glyph.curves.is_empty());
        assert!(!glyph.horizontal_bands.is_empty());
    }

    #[test]
    fn non_finite_stroke_paints_nothing() {
        let out = stroke_to_fill(
            &line(Vec2::ZERO, Vec2::new(10.0, 0.0)),
            &Stroke::new(f32::NAN),
        );
        assert!(out.commands.is_empty());
    }

    /// Codex round-1 regression (#138 / PR #150): a dash boundary that lands
    /// exactly on an existing path vertex must not erase the end cap. The
    /// first dash of `M0,0 L10,0 L20,0` with pattern `[10, 10]` ends at the
    /// vertex `(10, 0)`; its square cap extends half-width (2) past it.
    #[test]
    fn codex_r1_dash_boundary_at_path_vertex_preserves_cap() {
        let path = polyline(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)]);
        let stroke = Stroke {
            cap: LineCap::Square,
            dash: Some(DashPattern::new(vec![10.0, 10.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&path, &stroke);
        assert_ne!(
            winding(&out, Vec2::new(11.0, 0.0)),
            0,
            "square cap missing where the dash ends on a path vertex"
        );
        assert!(out.bounds.max.x >= 11.0, "max.x={}", out.bounds.max.x);
    }

    /// Same boundary-on-vertex scenario as
    /// [`codex_r1_dash_boundary_at_path_vertex_preserves_cap`], round cap.
    #[test]
    fn dash_boundary_at_path_vertex_preserves_round_cap() {
        let path = polyline(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)]);
        let stroke = Stroke {
            cap: LineCap::Round,
            dash: Some(DashPattern::new(vec![10.0, 10.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&path, &stroke);
        assert_ne!(
            winding(&out, Vec2::new(11.0, 0.0)),
            0,
            "round cap missing where the dash ends on a path vertex"
        );
        assert!(out.bounds.max.x >= 11.5, "max.x={}", out.bounds.max.x);
    }

    /// Butt caps add nothing by definition, but the boundary-on-vertex case
    /// must still produce the plain dash rectangle without artifacts.
    #[test]
    fn dash_boundary_at_path_vertex_butt_is_unchanged() {
        let path = polyline(&[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0)]);
        let stroke = Stroke {
            cap: LineCap::Butt,
            dash: Some(DashPattern::new(vec![10.0, 10.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&path, &stroke);
        assert!(!out.commands.is_empty());
        assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0, "dash body inked");
        assert_eq!(winding(&out, Vec2::new(11.0, 0.0)), 0, "no cap extent");
        assert!(
            (out.bounds.max.x - 10.0).abs() < 1e-3,
            "max.x={}",
            out.bounds.max.x
        );
    }

    /// A closed 10×10 square used by the seam-wrap regression tests below.
    fn closed_square10() -> VectorPath {
        VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(10.0, 0.0)),
                PathCommand::LineTo(Vec2::new(10.0, 10.0)),
                PathCommand::LineTo(Vec2::new(0.0, 10.0)),
                PathCommand::Close,
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        )
    }

    /// Codex round-2 regression (#138 / PR #150): on a closed contour, an "on"
    /// run wrapping the closing seam must be one run with the seam vertex as
    /// an interior *joined* vertex, not two capped open runs. Perimeter 40,
    /// dash `[30, 10]` at offset 5: on from t=35 through the seam to t=25, so
    /// the seam corner (0,0) carries the miter whose apex reaches (-2,-2).
    #[test]
    fn codex_r2_closed_seam_dash_wrap_preserves_join() {
        let stroke = Stroke {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dash: Some(DashPattern::new(vec![30.0, 10.0], 5.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&closed_square10(), &stroke);
        assert_ne!(
            winding(&out, Vec2::new(-1.5, -1.5)),
            0,
            "seam miter missing: wrapping dash run split into two capped runs"
        );
        // The off gap t in [25, 35] (midpoint of the top edge down the left
        // side) stays unpainted.
        assert_eq!(winding(&out, Vec2::new(2.5, 10.0)), 0, "gap painted");
    }

    /// All three join styles must render at the seam when the run wraps.
    /// (-0.6,-0.6) sits inside every join wedge; the outer probes tell the
    /// styles apart (miter reaches the apex, round stays within radius 2,
    /// bevel cuts the diagonal x+y=-2).
    #[test]
    fn closed_seam_dash_wrap_joins_all_styles() {
        for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
            let stroke = Stroke {
                cap: LineCap::Butt,
                join,
                dash: Some(DashPattern::new(vec![30.0, 10.0], 5.0)),
                ..Stroke::new(4.0)
            };
            let out = stroke_to_fill(&closed_square10(), &stroke);
            assert_ne!(
                winding(&out, Vec2::new(-0.6, -0.6)),
                0,
                "{join:?} join missing at the wrapped seam"
            );
            let apex = winding(&out, Vec2::new(-1.5, -1.5)) != 0;
            match join {
                LineJoin::Miter => assert!(apex, "miter reaches the seam apex"),
                LineJoin::Bevel => assert!(!apex, "bevel cuts the seam apex"),
                LineJoin::Round => {
                    assert!(!apex, "round stays inside the seam apex");
                    assert_ne!(winding(&out, Vec2::new(-1.0, -1.0)), 0, "round arc inked");
                }
            }
        }
    }

    /// An "on" run that terminates *before* the closing seam keeps its caps:
    /// no join material may appear at the seam corner.
    #[test]
    fn closed_seam_dash_not_wrapping_keeps_caps() {
        let stroke = Stroke {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dash: Some(DashPattern::new(vec![12.0, 28.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&closed_square10(), &stroke);
        // On-run covers t in [0, 12]: bottom edge plus 2 units up the right.
        assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0, "dash body inked");
        assert_ne!(winding(&out, Vec2::new(10.0, 1.5)), 0, "dash tail inked");
        // Butt cap at the seam start: nothing beyond it, no seam join.
        assert_eq!(winding(&out, Vec2::new(-0.6, -0.6)), 0, "stray seam join");
        assert_eq!(winding(&out, Vec2::new(-1.5, -1.5)), 0, "stray seam miter");
        assert_eq!(winding(&out, Vec2::new(0.0, 5.0)), 0, "off region painted");
    }

    /// A pattern that never switches off within the perimeter paints the whole
    /// ring as a closed stroke: joins everywhere (including the seam), no caps.
    #[test]
    fn fully_on_dash_ring_closes_with_seam_join() {
        let stroke = Stroke {
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            dash: Some(DashPattern::new(vec![100.0, 20.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&closed_square10(), &stroke);
        assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0, "edge is stroked");
        assert_eq!(winding(&out, Vec2::new(5.0, 5.0)), 0, "centre is hollow");
        assert_ne!(
            winding(&out, Vec2::new(-1.5, -1.5)),
            0,
            "seam corner takes a miter join like every other corner"
        );
    }

    /// Open contours are exempt from the wrap-merge: a dashed open polyline
    /// whose walk starts and ends "on" keeps a cap at each run end.
    #[test]
    fn open_contour_runs_keep_caps_no_wrap_merge() {
        let stroke = Stroke {
            cap: LineCap::Square,
            dash: Some(DashPattern::new(vec![12.0, 6.0], 0.0)),
            ..Stroke::new(4.0)
        };
        let out = stroke_to_fill(&line(Vec2::ZERO, Vec2::new(30.0, 0.0)), &stroke);
        // Runs [0,12] and [18,30]; square caps extend 2 past every run end.
        assert_ne!(winding(&out, Vec2::new(-1.0, 0.0)), 0, "start cap");
        assert_ne!(winding(&out, Vec2::new(31.0, 0.0)), 0, "end cap");
        assert_ne!(winding(&out, Vec2::new(13.0, 0.0)), 0, "cap into the gap");
        assert_ne!(winding(&out, Vec2::new(17.0, 0.0)), 0, "cap into the gap");
        assert_eq!(winding(&out, Vec2::new(15.0, 0.0)), 0, "gap centre clear");
    }

    /// Property-style: dashing a straight polyline must be indistinguishable
    /// from dashing a single segment of the same total length — vertices the
    /// pattern crosses (or lands on) contribute nothing. Coverage is compared
    /// on a sample grid whose x values sit at `*.x5` offsets so no sample
    /// lands exactly on a dash/cap boundary (all boundaries here are integer).
    #[test]
    fn dashes_crossing_vertices_match_single_segment_equivalent() {
        let multi = polyline(&[
            Vec2::ZERO,
            Vec2::new(10.0, 0.0),
            Vec2::new(20.0, 0.0),
            Vec2::new(30.0, 0.0),
        ]);
        let single = line(Vec2::ZERO, Vec2::new(30.0, 0.0));
        let patterns: [&[f32]; 4] = [
            &[10.0, 10.0],
            &[5.0, 5.0],
            &[7.0, 3.0],
            &[10.0, 5.0, 5.0, 5.0],
        ];
        for pattern in patterns {
            for cap in [LineCap::Butt, LineCap::Square, LineCap::Round] {
                let stroke = Stroke {
                    cap,
                    dash: Some(DashPattern::new(pattern.to_vec(), 0.0)),
                    ..Stroke::new(4.0)
                };
                let a = stroke_to_fill(&multi, &stroke);
                let b = stroke_to_fill(&single, &stroke);
                for xi in 0..=360 {
                    let x = xi as f32 * 0.1 - 3.05;
                    for y in [-1.95, -1.0, -0.05, 1.0, 1.95] {
                        let p = Vec2::new(x, y);
                        assert_eq!(
                            winding(&a, p) != 0,
                            winding(&b, p) != 0,
                            "coverage mismatch at ({x}, {y}) for dash {pattern:?}, cap {cap:?}"
                        );
                    }
                }
            }
        }
    }

    /// Build an open or closed path from a point list.
    fn contour_path(pts: &[Vec2], closed: bool) -> VectorPath {
        let mut cmds = vec![PathCommand::MoveTo(pts[0])];
        for &p in &pts[1..] {
            cmds.push(PathCommand::LineTo(p));
        }
        if closed {
            cmds.push(PathCommand::Close);
        }
        VectorPath::new(cmds, FillRule::NonZero, Bounds::new(Vec2::ZERO, Vec2::ZERO))
    }

    fn vecs(pts: &[(f32, f32)], s: f32) -> Vec<Vec2> {
        pts.iter().map(|&(x, y)| Vec2::new(x * s, y * s)).collect()
    }

    /// Winding-coverage signature of `path` over `probes`.
    fn coverage_sig(path: &VectorPath, probes: &[Vec2]) -> Vec<bool> {
        probes.iter().map(|&p| winding(path, p) != 0).collect()
    }

    /// An n×n probe grid over `[min, max]²`, scaled by `s`, offset off round
    /// coordinates so probes avoid the integer-aligned geometry edges these
    /// tests use.
    fn probe_grid(min: f32, max: f32, n: usize, s: f32) -> Vec<Vec2> {
        let step = (max - min) / n as f32;
        let mut probes = Vec::with_capacity(n * n);
        for i in 0..n {
            for j in 0..n {
                probes.push(Vec2::new(
                    (min + step * (i as f32 + 0.531)) * s,
                    (min + step * (j as f32 + 0.531)) * s,
                ));
            }
        }
        probes
    }

    /// Minimum distance from `p` to the polyline (or ring) through `pts`.
    fn dist_to_contour(p: Vec2, pts: &[Vec2], closed: bool) -> f32 {
        let n = pts.len();
        if n == 1 {
            return length(sub(p, pts[0]));
        }
        let seg_count = if closed { n } else { n - 1 };
        let mut best = f32::INFINITY;
        for i in 0..seg_count {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            let ab = sub(b, a);
            let t = (dot(sub(p, a), ab) / dot(ab, ab).max(1e-12)).clamp(0.0, 1.0);
            best = best.min(length(sub(p, add(a, scale(ab, t)))));
        }
        best
    }

    /// Codex round-3 regression (#138 / PR #150): flattening must not leak the
    /// post-`Close` cursor as a phantom one-point open contour — with a round
    /// (or square) cap it painted a ghost dot at the closed contour's seam.
    /// Cap style is meaningless on a closed contour, so coverage must be
    /// identical across all caps; with bevel joins the probe (-1.2, -1.2)
    /// sits outside the bevel and must stay unpainted for every cap.
    #[test]
    fn codex_r3_closed_contour_round_cap_no_ghost_circle() {
        let probes = [
            Vec2::new(-1.2, -1.2),
            Vec2::new(-1.5, -1.5),
            Vec2::new(-0.5, -0.5),
            Vec2::new(5.0, 0.0),
            Vec2::new(5.0, 5.0),
            Vec2::new(11.0, 5.0),
        ];
        let sig = |cap: LineCap| {
            let stroke = Stroke {
                cap,
                join: LineJoin::Bevel,
                ..Stroke::new(4.0)
            };
            coverage_sig(&stroke_to_fill(&closed_square10(), &stroke), &probes)
        };
        let butt = sig(LineCap::Butt);
        assert!(!butt[0], "bevel seam corner leaks at (-1.2,-1.2)");
        assert!(butt[3] && !butt[4], "square edge inked, centre hollow");
        assert_eq!(
            butt,
            sig(LineCap::Round),
            "round cap changed a closed contour"
        );
        assert_eq!(
            butt,
            sig(LineCap::Square),
            "square cap changed a closed contour"
        );
    }

    /// A subpath consisting of a lone `MoveTo` is not stroked (SVG): no ghost
    /// cap dot may appear at the bare cursor position.
    #[test]
    fn lone_moveto_paints_nothing() {
        let stroke = Stroke {
            cap: LineCap::Round,
            ..Stroke::new(4.0)
        };
        let lone = VectorPath::new(
            vec![PathCommand::MoveTo(Vec2::new(5.0, 5.0))],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        assert!(stroke_to_fill(&lone, &stroke).commands.is_empty());

        let mixed = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(5.0, 5.0)),
                PathCommand::MoveTo(Vec2::new(20.0, 0.0)),
                PathCommand::LineTo(Vec2::new(30.0, 0.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let out = stroke_to_fill(&mixed, &stroke);
        assert_eq!(
            winding(&out, Vec2::new(5.0, 5.0)),
            0,
            "abandoned MoveTo dot"
        );
        assert_ne!(winding(&out, Vec2::new(25.0, 0.0)), 0, "real segment inked");
    }

    /// A drawing command after `Close` starts a new subpath at the closed
    /// contour's start point (SVG pen semantics) — guarding the lazy-seeding
    /// rewrite of `flatten_contours`.
    #[test]
    fn close_then_lineto_starts_new_subpath_at_seam() {
        let path = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(10.0, 0.0)),
                PathCommand::LineTo(Vec2::new(0.0, 10.0)),
                PathCommand::Close,
                PathCommand::LineTo(Vec2::new(0.0, -10.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let butt = stroke_to_fill(&path, &Stroke::new(4.0));
        assert_ne!(winding(&butt, Vec2::new(5.0, 0.5)), 0, "triangle stroked");
        assert_ne!(
            winding(&butt, Vec2::new(0.0, -5.0)),
            0,
            "tail subpath stroked"
        );
        assert_eq!(winding(&butt, Vec2::new(0.0, -11.0)), 0, "butt adds no cap");
        let round = stroke_to_fill(
            &path,
            &Stroke {
                cap: LineCap::Round,
                ..Stroke::new(4.0)
            },
        );
        assert_ne!(
            winding(&round, Vec2::new(0.0, -11.0)),
            0,
            "tail is open: round cap extends past its endpoint"
        );
    }

    /// An exact 180° reversal (a spike) must bulge under a round join — the
    /// outer arc degenerates to a semicircle past the turning point. Miter
    /// (infinite apex) and bevel (zero area) paint nothing extra there.
    #[test]
    fn spike_reversal_round_join_bulges() {
        let spike = polyline(&vecs(&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.0)], 1.0));
        let probe = Vec2::new(11.5, 0.0);
        for (join, bulges) in [
            (LineJoin::Round, true),
            (LineJoin::Miter, false),
            (LineJoin::Bevel, false),
        ] {
            let out = stroke_to_fill(
                &spike,
                &Stroke {
                    join,
                    ..Stroke::new(4.0)
                },
            );
            assert_ne!(winding(&out, Vec2::new(5.0, 0.0)), 0, "{join:?}: body");
            assert_eq!(
                winding(&out, probe) != 0,
                bulges,
                "{join:?} at the spike tip"
            );
        }
    }

    /// A degenerate closed two-point ring (`M a L b Z`) is a there-and-back
    /// line: round joins at both reversals make it a capsule.
    #[test]
    fn closed_two_point_ring_round_join_is_a_capsule() {
        let ring = contour_path(&vecs(&[(0.0, 0.0), (10.0, 0.0)], 1.0), true);
        let round = stroke_to_fill(
            &ring,
            &Stroke {
                join: LineJoin::Round,
                ..Stroke::new(4.0)
            },
        );
        assert_ne!(winding(&round, Vec2::new(5.0, 1.5)), 0, "body inked");
        assert_ne!(winding(&round, Vec2::new(-1.5, 0.0)), 0, "left bulge");
        assert_ne!(winding(&round, Vec2::new(11.5, 0.0)), 0, "right bulge");
        let miter = stroke_to_fill(&ring, &Stroke::new(4.0));
        assert_eq!(
            winding(&miter, Vec2::new(11.5, 0.0)),
            0,
            "miter spike is cut"
        );
    }

    /// SVG marks a zero-length subpath under a square cap too: an axis-aligned
    /// width×width square (there is no direction to orient it by). Butt caps
    /// paint nothing.
    #[test]
    fn square_cap_marks_zero_length_subpath() {
        let dot = contour_path(&[Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0)], false);
        let square = stroke_to_fill(
            &dot,
            &Stroke {
                cap: LineCap::Square,
                ..Stroke::new(4.0)
            },
        );
        assert_ne!(winding(&square, Vec2::new(6.9, 6.9)), 0, "square corner");
        assert_eq!(
            winding(&square, Vec2::new(5.0, 7.5)),
            0,
            "beyond half-width"
        );
        assert_eq!(
            winding(&square, Vec2::new(8.0, 5.0)),
            0,
            "beyond half-width"
        );
        let butt = stroke_to_fill(&dot, &Stroke::new(4.0));
        assert!(butt.commands.is_empty(), "butt paints no zero-length mark");
    }

    /// The dash pattern decides whether a zero-length subpath shows its mark:
    /// "on" at the subpath's position keeps the dot, "off" drops it.
    #[test]
    fn dashed_zero_length_subpath_follows_pattern_phase() {
        let dot = contour_path(&[Vec2::new(5.0, 5.0), Vec2::new(5.0, 5.0)], false);
        let dashed = |offset: f32, cap: LineCap| {
            stroke_to_fill(
                &dot,
                &Stroke {
                    cap,
                    dash: Some(DashPattern::new(vec![10.0, 10.0], offset)),
                    ..Stroke::new(4.0)
                },
            )
        };
        let on = dashed(0.0, LineCap::Round);
        assert_ne!(winding(&on, Vec2::new(5.8, 5.8)), 0, "on-phase dot kept");
        let off = dashed(10.0, LineCap::Round);
        assert!(off.commands.is_empty(), "off-phase dot dropped");
        let on_square = dashed(0.0, LineCap::Square);
        assert_ne!(
            winding(&on_square, Vec2::new(6.9, 6.9)),
            0,
            "square mark kept"
        );
    }

    /// Property: on a **solid closed** contour cap style is meaningless — the
    /// coverage must be bit-identical for butt/round/square across shapes,
    /// join styles, and path scales.
    #[test]
    fn prop_solid_closed_contours_are_cap_invariant() {
        let shapes: [&[(f32, f32)]; 2] = [
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
            &[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)],
        ];
        for shape in shapes {
            for s in [1e-3, 1.0, 1e3] {
                let path = contour_path(&vecs(shape, s), true);
                let probes = probe_grid(-6.0, 16.0, 22, s);
                for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
                    let sig = |cap: LineCap| {
                        let stroke = Stroke {
                            cap,
                            join,
                            width_px: 4.0 * s,
                            dash: None,
                        };
                        coverage_sig(&stroke_to_fill(&path, &stroke), &probes)
                    };
                    let butt = sig(LineCap::Butt);
                    assert!(butt.iter().any(|&b| b), "nothing painted ({s}, {join:?})");
                    for cap in [LineCap::Round, LineCap::Square] {
                        assert_eq!(
                            butt,
                            sig(cap),
                            "cap {cap:?} altered closed coverage (scale {s}, {join:?})"
                        );
                    }
                }
            }
        }
    }

    /// Property: no ghost geometry. Nothing may be painted farther than
    /// 2×width from the source contour (the worst legitimate reach is the
    /// miter limit, 4 × half-width) — across shapes, caps, joins, and dashes.
    #[test]
    fn prop_no_ghost_geometry_far_from_contour() {
        let shapes: [(&[(f32, f32)], bool); 4] = [
            (&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)], false),
            (&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], true),
            (&[(0.0, 0.0), (10.0, 0.0), (0.0, 0.0)], false),
            (&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)], true),
        ];
        let dashes: [Option<(Vec<f32>, f32)>; 3] = [
            None,
            Some((vec![7.0, 3.0], 5.0)),
            Some((vec![10.0, 5.0, 5.0, 5.0], 13.0)),
        ];
        let probes = probe_grid(-12.0, 22.0, 18, 1.0);
        for (shape, closed) in shapes {
            let pts = vecs(shape, 1.0);
            let path = contour_path(&pts, closed);
            for dash in &dashes {
                for cap in [LineCap::Butt, LineCap::Round, LineCap::Square] {
                    for join in [LineJoin::Miter, LineJoin::Round, LineJoin::Bevel] {
                        let stroke = Stroke {
                            cap,
                            join,
                            width_px: 4.0,
                            dash: dash.as_ref().map(|(i, o)| DashPattern::new(i.clone(), *o)),
                        };
                        let out = stroke_to_fill(&path, &stroke);
                        for &p in &probes {
                            if dist_to_contour(p, &pts, closed) > 8.2 {
                                assert_eq!(
                                    winding(&out, p),
                                    0,
                                    "ghost at {p:?} ({shape:?}, closed={closed}, \
                                     {cap:?}, {join:?}, dash={dash:?})"
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Property: open-contour endpoints carry exactly their cap shape, at any
    /// path scale. Probes past the end of a horizontal line distinguish butt
    /// (no extension), square (half-width box), and round (half-width disk).
    #[test]
    fn prop_open_endpoint_caps_match_style_across_scales() {
        for s in [1e-3, 1.0, 1e3] {
            let path = line(Vec2::ZERO, Vec2::new(10.0 * s, 0.0));
            // (probe, expected butt/square/round)
            let cases = [
                (Vec2::new(9.5 * s, 1.5 * s), [true, true, true]),
                (Vec2::new(10.5 * s, 0.0), [false, true, true]),
                (Vec2::new(11.9 * s, 1.9 * s), [false, true, false]),
                (Vec2::new(12.5 * s, 0.0), [false, false, false]),
            ];
            for (i, cap) in [LineCap::Butt, LineCap::Square, LineCap::Round]
                .into_iter()
                .enumerate()
            {
                let out = stroke_to_fill(
                    &path,
                    &Stroke {
                        cap,
                        width_px: 4.0 * s,
                        ..Stroke::new(0.0)
                    },
                );
                for (probe, expect) in cases {
                    assert_eq!(
                        winding(&out, probe) != 0,
                        expect[i],
                        "{cap:?} at {probe:?} (scale {s})"
                    );
                }
            }
        }
    }

    /// Property: on a closed contour, dash coverage is a function of arc
    /// length alone — re-cutting the ring at a different start vertex while
    /// compensating the dash offset by the start's arc distance must yield
    /// identical coverage. This pins the closing-seam handling to "the seam is
    /// an authoring artifact, never visible geometry".
    ///
    /// Precondition: the pattern total must divide the perimeter (40 here).
    /// Otherwise the cut point genuinely changes the dash layout — the walk is
    /// linear from the start point, exactly as in SVG — and no offset can
    /// compensate.
    #[test]
    fn prop_closed_dash_coverage_independent_of_start_vertex() {
        let verts = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let square_from = |k: usize| {
            let pts: Vec<Vec2> = (0..4)
                .map(|i| {
                    let (x, y) = verts[(i + k) % 4];
                    Vec2::new(x, y)
                })
                .collect();
            contour_path(&pts, true)
        };
        let probes = probe_grid(-4.0, 14.0, 20, 1.0);
        // Totals 20, 10, 20 — all divide the perimeter of 40.
        let patterns: [&[f32]; 3] = [&[10.0, 10.0], &[7.0, 3.0], &[10.0, 5.0, 3.0, 2.0]];
        for pattern in patterns {
            for offset in [0.0, 5.0, 13.0] {
                for cap in [LineCap::Butt, LineCap::Square] {
                    let stroke = |off: f32| Stroke {
                        cap,
                        dash: Some(DashPattern::new(pattern.to_vec(), off)),
                        ..Stroke::new(4.0)
                    };
                    let base =
                        coverage_sig(&stroke_to_fill(&square_from(0), &stroke(offset)), &probes);
                    for k in 1..4 {
                        let rot = coverage_sig(
                            &stroke_to_fill(&square_from(k), &stroke(offset + 10.0 * k as f32)),
                            &probes,
                        );
                        assert_eq!(
                            base, rot,
                            "start vertex {k} changed coverage \
                             (dash {pattern:?}, offset {offset}, {cap:?})"
                        );
                    }
                }
            }
        }
    }

    /// Property: redundant vertices are invisible — collinear midpoints and
    /// duplicate consecutive points must not change coverage, solid or dashed
    /// (they contribute no arc length and no join wedge).
    #[test]
    fn prop_redundant_vertices_are_invisible() {
        let plain = contour_path(
            &vecs(&[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], 1.0),
            true,
        );
        let midpoints = contour_path(
            &vecs(
                &[
                    (0.0, 0.0),
                    (5.0, 0.0),
                    (10.0, 0.0),
                    (10.0, 5.0),
                    (10.0, 10.0),
                    (5.0, 10.0),
                    (0.0, 10.0),
                    (0.0, 5.0),
                ],
                1.0,
            ),
            true,
        );
        let duplicates = contour_path(
            &vecs(
                &[
                    (0.0, 0.0),
                    (10.0, 0.0),
                    (10.0, 0.0),
                    (10.0, 10.0),
                    (10.0, 10.0),
                    (0.0, 10.0),
                ],
                1.0,
            ),
            true,
        );
        let probes = probe_grid(-4.0, 14.0, 20, 1.0);
        for dash in [None, Some(DashPattern::new(vec![7.0, 3.0], 5.0))] {
            for (cap, join) in [
                (LineCap::Butt, LineJoin::Miter),
                (LineCap::Round, LineJoin::Round),
                (LineCap::Square, LineJoin::Bevel),
            ] {
                let stroke = Stroke {
                    cap,
                    join,
                    width_px: 4.0,
                    dash: dash.clone(),
                };
                let base = coverage_sig(&stroke_to_fill(&plain, &stroke), &probes);
                assert!(base.iter().any(|&b| b));
                assert_eq!(
                    base,
                    coverage_sig(&stroke_to_fill(&midpoints, &stroke), &probes),
                    "collinear midpoints changed coverage ({cap:?}, {join:?}, dashed={})",
                    dash.is_some()
                );
                assert_eq!(
                    base,
                    coverage_sig(&stroke_to_fill(&duplicates, &stroke), &probes),
                    "duplicate vertices changed coverage ({cap:?}, {join:?}, dashed={})",
                    dash.is_some()
                );
            }
        }
    }

    /// Dash offsets are cyclic: negative offsets and offsets beyond the
    /// pattern total wrap (`rem_euclid`), and a shifted offset really shifts.
    #[test]
    fn dash_offset_wraps_negative_and_beyond_total() {
        let path = line(Vec2::ZERO, Vec2::new(30.0, 0.0));
        let mut probes = Vec::new();
        for i in 0..40 {
            probes.push(Vec2::new(-3.05 + i as f32 * 0.95, 0.45));
        }
        let sig = |offset: f32| {
            let stroke = Stroke {
                dash: Some(DashPattern::new(vec![10.0, 10.0], offset)),
                ..Stroke::new(4.0)
            };
            coverage_sig(&stroke_to_fill(&path, &stroke), &probes)
        };
        assert_eq!(sig(-5.0), sig(15.0), "negative offset wraps");
        assert_eq!(sig(45.0), sig(5.0), "offset beyond total wraps");
        assert_ne!(sig(5.0), sig(0.0), "offset actually shifts the pattern");
    }

    /// An invalid dash pattern (negative interval, empty, or all-zero) is not
    /// an error: it degrades to a solid stroke, matching SVG's invalid
    /// `stroke-dasharray` rule. A negative width paints nothing.
    #[test]
    fn invalid_dash_degrades_to_solid_and_negative_width_paints_nothing() {
        let path = line(Vec2::ZERO, Vec2::new(30.0, 0.0));
        let probes = probe_grid(-4.0, 34.0, 20, 1.0);
        let solid = coverage_sig(&stroke_to_fill(&path, &Stroke::new(4.0)), &probes);
        for intervals in [vec![10.0, -5.0], vec![], vec![0.0, 0.0]] {
            let stroke = Stroke {
                dash: Some(DashPattern::new(intervals.clone(), 3.0)),
                ..Stroke::new(4.0)
            };
            assert_eq!(
                solid,
                coverage_sig(&stroke_to_fill(&path, &stroke), &probes),
                "dash {intervals:?} should degrade to solid"
            );
        }
        assert!(stroke_to_fill(&path, &Stroke::new(-4.0))
            .commands
            .is_empty());
    }

    #[test]
    fn round_cap_on_a_zero_length_subpath_is_a_dot() {
        // A degenerate open subpath with a round cap paints a circular dot.
        let dot = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(5.0, 5.0)),
                PathCommand::LineTo(Vec2::new(5.0, 5.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let out = stroke_to_fill(
            &dot,
            &Stroke {
                cap: LineCap::Round,
                ..Stroke::new(4.0)
            },
        );
        assert!(!out.commands.is_empty());
        assert_ne!(winding(&out, Vec2::new(5.0, 5.0)), 0);
        assert_eq!(winding(&out, Vec2::new(5.0, 10.0)), 0);
    }
}
