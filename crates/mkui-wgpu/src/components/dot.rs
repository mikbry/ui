//! Generic status `Dot` atom.
//!
//! Tiny coloured pip used as a status pip on table rows and inline state
//! signals. Variant is one of the four status-colour tokens (`Ok`, `Warn`,
//! `Danger`, `Neutral`) — what each means in a given product is decided
//! downstream. An optional `halo` ring at 1.5× diameter and an optional
//! `animation` enum (`None | Pulse | PulseUrgent | Spin`) act as generic
//! visual modifiers; the animation lives on [`Scene::animations`] for the
//! renderer to interpret per-frame.
//!
//! **Circle authoring (#142, Sprint 8 Wave 2).** With the `slug` feature on,
//! the body (and halo, when present) paint as a true vector circle —
//! `Primitive::VectorPath`, 4 cubic Bézier arcs around a unit circle, placed
//! by translating to the dot's centre and scaling by its radius — instead
//! of the quad this atom drew through v0.10.0. Off-feature builds keep the
//! exact v0.10.0 quad path (same primitives, same order), so there is zero
//! public API or behavior change without `slug`. The unit-circle geometry is
//! built once into a process-wide `Arc`-shared path and cloned (a refcount
//! bump, no allocation) on every call.

use crate::theme::{DotSize, DotVariant, WgpuTheme};
use crate::types::{Color, DotAnimation, DotAnimationInstance, Point, Scene};

#[cfg(not(feature = "slug"))]
use crate::types::{CornerRadii, Quad, Rect, Size};

#[cfg(feature = "slug")]
use crate::types::PlacedVectorPath;
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
#[cfg(feature = "slug")]
use std::sync::{Arc, LazyLock};

/// The standard 4-arc circle approximation constant, `4/3 * (sqrt(2) - 1)`.
/// Accurate to within ~0.027% of the radius — far tighter than any UI dot's
/// visible error budget.
#[cfg(feature = "slug")]
const CIRCLE_KAPPA: f32 = 0.552_284_8;

/// Build the unit circle — radius 1, centred at the origin — as 4 cubic
/// Bézier arcs (issue #142's circle-construction recipe; no `LineTo`
/// segments, so the boundary is a true curve, not a polygon). The Sprint 7
/// Slug *glyph* encoder rejects cubic segments, but the general
/// `encode_vector_path` encoder this lane runs through (#137) subdivides
/// cubics to quadratics internally, so this needs no local flattening.
#[cfg(feature = "slug")]
fn unit_circle_path() -> VectorPath {
    let k = CIRCLE_KAPPA;
    VectorPath::new(
        vec![
            PathCommand::MoveTo(Vec2::new(1.0, 0.0)),
            PathCommand::CubicTo {
                control1: Vec2::new(1.0, k),
                control2: Vec2::new(k, 1.0),
                to: Vec2::new(0.0, 1.0),
            },
            PathCommand::CubicTo {
                control1: Vec2::new(-k, 1.0),
                control2: Vec2::new(-1.0, k),
                to: Vec2::new(-1.0, 0.0),
            },
            PathCommand::CubicTo {
                control1: Vec2::new(-1.0, -k),
                control2: Vec2::new(-k, -1.0),
                to: Vec2::new(0.0, -1.0),
            },
            PathCommand::CubicTo {
                control1: Vec2::new(k, -1.0),
                control2: Vec2::new(1.0, -k),
                to: Vec2::new(1.0, 0.0),
            },
            PathCommand::Close,
        ],
        FillRule::NonZero,
        Bounds::new(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0)),
    )
}

/// Process-wide cache of [`unit_circle_path`]'s geometry. Every dot/halo
/// circle shares this one `Arc<VectorPath>` — placement (`origin_px` /
/// `scale_px_per_unit`) carries the dot's actual centre/radius, so the same
/// shared geometry serves every variant/size/halo combination. Built once on
/// first use; every subsequent call only bumps the `Arc` refcount — the
/// issue's "allocation-free hot path" criterion.
#[cfg(feature = "slug")]
static UNIT_CIRCLE: LazyLock<Arc<VectorPath>> = LazyLock::new(|| Arc::new(unit_circle_path()));

/// Emit one filled circle of `radius` centred at `center` (#142). A true
/// `Primitive::VectorPath`: placement maps the shared unit circle's local
/// `(0, 0)` to `center` in pixels and scales it by `radius`. No stroke — a
/// dot is a filled shape by default.
#[cfg(feature = "slug")]
fn emit_circle(scene: &mut Scene, center: Point, radius: f32, fill: Color) {
    scene.vector_path(PlacedVectorPath {
        path: Arc::clone(&UNIT_CIRCLE),
        stroke: None,
        // Ignored: `stroke` is `None`, so no stroke paint is ever read.
        stroke_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
        fill_color: Some(fill),
        origin_px: [center.x, center.y],
        scale_px_per_unit: radius,
    });
}

/// Emit one filled circle of `radius` centred at `center`. Off the `slug`
/// feature this is a quad with `corner_radii` set to the full radius — since
/// #212, the native tessellator facets that into a many-sided polygon
/// approximating a true circle (flat, hard edges; no per-fragment AA,
/// unlike the `slug` lane's circle).
#[cfg(not(feature = "slug"))]
fn emit_circle(scene: &mut Scene, center: Point, radius: f32, fill: Color) {
    let origin = Point::new(center.x - radius, center.y - radius);
    scene.quad(Quad {
        rect: Rect::new(origin, Size::new(radius * 2.0, radius * 2.0)),
        fill,
        corner_radii: CornerRadii::all(radius),
        stroke: None,
    });
}

/// Emit a dot into `scene` centred at `point`. The atom paints the pip
/// (and optional halo) as circles — vector paths under the `slug` feature,
/// quads otherwise (see the module docs) — and pushes a single
/// [`DotAnimationInstance`] into `scene.animations` when `animation` is
/// non-`None`.
// allow: signature parallels `badge` — `variant` / `size` resolve through
// the theme, then the generic visual modifiers (`halo`, `animation`) follow.
// Bundling them into a param-struct would split the atom layer's call shape
// without clarity gain.
#[allow(clippy::too_many_arguments)]
pub fn dot(
    scene: &mut Scene,
    point: Point,
    variant: DotVariant,
    size: DotSize,
    halo: bool,
    animation: DotAnimation,
    theme: &WgpuTheme,
) {
    let style = theme.dot_style(variant, size);
    let radius = style.diameter * 0.5;

    if halo {
        let halo_diameter = style.diameter * 1.5;
        emit_circle(scene, point, halo_diameter * 0.5, style.halo);
    }

    emit_circle(scene, point, radius, style.fill);

    scene.animate(DotAnimationInstance {
        center: point,
        radius,
        kind: animation,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(100.0, 100.0))
    }

    /// One dot "shape" primitive's fill color and pixel diameter, abstracted
    /// over the slug/non-slug emission so test bodies don't need to
    /// duplicate per feature. Off `slug`: a `Quad`'s `fill` + its rect width.
    /// On `slug`: a `VectorPath`'s `fill_color` + twice its placement scale
    /// (the unit circle's diameter once placed).
    #[cfg(not(feature = "slug"))]
    fn dot_shapes(scene: &Scene) -> Vec<(Color, f32)> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some((q.fill, q.rect.size.width)),
                _ => None,
            })
            .collect()
    }

    #[cfg(feature = "slug")]
    fn dot_shapes(scene: &Scene) -> Vec<(Color, f32)> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::VectorPath(v) => v.fill_color.map(|c| (c, v.scale_px_per_unit * 2.0)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn emits_one_dot_shape_per_variant_size_combination_without_halo() {
        let theme = WgpuTheme::default();
        for variant in [
            DotVariant::Ok,
            DotVariant::Warn,
            DotVariant::Danger,
            DotVariant::Neutral,
        ] {
            for size in [DotSize::Sm, DotSize::Md] {
                let mut s = scene();
                dot(
                    &mut s,
                    Point::new(50.0, 50.0),
                    variant,
                    size,
                    false,
                    DotAnimation::None,
                    &theme,
                );
                assert_eq!(
                    dot_shapes(&s).len(),
                    1,
                    "{variant:?}/{size:?} without halo should emit one shape"
                );
            }
        }
    }

    #[test]
    fn halo_emits_an_extra_shape_at_one_and_a_half_diameter() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(20.0, 20.0),
            DotVariant::Ok,
            DotSize::Md,
            true,
            DotAnimation::None,
            &theme,
        );
        let shapes = dot_shapes(&s);
        assert_eq!(shapes.len(), 2, "halo should add a second shape");
        let dot_size = DotSize::Md.diameter();
        let (halo_fill, halo_size) = shapes[0];
        let (body_fill, body_size) = shapes[1];
        assert!((halo_size - dot_size * 1.5).abs() < 0.001);
        assert!((body_size - dot_size).abs() < 0.001);
        assert!(
            halo_fill.a < body_fill.a,
            "halo should be drawn with lower alpha than the body"
        );
    }

    #[test]
    fn variants_resolve_to_distinct_fills() {
        let theme = WgpuTheme::default();
        let mut fills = Vec::new();
        for variant in [
            DotVariant::Ok,
            DotVariant::Warn,
            DotVariant::Danger,
            DotVariant::Neutral,
        ] {
            let mut s = scene();
            dot(
                &mut s,
                Point::new(10.0, 10.0),
                variant,
                DotSize::Md,
                false,
                DotAnimation::None,
                &theme,
            );
            fills.push(dot_shapes(&s)[0].0);
        }
        for (i, a) in fills.iter().enumerate() {
            for b in fills.iter().skip(i + 1) {
                assert_ne!(a, b, "every status variant should resolve to a unique fill");
            }
        }
    }

    #[cfg(not(feature = "slug"))]
    #[test]
    fn off_slug_dot_is_byte_identical_to_v0_10_0() {
        // Pins the acceptance criterion directly: the off-`slug` build must
        // emit the exact same two `Quad` primitives, in the exact same
        // order, as the pre-#142 `dot()` — reconstructed here from the same
        // expressions v0.10.0 used, not re-derived from the current
        // `emit_circle`.
        let theme = WgpuTheme::default();
        let style = theme.dot_style(DotVariant::Ok, DotSize::Md);
        let point = Point::new(42.0, 24.0);
        let radius = style.diameter * 0.5;
        let halo_diameter = style.diameter * 1.5;
        let halo_origin = Point::new(point.x - halo_diameter * 0.5, point.y - halo_diameter * 0.5);
        let origin = Point::new(point.x - radius, point.y - radius);

        let mut s = scene();
        dot(
            &mut s,
            point,
            DotVariant::Ok,
            DotSize::Md,
            true,
            DotAnimation::None,
            &theme,
        );

        assert_eq!(
            s.primitives,
            vec![
                Primitive::Quad(Quad {
                    rect: Rect::new(halo_origin, Size::new(halo_diameter, halo_diameter)),
                    fill: style.halo,
                    corner_radii: CornerRadii::all(halo_diameter * 0.5),
                    stroke: None,
                }),
                Primitive::Quad(Quad {
                    rect: Rect::new(origin, Size::new(style.diameter, style.diameter)),
                    fill: style.fill,
                    corner_radii: CornerRadii::all(radius),
                    stroke: None,
                }),
            ]
        );
    }

    #[test]
    fn animation_none_pushes_no_metadata() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(10.0, 10.0),
            DotVariant::Ok,
            DotSize::Sm,
            false,
            DotAnimation::None,
            &theme,
        );
        assert!(s.animations.is_empty());
    }

    #[test]
    fn animation_non_none_pushes_one_instance_keyed_at_center() {
        let theme = WgpuTheme::default();
        for kind in [
            DotAnimation::Pulse,
            DotAnimation::PulseUrgent,
            DotAnimation::Spin,
        ] {
            let mut s = scene();
            dot(
                &mut s,
                Point::new(42.0, 24.0),
                DotVariant::Warn,
                DotSize::Md,
                true,
                kind,
                &theme,
            );
            assert_eq!(s.animations.len(), 1, "{kind:?} should push one instance");
            assert_eq!(s.animations[0].kind, kind);
            assert_eq!(s.animations[0].center, Point::new(42.0, 24.0));
            assert!((s.animations[0].radius - DotSize::Md.diameter() * 0.5).abs() < 0.001);
        }
    }

    // ---- slug-feature-only: vector circle shape contract (#142) -----------

    #[cfg(feature = "slug")]
    #[test]
    fn vector_circle_uses_only_curves_no_line_segments() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(10.0, 10.0),
            DotVariant::Ok,
            DotSize::Md,
            false,
            DotAnimation::None,
            &theme,
        );
        let path = match &s.primitives[0] {
            Primitive::VectorPath(v) => &v.path,
            other => panic!("expected a VectorPath primitive, got {other:?}"),
        };
        assert!(
            !path
                .commands
                .iter()
                .any(|c| matches!(c, PathCommand::LineTo(_))),
            "a dot-with-line-segments is still a polygon; the circle must be built from curves"
        );
        assert!(
            path.commands
                .iter()
                .any(|c| matches!(c, PathCommand::CubicTo { .. })),
            "the circle recipe is 4 cubic Bezier arcs"
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn vector_circle_path_is_shared_across_calls() {
        let theme = WgpuTheme::default();
        let mut a = scene();
        let mut b = scene();
        dot(
            &mut a,
            Point::new(1.0, 1.0),
            DotVariant::Ok,
            DotSize::Sm,
            false,
            DotAnimation::None,
            &theme,
        );
        dot(
            &mut b,
            Point::new(2.0, 2.0),
            DotVariant::Warn,
            DotSize::Md,
            false,
            DotAnimation::None,
            &theme,
        );
        let path_a = match &a.primitives[0] {
            Primitive::VectorPath(v) => Arc::clone(&v.path),
            _ => unreachable!("vector_circle_uses_only_curves_no_line_segments proves the shape"),
        };
        let path_b = match &b.primitives[0] {
            Primitive::VectorPath(v) => Arc::clone(&v.path),
            _ => unreachable!(),
        };
        assert!(
            Arc::ptr_eq(&path_a, &path_b),
            "every dot must share the same cached unit-circle geometry (allocation-free hot path)"
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn vector_circle_has_no_stroke_and_places_at_center_scaled_by_radius() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(42.0, 24.0),
            DotVariant::Danger,
            DotSize::Md,
            false,
            DotAnimation::None,
            &theme,
        );
        let placed = match &s.primitives[0] {
            Primitive::VectorPath(v) => v,
            other => panic!("expected a VectorPath primitive, got {other:?}"),
        };
        assert!(
            placed.stroke.is_none(),
            "dot is a filled shape, no stroke by default"
        );
        assert_eq!(placed.origin_px, [42.0, 24.0]);
        assert!((placed.scale_px_per_unit - DotSize::Md.diameter() * 0.5).abs() < 0.001);
    }

    #[cfg(feature = "slug")]
    #[test]
    fn unit_circle_path_encodes_cleanly_through_the_real_encoder() {
        // Proves the unit-radius circle actually survives the general
        // `encode_vector_path` lane end-to-end. `encode_fill` (the
        // renderer's real call site) silently drops a path that fails to
        // encode (`.ok()?`), which would make the dot invisible on the
        // `slug` lane with no visible error — so this must be checked
        // directly, not inferred from `dot()` only building a primitive.
        use mkui_vector2d::{encode_vector_path, SlugConfig};

        // Mirrors `mkui_vector2d_wgpu`'s private `default_path_config`
        // exactly (band counts 8x8, revision 1) — the real call site's
        // config.
        let config = SlugConfig::new(8, 8, 1);
        let glyph = encode_vector_path(&UNIT_CIRCLE, &config)
            .expect("the unit circle must encode through the real VectorPath encoder");

        assert!(!glyph.curves.is_empty(), "the circle must produce curves");
        assert!(
            glyph.curves.iter().all(|c| c.p1 != c.p2),
            "every curve must be a real quadratic (p1 != p2), not a line sentinel \
             (the circle recipe has no LineTo segments)"
        );
        assert!(
            (glyph.bounds.x_min - -1.0).abs() < 0.05
                && (glyph.bounds.x_max - 1.0).abs() < 0.05
                && (glyph.bounds.y_min - -1.0).abs() < 0.05
                && (glyph.bounds.y_max - 1.0).abs() < 0.05,
            "derived control-hull bounds must stay close to the unit circle's \
             [-1, 1] extent, got {:?}",
            glyph.bounds
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn halo_variant_also_renders_vector() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(5.0, 5.0),
            DotVariant::Ok,
            DotSize::Md,
            true,
            DotAnimation::None,
            &theme,
        );
        let vector_paths: Vec<_> = s
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::VectorPath(_)))
            .collect();
        assert_eq!(
            vector_paths.len(),
            2,
            "halo + body should both render as VectorPath"
        );
    }
}
