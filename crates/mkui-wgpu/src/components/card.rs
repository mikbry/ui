//! `card` — rounded-rect surface chrome.
//!
//! The shadcn "Card" primitive (minus the typography slot): a rounded rect
//! with fill + stroke and no shadow. Use it for inline list rows and inspector
//! row backgrounds; the style resolves through [`CardStyle`].
//!
//! Sprint 8 Wave 2 (#141): under the `slug` feature, the rounded rect + border
//! draws as a single [`crate::types::PlacedVectorPath`] (fill + stroke) on the
//! Slug coverage lane instead of being tessellated to triangles — the
//! tessellator never respects `Quad::corner_radii` (see
//! `tessellation::tessellate_quad`), so the pre-#141 "rounded" card was in fact
//! a square-cornered approximation. With the feature off, `card` is
//! byte-identical to v0.10.0: same `Quad` push, same corner-radii field (still
//! unused by the triangulator, preserved only for API parity).

use crate::theme::CardStyle;
use crate::types::{Rect, Scene};

#[cfg(any(test, not(feature = "slug")))]
use crate::types::{CornerRadii, Quad, Stroke};

#[cfg(feature = "slug")]
use std::collections::HashMap;
#[cfg(feature = "slug")]
use std::sync::{Arc, Mutex, OnceLock};

#[cfg(feature = "slug")]
use crate::types::PlacedVectorPath;
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Vec2, VectorPath};

/// Card chrome — rounded rect with fill, stroke, no shadow. The shadcn
/// "Card" primitive (minus the typography slot). Use this for inline list
/// rows and inspector row backgrounds.
pub fn card(scene: &mut Scene, rect: Rect, style: CardStyle) {
    #[cfg(feature = "slug")]
    {
        card_vector(scene, rect, &style);
    }
    #[cfg(not(feature = "slug"))]
    {
        card_triangulated(scene, rect, &style);
    }
}

/// Pre-#141 triangulated path: a plain `Quad` push, unchanged. Compiled
/// whenever `slug` is off (production use via [`card`]) *or* under
/// `cfg(test)` (the `slug`-feature cross-consumer test below calls it
/// alongside [`card_vector`] to compare both lanes in one binary) — never
/// both reasons live at once with `slug` on outside tests, so this stays
/// dead-code-clean without an `allow`.
#[cfg(any(test, not(feature = "slug")))]
fn card_triangulated(scene: &mut Scene, rect: Rect, style: &CardStyle) {
    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        }),
    });
}

/// #141 vector path: the rounded rect's fill plus its border, pushed as two
/// separate `PlacedVectorPath`s (fill first, border on top, matching
/// `card_triangulated`'s fill-then-border-rects order) — the substrate
/// corners + border lane, in place of the triangulated approximation.
///
/// The border is *filled* geometry (an outer-minus-inner annulus, built by
/// [`build_rounded_rect_border_path`]), not a [`mkui_vector2d_wgpu::Stroke`]
/// centered on a path. Codex round-1 review of #141 found that a centered
/// stroke on the fill's own outer boundary paints half the border outside
/// the card's allocated rect; round-2 review found that re-centering the
/// stroke on an *inset* centerline instead (this function's first fix)
/// introduces its own two edge cases — the border vanishing once the inset
/// collapses the centerline rect to nothing, and a reduced, non-matching
/// outer radius once the inset shrinks the centerline's radius below zero.
/// Building the border directly as an annulus — whose *outer* contour is
/// always the fill's own unmodified outline — sidesteps both: see
/// [`build_rounded_rect_border_path`]'s docs.
#[cfg(feature = "slug")]
fn card_vector(scene: &mut Scene, rect: Rect, style: &CardStyle) {
    let fill_path = rounded_rect_path(rect.size.width, rect.size.height, style.corner_radius);
    scene.vector_path(PlacedVectorPath {
        path: fill_path,
        stroke: None,
        stroke_color: style.fill,
        fill_color: Some(style.fill),
        origin_px: [rect.origin.x, rect.origin.y],
        scale_px_per_unit: 1.0,
    });

    if style.stroke_width <= 0.0 {
        return;
    }
    let border_path = rounded_rect_border_path(
        rect.size.width,
        rect.size.height,
        style.corner_radius,
        style.stroke_width,
    );
    scene.vector_path(PlacedVectorPath {
        path: border_path,
        stroke: None,
        stroke_color: style.stroke,
        fill_color: Some(style.stroke),
        origin_px: [rect.origin.x, rect.origin.y],
        scale_px_per_unit: 1.0,
    });
}

/// Cache of rounded-rect path geometry keyed by `(width, height, radius)` bit
/// patterns. A theme hands out a small, fixed set of card sizes/radii, so
/// after warm-up every `card()` call hits this map and only bumps an `Arc`
/// refcount — no `Vec<PathCommand>` allocation on the steady-state hot path,
/// matching [`crate::types::PlacedVectorPath::path`]'s `Arc`-sharing contract.
/// `(width_bits, height_bits, radius_bits)` cache key -> path.
#[cfg(feature = "slug")]
type RoundedRectCache = Mutex<HashMap<(u32, u32, u32), Arc<VectorPath>>>;

#[cfg(feature = "slug")]
static ROUNDED_RECT_CACHE: OnceLock<RoundedRectCache> = OnceLock::new();

/// Cap on distinct `(width, height, radius)` entries. A live-resize (card
/// width tracking a dragged panel, say) would otherwise grow this map by one
/// entry per distinct pixel width forever — a slow leak, not the small,
/// steady theme-driven set the cache is sized for. Cleared wholesale rather
/// than evicted LRU-style: simplest fix, and a cache this is sized for is
/// always cold again within a frame or two.
#[cfg(feature = "slug")]
const ROUNDED_RECT_CACHE_CAP: usize = 64;

#[cfg(feature = "slug")]
fn rounded_rect_path(width: f32, height: f32, radius: f32) -> Arc<VectorPath> {
    let key = (width.to_bits(), height.to_bits(), radius.to_bits());
    let cache = ROUNDED_RECT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("rounded-rect cache poisoned");
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    if cache.len() >= ROUNDED_RECT_CACHE_CAP {
        cache.clear();
    }
    let path = Arc::new(build_rounded_rect_path(width, height, radius));
    cache.insert(key, path.clone());
    path
}

/// Cache of border-annulus path geometry, parallel to [`ROUNDED_RECT_CACHE`]
/// but keyed on the extra `stroke_width` dimension.
/// `(width_bits, height_bits, radius_bits, stroke_width_bits)` -> path.
#[cfg(feature = "slug")]
type RoundedRectBorderCache = Mutex<HashMap<(u32, u32, u32, u32), Arc<VectorPath>>>;

#[cfg(feature = "slug")]
static ROUNDED_RECT_BORDER_CACHE: OnceLock<RoundedRectBorderCache> = OnceLock::new();

#[cfg(feature = "slug")]
fn rounded_rect_border_path(
    width: f32,
    height: f32,
    radius: f32,
    stroke_width: f32,
) -> Arc<VectorPath> {
    let key = (
        width.to_bits(),
        height.to_bits(),
        radius.to_bits(),
        stroke_width.to_bits(),
    );
    let cache = ROUNDED_RECT_BORDER_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut cache = cache.lock().expect("rounded-rect border cache poisoned");
    if let Some(cached) = cache.get(&key) {
        return cached.clone();
    }
    if cache.len() >= ROUNDED_RECT_CACHE_CAP {
        cache.clear();
    }
    let path = Arc::new(build_rounded_rect_border_path(
        width,
        height,
        radius,
        stroke_width,
    ));
    cache.insert(key, path.clone());
    path
}

/// 4/3 × (√2 − 1): distance (as a fraction of radius) from a circular arc's
/// tangent point to its cubic-Bézier control point — the standard
/// circle-to-cubic constant, shared by every rounded-rect contour this
/// module builds.
#[cfg(feature = "slug")]
const KAPPA: f32 = 0.552_284_8;

/// Build one clockwise (in screen space) rounded-rect contour — `MoveTo` +
/// line/cubic segments + `Close` — in local path space, shifted by `offset`.
/// Shared by [`build_rounded_rect_path`] (the fill, `offset` always zero)
/// and [`build_rounded_rect_border_path`] (both the border's outer contour,
/// `offset` zero, and its inner contour, `offset` the border inset).
///
/// The Slug lane is y-up like a glyph's font units — the adapter projects
/// `screen_y = origin_px[1] - local_y * scale_px_per_unit` (`x` has no flip:
/// `screen_x = origin_px[0] + local_x * scale_px_per_unit`; see
/// `mkui_vector2d_wgpu`'s `dilated_quad_brackets_the_scaled_glyph_bounds`
/// test). So a rect's screen top-left is local `(0, 0)` + `offset`, and
/// screen bottom-right is local `(width, -height)` + `offset` — `y` runs
/// negative downward, the mirror of this crate's own y-down `Rect`.
///
/// Each corner is one cubic Bézier built from [`KAPPA`], the same
/// construction a 4-arc circle approximation uses per quadrant — max radial
/// error ≈0.027% of `radius`, visually indistinguishable from a true arc
/// even at Sprint 8's "large radius" scale (§2.5). `radius` is clamped to
/// half the shorter side so opposing arcs never overlap. `radius <= 0.0` (or
/// `width`/`height` `<= 0.0`) degenerates every corner arc, and in the
/// zero-size case every segment, to a zero-length no-op — this doubles as
/// both the square-corner case and the fully-degenerate "no contour at all"
/// case with no special-casing (the encoder drops a degenerate segment on
/// its own, see `mkui_vector2d::encode::flatten_path_curves`).
#[cfg(feature = "slug")]
fn rounded_rect_contour(width: f32, height: f32, radius: f32, offset: Vec2) -> Vec<PathCommand> {
    let width = width.max(0.0);
    let height = height.max(0.0);
    let r = radius.max(0.0).min(width.min(height) * 0.5);
    let rk = r * KAPPA;
    let p = |x: f32, y: f32| Vec2::new(x + offset.x, y + offset.y);
    vec![
        PathCommand::MoveTo(p(r, 0.0)),
        PathCommand::LineTo(p(width - r, 0.0)),
        PathCommand::CubicTo {
            control1: p(width - r + rk, 0.0),
            control2: p(width, -(r - rk)),
            to: p(width, -r),
        },
        PathCommand::LineTo(p(width, -(height - r))),
        PathCommand::CubicTo {
            control1: p(width, -(height - r + rk)),
            control2: p(width - r + rk, -height),
            to: p(width - r, -height),
        },
        PathCommand::LineTo(p(r, -height)),
        PathCommand::CubicTo {
            control1: p(r - rk, -height),
            control2: p(0.0, -(height - r + rk)),
            to: p(0.0, -(height - r)),
        },
        PathCommand::LineTo(p(0.0, -r)),
        PathCommand::CubicTo {
            control1: p(0.0, -(r - rk)),
            control2: p(r - rk, 0.0),
            to: p(r, 0.0),
        },
        PathCommand::Close,
    ]
}

/// Build a rounded-rect outline in local path space — see
/// [`rounded_rect_contour`] for the coordinate convention and corner
/// construction this wraps (`offset` zero).
#[cfg(feature = "slug")]
fn build_rounded_rect_path(width: f32, height: f32, radius: f32) -> VectorPath {
    VectorPath::new(
        rounded_rect_contour(width, height, radius, Vec2::ZERO),
        FillRule::NonZero,
        Bounds::new(Vec2::new(0.0, -height), Vec2::new(width, 0.0)),
    )
}

/// Reverse a single closed contour's winding direction: decompose it into
/// `(from, segment)` pairs by walking the pen position, then replay them in
/// reverse order with each segment's own direction flipped (a reversed
/// `QuadTo`/`CubicTo` keeps its control point(s) — swapped for a cubic —
/// just runs from its old `to` back to its old `from`).
///
/// Pairing a contour with its reverse under [`FillRule::NonZero`] is the
/// TrueType/PostScript "glyph with a hole" convention `path.rs` already
/// documents (e.g. the letter "O": an outer contour one way, an inner
/// contour the other) — [`build_rounded_rect_border_path`] uses it to punch
/// the fill-shaped hole out of the border's outer silhouette.
#[cfg(feature = "slug")]
fn reverse_contour(commands: &[PathCommand]) -> Vec<PathCommand> {
    enum Seg {
        Line,
        Quad(Vec2),
        Cubic(Vec2, Vec2),
    }
    let mut start = Vec2::ZERO;
    let mut cur = Vec2::ZERO;
    let mut segments: Vec<(Vec2, Seg, Vec2)> = Vec::new();
    for cmd in commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                start = p;
                cur = p;
            }
            PathCommand::LineTo(p) => {
                segments.push((cur, Seg::Line, p));
                cur = p;
            }
            PathCommand::QuadTo { control, to } => {
                segments.push((cur, Seg::Quad(control), to));
                cur = to;
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                segments.push((cur, Seg::Cubic(control1, control2), to));
                cur = to;
            }
            PathCommand::Close => {
                if cur != start {
                    segments.push((cur, Seg::Line, start));
                }
                cur = start;
            }
            // `PathCommand` is `#[non_exhaustive]`; every variant it has
            // today is handled above.
            other => unreachable!("reverse_contour: unhandled PathCommand variant {other:?}"),
        }
    }
    let mut out = vec![PathCommand::MoveTo(start)];
    for (from, seg, _to) in segments.into_iter().rev() {
        out.push(match seg {
            Seg::Line => PathCommand::LineTo(from),
            Seg::Quad(control) => PathCommand::QuadTo { control, to: from },
            Seg::Cubic(control1, control2) => PathCommand::CubicTo {
                control1: control2,
                control2: control1,
                to: from,
            },
        });
    }
    out.push(PathCommand::Close);
    out
}

/// Build the card border as a single filled annulus — the fill's own outer
/// contour, plus a smaller inner contour (the border's inner edge) wound the
/// opposite direction so [`FillRule::NonZero`] punches it out as a hole —
/// rather than as a [`mkui_vector2d_wgpu::Stroke`] centered on some
/// centerline path.
///
/// Codex round-2 review of #141 found that centering a stroke on an *inset*
/// centerline (this function's predecessor) breaks in two edge cases: the
/// centerline rect degenerates to nothing once `stroke_width` reaches either
/// dimension (vanishing the border instead of covering the remaining
/// interior), and the centerline's own radius clamps to zero once
/// `stroke_width >= 2 * radius` (shrinking the *outer* silhouette the stroke
/// paints to a tighter, square-cornered radius than the fill's). Building
/// the border as an explicit outer-minus-inner shape fixes both at the root:
///
/// - **Outer contour** is `rounded_rect_contour(width, height, radius, ..)`
///   — byte-identical to the fill's own contour, unconditionally. The
///   border's outer silhouette can never mismatch the fill's, at any
///   `stroke_width`.
/// - **Inner contour** is inset by `stroke_width` on every side, with its
///   own radius reduced by `stroke_width` (the concentric offset of a
///   rounded rect), both floored at `0.0`. When the inset would consume the
///   whole rect, `rounded_rect_contour` degenerates it to a zero-area
///   no-op contour (see its own docs) — contributing no hole at all, so the
///   annulus covers the *entire* outer shape: the "cover the remaining
///   interior instead of vanishing" fix.
#[cfg(feature = "slug")]
fn build_rounded_rect_border_path(
    width: f32,
    height: f32,
    radius: f32,
    stroke_width: f32,
) -> VectorPath {
    let stroke_width = stroke_width.max(0.0);
    let mut commands = rounded_rect_contour(width, height, radius, Vec2::ZERO);
    let inner_width = width - 2.0 * stroke_width;
    let inner_height = height - 2.0 * stroke_width;
    let inner_radius = (radius - stroke_width).max(0.0);
    let inner = rounded_rect_contour(
        inner_width,
        inner_height,
        inner_radius,
        Vec2::new(stroke_width, -stroke_width),
    );
    commands.extend(reverse_contour(&inner));
    VectorPath::new(
        commands,
        FillRule::NonZero,
        Bounds::new(Vec2::new(0.0, -height), Vec2::new(width, 0.0)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Point, Primitive, Size};

    #[cfg(not(feature = "slug"))]
    #[test]
    fn card_emits_a_single_filled_quad() {
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let theme = WgpuTheme::default();
        card(
            &mut scene,
            Rect::new(Point::new(0.0, 0.0), Size::new(120.0, 40.0)),
            theme.card(),
        );
        let quads: Vec<_> = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .collect();
        assert_eq!(quads.len(), 1);
    }

    #[cfg(feature = "slug")]
    #[test]
    fn card_emits_a_fill_path_and_a_border_annulus_path() {
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let theme = WgpuTheme::default();
        let style = theme.card();
        card(
            &mut scene,
            Rect::new(Point::new(10.0, 5.0), Size::new(120.0, 40.0)),
            style,
        );
        let paths: Vec<_> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::VectorPath(placed) => Some(placed),
                _ => None,
            })
            .collect();
        assert_eq!(
            paths.len(),
            2,
            "card must emit a fill path and a separate border annulus path"
        );

        let fill = paths[0];
        assert_eq!(fill.fill_color, Some(style.fill));
        assert!(fill.stroke.is_none(), "the fill primitive must not stroke");
        assert_eq!(fill.origin_px, [10.0, 5.0]);
        // Local path space is y-up (see `rounded_rect_contour`'s docs):
        // screen top-left is local (0, 0), screen bottom-right is local
        // (width, -height). The fill path covers the card's full rect.
        assert_eq!(
            fill.path.bounds,
            Bounds::new(Vec2::new(0.0, -40.0), Vec2::new(120.0, 0.0))
        );

        let border = paths[1];
        assert_eq!(
            border.fill_color,
            Some(style.stroke),
            "the border annulus paints directly with fill_color, not a Stroke"
        );
        assert!(
            border.stroke.is_none(),
            "the border annulus must not also carry a centered Stroke (#141 round-2 fix)"
        );
        // Codex round-2 review of #141: the border's outer contour is the
        // fill's own unmodified outline, so its placement and overall
        // bounds are identical to the fill's — only the path's *interior*
        // differs (it carries an inner hole).
        assert_eq!(border.origin_px, fill.origin_px);
        assert_eq!(border.path.bounds, fill.path.bounds);
    }

    #[cfg(feature = "slug")]
    #[test]
    fn card_emits_no_quad_when_slug_enabled() {
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let theme = WgpuTheme::default();
        card(
            &mut scene,
            Rect::new(Point::new(0.0, 0.0), Size::new(120.0, 40.0)),
            theme.card(),
        );
        assert!(
            !scene
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Quad(_))),
            "slug-enabled card must not fall back to a triangulated Quad"
        );
    }

    /// Paint-data parity between the two lane builders: fill color, stroke
    /// color/width, and the rect each lane's border stays within must agree
    /// between `card_triangulated` and `card_vector` for the same
    /// `(rect, style)`, so a theme author sees the same visual result
    /// switching the `slug` feature, modulo "rounded" vs. "square" corners
    /// (the bug #141 fixes). This is a structural, CPU-only check; the real
    /// pixel-level cross-consumer test named in the issue (Sprint 8 plan v2
    /// §4.3) is
    /// [`crate::render::card_vector_gpu::card_vector_vs_triangulated`], which
    /// renders both lanes on the Lavapipe GPU harness and reads back pixels —
    /// `cargo test -p mkui-wgpu --features 'gpu-tests slug' card_vector_vs_triangulated`.
    #[cfg(feature = "slug")]
    #[test]
    fn card_vector_and_triangulated_resolve_the_same_paint_data() {
        let theme = WgpuTheme::default();
        let style = theme.card();
        let rect = Rect::new(Point::new(4.0, 6.0), Size::new(96.0, 32.0));

        let mut triangulated_scene = Scene::new(Size::new(200.0, 80.0));
        card_triangulated(&mut triangulated_scene, rect, &style);
        let quad = triangulated_scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) => Some(*q),
                _ => None,
            })
            .expect("triangulated lane must emit a Quad");
        let quad_stroke = quad.stroke.expect("card always strokes a border");

        let mut vector_scene = Scene::new(Size::new(200.0, 80.0));
        card_vector(&mut vector_scene, rect, &style);
        let mut vector_paths = vector_scene.primitives.iter().filter_map(|p| match p {
            Primitive::VectorPath(placed) => Some(placed.clone()),
            _ => None,
        });
        let fill = vector_paths.next().expect("vector lane must emit a fill");
        let border = vector_paths.next().expect("vector lane must emit a border");

        assert_eq!(fill.fill_color, Some(quad.fill));
        assert_eq!(fill.origin_px, [quad.rect.origin.x, quad.rect.origin.y]);
        assert_eq!(
            fill.path.bounds,
            Bounds::new(
                Vec2::new(0.0, -quad.rect.size.height),
                Vec2::new(quad.rect.size.width, 0.0)
            )
        );

        assert_eq!(border.fill_color, Some(quad_stroke.color));
        assert!(border.stroke.is_none());
        // `card_triangulated`'s border rects stay entirely within `quad.rect`
        // (see `tessellate_quad`); the vector border annulus's outer contour
        // — identical to the fill's — covers the same footprint, never more
        // (the regression Codex round-1 review of #141 caught) and never
        // less, regardless of `stroke_width` (the two further edge cases
        // Codex round-2 review caught). The GPU test verifies the painted
        // pixels directly, including both edge cases.
        assert_eq!(border.origin_px, fill.origin_px);
        assert_eq!(border.path.bounds, fill.path.bounds);
    }

    #[cfg(feature = "slug")]
    #[test]
    fn rounded_rect_path_is_cached_across_calls() {
        let a = rounded_rect_path(80.0, 32.0, 8.0);
        let b = rounded_rect_path(80.0, 32.0, 8.0);
        assert!(
            Arc::ptr_eq(&a, &b),
            "same (width, height, radius) must hit the cache, not rebuild"
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn zero_radius_degenerates_to_a_plain_rect_bounds() {
        let path = build_rounded_rect_path(50.0, 20.0, 0.0);
        assert_eq!(
            path.bounds,
            Bounds::new(Vec2::new(0.0, -20.0), Vec2::new(50.0, 0.0))
        );
        assert!(!path.is_empty());
    }

    #[cfg(feature = "slug")]
    #[test]
    fn reverse_contour_flips_winding_direction() {
        // A simple unit square traced with only line segments, so winding
        // direction reduces to the textbook shoelace-formula signed area.
        let square = vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::LineTo(Vec2::new(1.0, 0.0)),
            PathCommand::LineTo(Vec2::new(1.0, 1.0)),
            PathCommand::LineTo(Vec2::new(0.0, 1.0)),
            PathCommand::Close,
        ];
        let reversed = reverse_contour(&square);

        fn signed_area(commands: &[PathCommand]) -> f32 {
            let mut points = Vec::new();
            for cmd in commands {
                match *cmd {
                    PathCommand::MoveTo(p) | PathCommand::LineTo(p) => points.push(p),
                    PathCommand::Close => {}
                    other => unreachable!("test only emits MoveTo/LineTo/Close, got {other:?}"),
                }
            }
            let mut area = 0.0;
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                area += a.x * b.y - b.x * a.y;
            }
            area * 0.5
        }

        let forward_area = signed_area(&square);
        let reversed_area = signed_area(&reversed);
        assert!(
            forward_area.abs() > 0.0,
            "the test square must have nonzero area"
        );
        assert_eq!(
            reversed_area, -forward_area,
            "reversing a contour must flip its signed area (winding direction)"
        );
        assert_eq!(
            reversed.first(),
            Some(&PathCommand::MoveTo(Vec2::new(0.0, 0.0))),
            "the reversed contour starts at the same point as the original"
        );
    }

    /// Codex round-2 review of #141: a centerline-inset border's own radius
    /// clamps to zero once `stroke_width >= 2 * radius`, producing a
    /// tighter, square-cornered outer silhouette than the fill's — painting
    /// outside the fill's rounded corner (confirmed with radius 4, stroke
    /// width 8). The border's outer contour must be byte-identical to the
    /// fill's own contour, unconditionally — never reduced by `stroke_width`.
    #[cfg(feature = "slug")]
    #[test]
    fn border_outer_contour_matches_fill_outline_regardless_of_stroke_width() {
        let (width, height, radius) = (40.0, 40.0, 4.0);
        let fill_contour = rounded_rect_contour(width, height, radius, Vec2::ZERO);
        for stroke_width in [0.5, 2.0, 4.0, 8.0, 100.0] {
            let border = build_rounded_rect_border_path(width, height, radius, stroke_width);
            assert_eq!(
                border.commands[..fill_contour.len()],
                fill_contour[..],
                "the border's outer contour must match the fill's exactly at stroke_width={stroke_width}"
            );
        }
    }

    /// Codex round-2 review of #141: a border whose `stroke_width` reaches
    /// (or exceeds) the card's shorter dimension must not vanish — it must
    /// still paint, covering the whole interior (confirmed with a 32×8 card
    /// and an 8px border). This is a CPU-only well-formedness check; the GPU
    /// test (`card_vector_vs_triangulated`) verifies the rendered result
    /// actually covers the interior, not just that construction succeeds.
    #[cfg(feature = "slug")]
    #[test]
    fn border_path_is_well_formed_when_stroke_width_consumes_the_shorter_dimension() {
        let border = build_rounded_rect_border_path(32.0, 8.0, 2.0, 8.0);
        assert!(
            !border.is_empty(),
            "the border's outer contour must still paint when the inset collapses"
        );
        assert_eq!(
            border.bounds,
            Bounds::new(Vec2::new(0.0, -8.0), Vec2::new(32.0, 0.0))
        );
    }
}
