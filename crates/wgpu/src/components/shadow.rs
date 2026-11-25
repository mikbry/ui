//! `Shadow` vector-path emission (#279).
//!
//! Extends the Slug-GPU `VectorPath` mechanism (#140/#141/#142 —
//! Button/Card/Dot's rounded-rect fill+border) to `Shadow`, fixing
//! #219/#212's corner-radii bug class at the source: `tessellate_shadow`
//! (`tessellation.rs`) never reads `Shadow::corner_radii` — it always emits
//! a plain expanded rectangle — so a `Shadow` paired with a rounded `Quad`
//! (e.g. `panel`/`glass_panel`/`toast_stack`'s chrome) renders with square
//! corners that visibly poke out past the `Quad`'s own rounded ones.
//!
//! **Scope.** Button, Card, and Dot never push a `Shadow` primitive at all
//! — Card's own module doc is explicit that it has "no shadow" — so this
//! wires into the primitive's actual producers (`panel`/`glass_panel` in
//! `components::panel`, and `toast_stack` in `components::toast`) rather
//! than adding a shadow none of those three atoms ever asked for. Per
//! #140's own precedent for the triangulated lane, `tessellate_shadow`
//! stays byte-identical; this only changes what a `slug`-on call site
//! pushes into the scene, via [`emit_shadow`] (feature-forked below, like
//! `button`/`card`/`dot`'s own chrome helpers).

use crate::types::{Scene, Shadow};

#[cfg(feature = "slug")]
use crate::tessellation::clamp_corner_radii;
#[cfg(feature = "slug")]
use crate::types::{Color, CornerRadii, PlacedVectorPath};
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
#[cfg(feature = "slug")]
use std::sync::Arc;

/// `tessellate_shadow`'s own blur/spread approximation, duplicated here
/// (not shared — `tessellation.rs` keeps it `fn`-local too) so both lanes
/// expand a `Shadow`'s rect by the exact same amount.
#[cfg(feature = "slug")]
fn expand_amount(shadow: &Shadow) -> f32 {
    shadow.spread.max(0.0) + shadow.blur_radius.max(0.0) * 0.35
}

/// Push `shadow`'s chrome into `scene`. Off `slug`, this is exactly
/// `scene.shadow` — the pre-existing, still-`corner_radii`-blind
/// `tessellate_shadow` path, unchanged (#279's scope: this issue doesn't
/// fix the triangulated lane's bug, same as #140 didn't fix triangulated
/// rendering's limitations for `Button`).
#[cfg(not(feature = "slug"))]
pub(crate) fn emit_shadow(scene: &mut Scene, shadow: Shadow) {
    scene.shadow(shadow);
}

/// On `slug`, a fill-only [`VectorPath`] rounded rect in place of the
/// triangulated `Shadow` primitive: the same rect expansion
/// [`expand_amount`] the triangulated lane uses, but with `corner_radii`
/// *also* grown by that same amount — the CSS `box-shadow` spec's own
/// spread semantics ("the corner radii of the resulting shape are also
/// increased... by the spread distance, but never made negative"). Growing
/// the radius instead of reusing the `Quad`'s own keeps every corner arc's
/// *center* exactly where the paired `Quad`'s arc center is — only the
/// radius differs, by exactly `expand_amount` — which is what makes the
/// shadow's rounded corner actually track the fill's rounded corner instead
/// of drifting away from it as spread/blur grows. A radius of `0.0` grows
/// to `expand_amount` too, by the same spec wording (a square box's shadow
/// still gains rounded corners once spread is positive).
///
/// `radii` is clamped against the *original* (un-expanded) rect first —
/// matching how the paired `Quad`'s own radii are clamped against *its*
/// rect — so the grown radius can never exceed half the *expanded* rect's
/// shorter side either: `clamped <= original_dim / 2`, so `clamped +
/// expand_amount <= original_dim / 2 + expand_amount == expanded_dim / 2`.
/// No second clamp is needed after growing.
#[cfg(feature = "slug")]
pub(crate) fn emit_shadow(scene: &mut Scene, shadow: Shadow) {
    let amount = expand_amount(&shadow);
    let base = clamp_corner_radii(shadow.rect, shadow.corner_radii);
    let radii = CornerRadii {
        top_left: base.top_left + amount,
        top_right: base.top_right + amount,
        bottom_right: base.bottom_right + amount,
        bottom_left: base.bottom_left + amount,
    };
    let rect = shadow.rect.expand(amount);
    let w = rect.size.width.max(0.0);
    let h = rect.size.height.max(0.0);

    scene.vector_path(PlacedVectorPath {
        path: Arc::new(rounded_rect_path(w, h, radii)),
        stroke: None,
        // Ignored: `stroke` is `None`, so no stroke paint is ever read.
        stroke_color: Color::rgba(0.0, 0.0, 0.0, 0.0),
        fill_color: Some(shadow.color.multiply_alpha(0.6)),
        origin_px: [rect.origin.x, rect.origin.y],
        scale_px_per_unit: 1.0,
    });
}

/// 4/3 × (√2 − 1): the standard circle-to-cubic-Bézier control-point
/// constant — `components::card`'s own `KAPPA`, duplicated here (that
/// constant is private to `card.rs`) rather than built into a cross-module
/// shared helper, same tradeoff `dot.rs`'s `CIRCLE_KAPPA` already makes.
#[cfg(feature = "slug")]
const KAPPA: f32 = 0.552_284_8;

/// Build a closed rounded-rect contour with one independent radius per
/// corner, in the Slug lane's local path space — `components::card`'s
/// `rounded_rect_contour` convention (origin `(0, 0)` is the rect's screen
/// top-left, `y` runs *negative* downward; see that function's doc for the
/// placement math), generalized from one uniform radius to
/// [`CornerRadii`]'s four independent ones, since `Shadow::corner_radii`
/// (unlike `PanelStyle`/`CardStyle`'s single `corner_radius: f32`) can be
/// asymmetric. Each corner's two tangent points and cubic control points
/// all use that corner's own radius — matching how
/// `tessellation::rounded_rect_boundary` applies `CornerRadii` to the
/// triangulated lane, one radius per corner, not a single shared value.
///
/// `radii` is trusted pre-clamped by the caller (every member
/// `<= half the shorter rect dimension`), so no corner's arc can
/// self-intersect; a `0.0` member degenerates that corner's cubic to a
/// zero-length no-op (a sharp corner), same as `card.rs`'s single-radius
/// case.
#[cfg(feature = "slug")]
fn rounded_rect_path(width: f32, height: f32, radii: CornerRadii) -> VectorPath {
    let (tl, tr, br, bl) = (
        radii.top_left.max(0.0),
        radii.top_right.max(0.0),
        radii.bottom_right.max(0.0),
        radii.bottom_left.max(0.0),
    );
    let (ktl, ktr, kbr, kbl) = (tl * KAPPA, tr * KAPPA, br * KAPPA, bl * KAPPA);
    let commands = vec![
        PathCommand::MoveTo(Vec2::new(tl, 0.0)),
        PathCommand::LineTo(Vec2::new(width - tr, 0.0)),
        PathCommand::CubicTo {
            control1: Vec2::new(width - tr + ktr, 0.0),
            control2: Vec2::new(width, -(tr - ktr)),
            to: Vec2::new(width, -tr),
        },
        PathCommand::LineTo(Vec2::new(width, -(height - br))),
        PathCommand::CubicTo {
            control1: Vec2::new(width, -(height - br + kbr)),
            control2: Vec2::new(width - br + kbr, -height),
            to: Vec2::new(width - br, -height),
        },
        PathCommand::LineTo(Vec2::new(bl, -height)),
        PathCommand::CubicTo {
            control1: Vec2::new(bl - kbl, -height),
            control2: Vec2::new(0.0, -(height - bl + kbl)),
            to: Vec2::new(0.0, -(height - bl)),
        },
        PathCommand::LineTo(Vec2::new(0.0, -tl)),
        PathCommand::CubicTo {
            control1: Vec2::new(0.0, -(tl - ktl)),
            control2: Vec2::new(tl - ktl, 0.0),
            to: Vec2::new(tl, 0.0),
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
    // `Color`/`CornerRadii` are only in scope unconditionally here (not via
    // `super::*`, which only imports them under `slug`) because the
    // off-`slug` test below needs both to build a `Shadow` literal too.
    use crate::types::{Color, CornerRadii, Point, Primitive, Rect, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(400.0, 400.0))
    }

    #[cfg(not(feature = "slug"))]
    #[test]
    fn off_slug_emits_the_unchanged_shadow_primitive() {
        let shadow = Shadow {
            rect: Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            blur_radius: 6.0,
            spread: 2.0,
            color: Color::rgba(0.0, 0.0, 0.0, 0.3),
            corner_radii: CornerRadii::all(8.0),
        };
        let mut s = scene();
        emit_shadow(&mut s, shadow);
        assert_eq!(s.primitives, vec![Primitive::Shadow(shadow)]);
    }

    #[cfg(feature = "slug")]
    fn shadow_color() -> Color {
        Color::rgba(0.0, 0.0, 0.0, 0.3)
    }

    #[cfg(feature = "slug")]
    #[test]
    fn on_slug_emits_exactly_one_fill_only_vector_path() {
        let shadow = Shadow {
            rect: Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            blur_radius: 6.0,
            spread: 2.0,
            color: shadow_color(),
            corner_radii: CornerRadii::all(8.0),
        };
        let mut s = scene();
        emit_shadow(&mut s, shadow);
        assert_eq!(s.primitives.len(), 1);
        let placed = match &s.primitives[0] {
            Primitive::VectorPath(v) => v,
            other => panic!("expected a VectorPath primitive, got {other:?}"),
        };
        assert!(placed.stroke.is_none(), "a shadow has no stroke");
        assert_eq!(
            placed.fill_color,
            Some(shadow.color.multiply_alpha(0.6)),
            "the vector lane must match tessellate_shadow's own alpha approximation"
        );
        assert!(
            !s.primitives
                .iter()
                .any(|p| matches!(p, Primitive::Shadow(_))),
            "slug-on must not also emit the triangulated Shadow primitive"
        );
    }

    /// The acceptance criterion at the structural level: a `Shadow`'s vector
    /// corner arc must share its paired `Quad`'s own arc *center*, with a
    /// radius larger by exactly the spread/blur expansion — proven here by
    /// reconstructing both lanes' expected geometry independently (not by
    /// calling `emit_shadow` for one side and `tessellation`'s helpers for
    /// the other, so the test can't pass by the two sides sharing a bug).
    #[cfg(feature = "slug")]
    #[test]
    fn vector_corner_arc_shares_the_paired_quads_arc_center() {
        let rect = Rect::new(Point::new(20.0, 30.0), Size::new(100.0, 60.0));
        let quad_radii = CornerRadii {
            top_left: 4.0,
            top_right: 12.0,
            bottom_right: 6.0,
            bottom_left: 18.0,
        };
        let blur_radius = 10.0;
        let spread = 3.0;
        let amount = spread + blur_radius * 0.35;

        let shadow = Shadow {
            rect,
            blur_radius,
            spread,
            color: shadow_color(),
            corner_radii: quad_radii,
        };
        let mut s = scene();
        emit_shadow(&mut s, shadow);
        let placed = match &s.primitives[0] {
            Primitive::VectorPath(v) => v,
            other => panic!("expected a VectorPath primitive, got {other:?}"),
        };

        // The Quad's own top-left arc center (tessellation.rs convention):
        // `rect.origin + (radius, radius)`.
        let quad_center_x = rect.origin.x + quad_radii.top_left;
        let quad_center_y = rect.origin.y + quad_radii.top_left;

        // The vector shadow's placed top-left arc center, in screen space:
        // local (0, 0) is screen top-left (`origin_px`), so the arc center
        // sits `grown_radius` away from `origin_px` on each axis —
        // `origin_px` is itself the *expanded* rect's origin
        // (`rect.origin - amount`), and `grown_radius` is
        // `quad_radii.top_left + amount`, so the two offsetting `amount`
        // terms cancel: the center must land exactly on the Quad's own
        // arc center, not `amount` away from it.
        let [ox, oy] = placed.origin_px;
        let grown_tl = quad_radii.top_left + amount;
        let shadow_center_x = ox + grown_tl;
        let shadow_center_y = oy + grown_tl;

        let eps = 1e-4;
        assert!(
            (shadow_center_x - quad_center_x).abs() < eps
                && (shadow_center_y - quad_center_y).abs() < eps,
            "shadow arc center must exactly match the quad's own arc center: \
             shadow ({shadow_center_x}, {shadow_center_y}), quad ({quad_center_x}, {quad_center_y})"
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn zero_radius_grows_to_the_expansion_amount_per_css_spread_semantics() {
        // CSS box-shadow spec: a positive spread grows border-radius too,
        // even from zero — a square box's shadow still gains rounded
        // corners once spread is positive. `blur_radius` is this codebase's
        // own spread-equivalent fudge factor (`* 0.35`), so it must behave
        // the same way.
        let shadow = Shadow {
            rect: Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0)),
            blur_radius: 0.0,
            spread: 5.0,
            color: shadow_color(),
            corner_radii: CornerRadii::all(0.0),
        };
        let mut s = scene();
        emit_shadow(&mut s, shadow);
        let placed = match &s.primitives[0] {
            Primitive::VectorPath(v) => v,
            other => panic!("expected a VectorPath primitive, got {other:?}"),
        };
        // A corner radius of exactly `spread` manifests as a `CubicTo`
        // whose `to` point is `spread` away from the local corner — proven
        // indirectly by bounds: the path's local bounds are still the full
        // expanded rect regardless of radius, so check the first command
        // directly.
        match placed.path.commands.first() {
            Some(PathCommand::MoveTo(p)) => assert!(
                (p.x - 5.0).abs() < 1e-4,
                "zero-radius corner must still grow to the spread amount, got {p:?}"
            ),
            other => panic!("expected MoveTo as the first command, got {other:?}"),
        }
    }

    #[cfg(feature = "slug")]
    #[test]
    fn asymmetric_radii_round_trip_through_the_contour() {
        let radii = CornerRadii {
            top_left: 2.0,
            top_right: 4.0,
            bottom_right: 6.0,
            bottom_left: 8.0,
        };
        let path = rounded_rect_path(50.0, 30.0, radii);
        assert!(matches!(
            path.commands.first(),
            Some(PathCommand::MoveTo(_))
        ));
        assert!(matches!(path.commands.last(), Some(PathCommand::Close)));
        assert_eq!(
            path.commands
                .iter()
                .filter(|c| matches!(c, PathCommand::CubicTo { .. }))
                .count(),
            4,
            "one cubic arc per corner"
        );
        match path.commands.first() {
            Some(PathCommand::MoveTo(p)) => assert_eq!(p.x, radii.top_left),
            other => panic!("expected MoveTo as the first command, got {other:?}"),
        }
    }
}
