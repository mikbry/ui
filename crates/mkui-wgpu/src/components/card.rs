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
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Stroke as PathStroke, Vec2, VectorPath};

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

/// #141 vector path: the rounded rect's fill plus its border stroke, pushed
/// as two separate `PlacedVectorPath`s (fill first, stroke on top, matching
/// `card_triangulated`'s fill-then-border-rects order) — the substrate
/// corners + border lane, in place of the triangulated approximation.
///
/// Codex round-1 review of #141: `PathStroke` centers the stroke on its
/// path's centerline, expanding `stroke_width / 2` on *both* sides. A stroke
/// centered on the fill's own outer boundary therefore painted half the
/// border outside the card's allocated rect — `card_triangulated`'s border
/// rects, by contrast, are drawn fully inside the rect (see
/// `tessellate_quad`). To match that inset footprint, the stroke is centered
/// on a *second*, smaller rounded rect — inset by `stroke_width / 2` on every
/// side, with the corner radius shrunk by the same amount (the concentric
/// offset of a rounded rect) — so the stroke's outer edge lands exactly on
/// the original rect's boundary and its inner edge lands `stroke_width`
/// inside it, the same band `card_triangulated`'s border rects cover.
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
    let inset = style.stroke_width * 0.5;
    let stroke_rect_width = rect.size.width - style.stroke_width;
    let stroke_rect_height = rect.size.height - style.stroke_width;
    if stroke_rect_width <= 0.0 || stroke_rect_height <= 0.0 {
        return;
    }
    let stroke_radius = (style.corner_radius - inset).max(0.0);
    let stroke_path = rounded_rect_path(stroke_rect_width, stroke_rect_height, stroke_radius);
    scene.vector_path(PlacedVectorPath {
        path: stroke_path,
        stroke: Some(PathStroke::new(style.stroke_width)),
        stroke_color: style.stroke,
        fill_color: None,
        origin_px: [rect.origin.x + inset, rect.origin.y + inset],
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

/// Build a clockwise (in screen space) rounded-rect outline in local path
/// space, placement is the caller's job via `PlacedVectorPath::origin_px`.
///
/// The Slug lane is y-up like a glyph's font units — the adapter projects
/// `screen_y = origin_px[1] - local_y * scale_px_per_unit` (`x` has no flip:
/// `screen_x = origin_px[0] + local_x * scale_px_per_unit`; see
/// `mkui_vector2d_wgpu`'s `dilated_quad_brackets_the_scaled_glyph_bounds`
/// test). So the rect's screen top-left (where `origin_px` is set, below) is
/// local `(0, 0)` and screen bottom-right is local `(width, -height)` —
/// `y` runs negative downward, the mirror of this crate's own y-down `Rect`.
///
/// Each corner is one cubic Bézier built from the standard circle-to-cubic
/// constant `KAPPA` (4/3 × (√2 − 1)), the same construction a 4-arc circle
/// approximation uses per quadrant — max radial error ≈0.027% of `radius`,
/// visually indistinguishable from a true arc even at Sprint 8's "large
/// radius" scale (§2.5). `radius` is clamped to half the shorter side so
/// opposing arcs never overlap. `radius <= 0.0` degenerates every corner arc
/// to a zero-length no-op, so this doubles as the square-corner case with no
/// special-casing.
#[cfg(feature = "slug")]
fn build_rounded_rect_path(width: f32, height: f32, radius: f32) -> VectorPath {
    /// 4/3 × (√2 − 1): distance (as a fraction of radius) from each arc's
    /// tangent point to its cubic control point.
    const KAPPA: f32 = 0.552_284_8;
    let r = radius.max(0.0).min(width.min(height) * 0.5);
    let rk = r * KAPPA;
    let commands = vec![
        PathCommand::MoveTo(Vec2::new(r, 0.0)),
        PathCommand::LineTo(Vec2::new(width - r, 0.0)),
        PathCommand::CubicTo {
            control1: Vec2::new(width - r + rk, 0.0),
            control2: Vec2::new(width, -(r - rk)),
            to: Vec2::new(width, -r),
        },
        PathCommand::LineTo(Vec2::new(width, -(height - r))),
        PathCommand::CubicTo {
            control1: Vec2::new(width, -(height - r + rk)),
            control2: Vec2::new(width - r + rk, -height),
            to: Vec2::new(width - r, -height),
        },
        PathCommand::LineTo(Vec2::new(r, -height)),
        PathCommand::CubicTo {
            control1: Vec2::new(r - rk, -height),
            control2: Vec2::new(0.0, -(height - r + rk)),
            to: Vec2::new(0.0, -(height - r)),
        },
        PathCommand::LineTo(Vec2::new(0.0, -r)),
        PathCommand::CubicTo {
            control1: Vec2::new(0.0, -(r - rk)),
            control2: Vec2::new(r - rk, 0.0),
            to: Vec2::new(r, 0.0),
        },
        PathCommand::Close,
    ];
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
    fn card_emits_a_fill_path_and_an_inset_stroke_path() {
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
            "card must emit a fill path and a separate inset stroke path"
        );

        let fill = paths[0];
        assert_eq!(fill.fill_color, Some(style.fill));
        assert!(fill.stroke.is_none(), "the fill primitive must not stroke");
        assert_eq!(fill.origin_px, [10.0, 5.0]);
        // Local path space is y-up (see `build_rounded_rect_path`'s docs):
        // screen top-left is local (0, 0), screen bottom-right is local
        // (width, -height). The fill path covers the card's full rect.
        assert_eq!(
            fill.path.bounds,
            Bounds::new(Vec2::new(0.0, -40.0), Vec2::new(120.0, 0.0))
        );

        let border = paths[1];
        assert!(
            border.fill_color.is_none(),
            "the stroke primitive must not also fill"
        );
        assert_eq!(border.stroke_color, style.stroke);
        let stroke = border.stroke.as_ref().expect("card border must stroke");
        assert_eq!(stroke.width_px, style.stroke_width);
        // Codex round-1 review of #141: a stroke centers on its path, so the
        // centerline path must be inset by half the stroke width on every
        // side — the stroke's outer edge then lands exactly on the fill's
        // outer edge instead of bleeding `stroke_width / 2` past it.
        let inset = style.stroke_width * 0.5;
        assert_eq!(border.origin_px, [10.0 + inset, 5.0 + inset]);
        assert_eq!(
            border.path.bounds,
            Bounds::new(
                Vec2::new(0.0, -(40.0 - style.stroke_width)),
                Vec2::new(120.0 - style.stroke_width, 0.0)
            )
        );
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

        assert_eq!(border.stroke_color, quad_stroke.color);
        let stroke = border.stroke.clone().expect("border must stroke");
        assert_eq!(stroke.width_px, quad_stroke.width);
        // `card_triangulated`'s border rects stay entirely within `quad.rect`
        // (see `tessellate_quad`); the vector border's stroke band must cover
        // the same footprint, not extend past it — the exact regression
        // Codex round-1 review of #141 caught. The border's inset centerline
        // path, offset by the half-stroke-width inset, achieves that; the
        // GPU test verifies the painted pixels directly.
        let inset = quad_stroke.width * 0.5;
        assert_eq!(
            border.origin_px,
            [quad.rect.origin.x + inset, quad.rect.origin.y + inset]
        );
        assert_eq!(
            border.path.bounds,
            Bounds::new(
                Vec2::new(0.0, -(quad.rect.size.height - quad_stroke.width)),
                Vec2::new(quad.rect.size.width - quad_stroke.width, 0.0)
            )
        );
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
}
