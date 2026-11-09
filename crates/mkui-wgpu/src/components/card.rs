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

/// #141 vector path: one `PlacedVectorPath` carrying the rounded rect's fill
/// *and* its border stroke — the substrate corners + border lane, in place of
/// the triangulated approximation.
#[cfg(feature = "slug")]
fn card_vector(scene: &mut Scene, rect: Rect, style: &CardStyle) {
    let path = rounded_rect_path(rect.size.width, rect.size.height, style.corner_radius);
    scene.vector_path(PlacedVectorPath {
        path,
        stroke: Some(PathStroke::new(style.stroke_width)),
        stroke_color: style.stroke,
        fill_color: Some(style.fill),
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
    fn card_emits_a_single_vector_path_with_fill_and_stroke() {
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
        assert_eq!(paths.len(), 1, "card must emit exactly one vector path");
        let placed = paths[0];
        assert_eq!(placed.fill_color, Some(style.fill));
        assert_eq!(placed.stroke_color, style.stroke);
        let stroke = placed.stroke.as_ref().expect("card border must stroke");
        assert_eq!(stroke.width_px, style.stroke_width);
        assert_eq!(placed.origin_px, [10.0, 5.0]);
        assert_eq!(placed.scale_px_per_unit, 1.0);
        // Local path space is y-up (see `build_rounded_rect_path`'s docs):
        // screen top-left is local (0, 0), screen bottom-right is local
        // (width, -height).
        assert_eq!(
            placed.path.bounds,
            Bounds::new(Vec2::new(0.0, -40.0), Vec2::new(120.0, 0.0))
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
    /// color/width, and placement must agree between `card_triangulated` and
    /// `card_vector` for the same `(rect, style)`, so a theme author sees the
    /// same visual result switching the `slug` feature, modulo "rounded" vs.
    /// "square" corners (the bug #141 fixes). This is a structural, CPU-only
    /// check; the real pixel-level cross-consumer test named in the issue
    /// (Sprint 8 plan v2 §4.3) is
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

        let mut vector_scene = Scene::new(Size::new(200.0, 80.0));
        card_vector(&mut vector_scene, rect, &style);
        let placed = vector_scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::VectorPath(placed) => Some(placed.clone()),
                _ => None,
            })
            .expect("vector lane must emit a PlacedVectorPath");

        assert_eq!(placed.fill_color, Some(quad.fill));
        assert_eq!(placed.stroke_color, quad.stroke.unwrap().color);
        assert_eq!(placed.stroke.unwrap().width_px, quad.stroke.unwrap().width);
        assert_eq!(placed.origin_px, [quad.rect.origin.x, quad.rect.origin.y]);
        assert_eq!(
            placed.path.bounds,
            Bounds::new(
                Vec2::new(0.0, -quad.rect.size.height),
                Vec2::new(quad.rect.size.width, 0.0)
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
