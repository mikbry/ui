//! `button` / `button_with` — two-state push button.
//!
//! `button` is the themed entry point: shadcn-style `variant` + `size` resolve
//! through [`WgpuTheme`] so a theme swap is a one-line change at every call
//! site. `button_with` is the escape hatch that takes a pre-resolved
//! [`ButtonStyle`] for one-off looks the theme does not cover.
//!
//! ## Chrome rendering (#140)
//!
//! The rounded-rect fill + border forks on the `slug` feature, resolved at
//! compile time in [`emit_chrome`] (two separate definitions below, each
//! `#[cfg]`-gated): off, it is the pre-existing triangulated [`Quad`] (byte-
//! identical to v0.10.0); on, it is two [`Primitive::VectorPath`]s — a
//! fill-only rounded rect at the full rect size, and a stroke-only rounded
//! rect inset by half the border width (Sprint 8 Wave 2 — `card`/`dot` follow
//! the same shape in #141/#142). The inset keeps the border's *outer* edge
//! flush with `rect`, matching the triangulated tessellator (its border
//! strips start exactly at `rect.origin` and never extend past it) — a plain
//! shared centreline would stroke outward past `rect` by half the border
//! width (#213 Codex round 1). `button`/`button_with`'s own signatures never
//! change — the fork is entirely internal.

use crate::theme::{ButtonSize, ButtonState, ButtonStyle, ButtonVariant, WgpuTheme};
use crate::types::{Color, Point, Rect, Scene, Size, TextAlign, TextStyle};

#[cfg(not(feature = "slug"))]
use crate::types::{CornerRadii, Quad, Stroke};

#[cfg(feature = "slug")]
use crate::types::PlacedVectorPath;
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Stroke as PathStroke, Vec2, VectorPath};

use super::label;

/// Low-level two-state button. Takes a pre-resolved `ButtonStyle` — reserved
/// for callers that need a one-off style. Most call sites should use the
/// themed [`button`] instead.
pub fn button_with(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    state: ButtonState,
    style: &ButtonStyle,
) {
    let (fill, stroke_color, label_color) = match state {
        ButtonState::Idle => (style.idle_fill, style.idle_stroke, style.idle_label),
        ButtonState::Active => (style.active_fill, style.active_stroke, style.active_label),
    };
    emit_chrome(
        scene,
        rect,
        fill,
        stroke_color,
        style.corner_radius,
        style.stroke_width,
    );
    let text_style = TextStyle {
        align: TextAlign::Center,
        color: label_color,
        ..style.label_style
    };
    let label_height = text_style.line_height_px;
    let label_rect = Rect::new(
        Point::new(
            rect.origin.x,
            rect.origin.y + (rect.size.height - label_height) * 0.5,
        ),
        Size::new(rect.size.width, label_height),
    );
    label(scene, label_rect, label_text, text_style);
}

/// Two-state button with shadcn-style `variant` + `size`. Resolves through
/// the provided [`WgpuTheme`] so a theme swap is a one-line change at every
/// call site.
pub fn button(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    variant: ButtonVariant,
    size: ButtonSize,
    state: ButtonState,
    theme: &WgpuTheme,
) {
    let style = theme.button_style(variant, size);
    button_with(scene, rect, label_text, state, &style);
}

/// Triangulated rounded-rect fill + border (v0.10.0 behavior, unchanged).
/// `corner_radii` is accepted by [`Quad`] but — like every other triangulated
/// component — the tessellator draws a plain rectangle (see
/// `tessellation::tessellate_quad`); this is exactly the pre-#140 output.
#[cfg(not(feature = "slug"))]
fn emit_chrome(
    scene: &mut Scene,
    rect: Rect,
    fill: Color,
    stroke_color: Color,
    corner_radius: f32,
    stroke_width: f32,
) {
    scene.quad(Quad {
        rect,
        fill,
        corner_radii: CornerRadii::all(corner_radius),
        stroke: Some(Stroke {
            color: stroke_color,
            width: stroke_width,
        }),
    });
}

/// Vector rounded-rect fill + border (#140): a fill-only [`VectorPath`] at
/// the full `rect`, plus a *separate* stroke-only [`VectorPath`] whose
/// centreline is inset by half the border width on every side (and whose
/// corner radius shrinks to match). Stroking that inset centreline with the
/// full `stroke_width` expands it back out by the same half-width, so the
/// border's outer edge lands exactly on `rect`'s boundary — never past it —
/// matching the triangulated [`Quad`] below, whose border strips are drawn
/// fully inside `rect` from `rect.origin`. `stroke` uses the theme width with
/// default (butt/miter, no dash) caps/joins per the issue's scope.
#[cfg(feature = "slug")]
fn emit_chrome(
    scene: &mut Scene,
    rect: Rect,
    fill: Color,
    stroke_color: Color,
    corner_radius: f32,
    stroke_width: f32,
) {
    let w = rect.size.width.max(0.0);
    let h = rect.size.height.max(0.0);
    // Bottom-left anchor in the Slug lane's y-up local path space — see
    // `rounded_rect_path`'s doc comment for why this is the bottom-left (not
    // top-left) corner.
    scene.vector_path(PlacedVectorPath {
        path: std::sync::Arc::new(rounded_rect_path(rect.size, corner_radius)),
        stroke: None,
        stroke_color,
        fill_color: Some(fill),
        origin_px: [rect.origin.x, rect.origin.y + h],
        scale_px_per_unit: 1.0,
    });

    let half = stroke_width.max(0.0) * 0.5;
    let inset_size = Size::new((w - stroke_width).max(0.0), (h - stroke_width).max(0.0));
    let inset_radius = (corner_radius - half).max(0.0);
    scene.vector_path(PlacedVectorPath {
        path: std::sync::Arc::new(rounded_rect_path(inset_size, inset_radius)),
        stroke: Some(PathStroke::new(stroke_width)),
        stroke_color,
        fill_color: None,
        // The inset rect's own bottom-left anchor: `rect`'s anchor shifted
        // inward by `half` on both axes (screen x grows right, screen y here
        // shrinks toward `rect`'s top as local y grows — see the module doc).
        origin_px: [rect.origin.x + half, rect.origin.y + h - half],
        scale_px_per_unit: 1.0,
    });
}

/// Build a closed rounded-rect outline via `moveTo` + `lineTo` + `quadTo` for
/// each of the 4 corners, in the Slug lane's **y-up** local path space (the
/// same convention a glyph outline uses — see [`PlacedVectorPath`]'s
/// placement contract: "matches the Slug glyph lane so a path scales with
/// zoom exactly like a glyph"). Local `(0, 0)` is the rect's bottom-left and
/// local `(w, h)` its top-right, so the caller anchors `origin_px` at the
/// rect's bottom-left screen position (`[rect.origin.x, rect.origin.y +
/// rect.size.height]`), not its top-left — getting this backwards would
/// render the whole shape flipped above the rect instead of filling it.
///
/// `radius` is clamped to half the shorter side so a small button (or a
/// theme radius larger than the button itself) never produces a
/// self-intersecting path.
#[cfg(feature = "slug")]
fn rounded_rect_path(size: Size, radius: f32) -> VectorPath {
    let w = size.width.max(0.0);
    let h = size.height.max(0.0);
    let r = radius.max(0.0).min(w.min(h) * 0.5);
    let commands = vec![
        PathCommand::MoveTo(Vec2::new(r, 0.0)),
        PathCommand::LineTo(Vec2::new(w - r, 0.0)),
        PathCommand::QuadTo {
            control: Vec2::new(w, 0.0),
            to: Vec2::new(w, r),
        },
        PathCommand::LineTo(Vec2::new(w, h - r)),
        PathCommand::QuadTo {
            control: Vec2::new(w, h),
            to: Vec2::new(w - r, h),
        },
        PathCommand::LineTo(Vec2::new(r, h)),
        PathCommand::QuadTo {
            control: Vec2::new(0.0, h),
            to: Vec2::new(0.0, h - r),
        },
        PathCommand::LineTo(Vec2::new(0.0, r)),
        PathCommand::QuadTo {
            control: Vec2::new(0.0, 0.0),
            to: Vec2::new(r, 0.0),
        },
        PathCommand::Close,
    ];
    VectorPath::new(
        commands,
        FillRule::NonZero,
        Bounds::new(Vec2::ZERO, Vec2::new(w, h)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Size};

    #[cfg(not(feature = "slug"))]
    fn first_fill(scene: &Scene) -> Option<Color> {
        scene.primitives.iter().find_map(|p| match p {
            Primitive::Quad(q) => Some(q.fill),
            _ => None,
        })
    }

    #[cfg(feature = "slug")]
    fn first_fill(scene: &Scene) -> Option<Color> {
        scene.primitives.iter().find_map(|p| match p {
            Primitive::VectorPath(v) => v.fill_color,
            _ => None,
        })
    }

    #[test]
    fn button_emits_different_fills_for_idle_and_active() {
        let theme = WgpuTheme::default();
        let mut idle_scene = Scene::new(Size::new(200.0, 80.0));
        let mut active_scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut idle_scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Sel",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            &theme,
        );
        button(
            &mut active_scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Sel",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Active,
            &theme,
        );
        let idle_fill = first_fill(&idle_scene).expect("idle chrome");
        let active_fill = first_fill(&active_scene).expect("active chrome");
        assert_ne!(idle_fill, active_fill);
    }

    #[cfg(not(feature = "slug"))]
    #[test]
    fn button_emits_a_triangulated_quad_with_border_when_slug_is_off() {
        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            &theme,
        );
        let quad = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .expect("slug-off button must emit the triangulated Quad chrome");
        assert!(quad.stroke.is_some(), "button chrome must carry a border");
    }

    #[cfg(feature = "slug")]
    #[test]
    fn button_emits_a_vector_path_fill_and_stroke_when_slug_is_on() {
        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            &theme,
        );
        let vector_paths: Vec<_> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::VectorPath(v) => Some(v),
                _ => None,
            })
            .collect();
        assert!(
            vector_paths.iter().any(|v| v.fill_color.is_some()),
            "button fill must ride a vector path"
        );
        assert!(
            vector_paths.iter().any(|v| v.stroke.is_some()),
            "button border must ride a vector path"
        );
        assert!(
            !scene
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Quad(_))),
            "slug-on button must not also emit the triangulated Quad chrome"
        );
    }

    /// #213 Codex round 1: the border's *expanded* stroke (centreline ±
    /// half its width) must stay entirely within the button's layout/hit
    /// rect, matching the triangulated tessellator — never bleed outside it.
    #[cfg(feature = "slug")]
    #[test]
    fn button_vector_border_outer_edge_stays_within_rect() {
        let theme = WgpuTheme::default();
        let style = theme.button_style(ButtonVariant::Default, ButtonSize::Default);
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0));
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        button_with(&mut scene, rect, "", ButtonState::Idle, &style);

        let border = scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::VectorPath(v) if v.stroke.is_some() => Some(v),
                _ => None,
            })
            .expect("button must emit a stroke-only VectorPath for its border");
        let stroke = border.stroke.as_ref().expect("checked above");
        let half = stroke.width_px * 0.5;
        let [ox, oy] = border.origin_px;
        let bounds = border.path.bounds;

        // Screen-space outer envelope of the expanded stroke, using this
        // module's y-up-local -> y-down-screen placement (`screen_x = ox +
        // local_x`, `screen_y = oy - local_y`): the centreline's local bounds,
        // widened by half the stroke width on every side.
        let outer_min_x = ox + bounds.min.x - half;
        let outer_max_x = ox + bounds.max.x + half;
        let outer_min_y = oy - bounds.max.y - half;
        let outer_max_y = oy - bounds.min.y + half;

        let eps = 1e-4;
        assert!(
            outer_min_x >= rect.origin.x - eps,
            "border must not extend left of rect: {outer_min_x} < {}",
            rect.origin.x
        );
        assert!(
            outer_max_x <= rect.origin.x + rect.size.width + eps,
            "border must not extend right of rect: {outer_max_x} > {}",
            rect.origin.x + rect.size.width
        );
        assert!(
            outer_min_y >= rect.origin.y - eps,
            "border must not extend above rect: {outer_min_y} < {}",
            rect.origin.y
        );
        assert!(
            outer_max_y <= rect.origin.y + rect.size.height + eps,
            "border must not extend below rect: {outer_max_y} > {}",
            rect.origin.y + rect.size.height
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn rounded_rect_path_is_a_single_closed_contour() {
        let path = rounded_rect_path(Size::new(60.0, 40.0), 8.0);
        assert!(matches!(
            path.commands.first(),
            Some(PathCommand::MoveTo(_))
        ));
        assert!(matches!(path.commands.last(), Some(PathCommand::Close)));
        assert_eq!(
            path.commands
                .iter()
                .filter(|c| matches!(c, PathCommand::QuadTo { .. }))
                .count(),
            4,
            "one quadratic arc per corner"
        );
    }

    #[cfg(feature = "slug")]
    #[test]
    fn rounded_rect_path_clamps_radius_to_half_the_shorter_side() {
        // Shorter side is 4.0 (height); an oversized 100.0 radius clamps to 2.0.
        let path = rounded_rect_path(Size::new(10.0, 4.0), 100.0);
        match path.commands.first() {
            Some(PathCommand::MoveTo(p)) => assert_eq!(p.x, 2.0),
            other => panic!("expected MoveTo as the first command, got {other:?}"),
        }
    }
}
