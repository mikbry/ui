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
fn flatten_contours(path: &VectorPath) -> Vec<Contour> {
    let mut out: Vec<Contour> = Vec::new();
    let mut cur: Vec<Vec2> = Vec::new();
    let mut start = Vec2::ZERO;
    let mut pen = Vec2::ZERO;

    let flush = |out: &mut Vec<Contour>, pts: &mut Vec<Vec2>, closed: bool| {
        if pts.len() >= 2 || (pts.len() == 1) {
            out.push(Contour {
                points: std::mem::take(pts),
                closed,
            });
        } else {
            pts.clear();
        }
    };

    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                flush(&mut out, &mut cur, false);
                start = p;
                pen = p;
                cur.push(p);
            }
            PathCommand::LineTo(p) => {
                push_point(&mut cur, p);
                pen = p;
            }
            PathCommand::QuadTo { control, to } => {
                flatten_quad(&mut cur, pen, control, to);
                pen = to;
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                let mut seg_start = pen;
                for (control, end) in subdivide_cubic(pen, control1, control2, to) {
                    flatten_quad(&mut cur, seg_start, control, end);
                    seg_start = end;
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
                pen = start;
                cur.push(start);
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
    if pts.len() < 2 {
        return Vec::new();
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

    // A degenerate contour (a single point, or all points coincident) paints a
    // dot only under a round cap.
    if segment_dirs(pts, contour.closed).is_empty() {
        if !contour.closed && cap == LineCap::Round {
            if let Some(&c) = pts.first() {
                out.push(circle(c, half));
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
        return; // Collinear: the two rectangles already meet flush.
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
