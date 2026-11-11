//! `button` / `button_with` — two-state push button.
//!
//! `button` is the themed entry point: shadcn-style `variant` + `size` resolve
//! through [`WgpuTheme`] so a theme swap is a one-line change at every call
//! site. `button_with` is the escape hatch that takes a pre-resolved
//! [`ButtonStyle`] for one-off looks the theme does not cover.
//!
//! ## Disabled, focus ring, touch target (#186)
//!
//! `disabled` and `focused` are supplied by the caller each frame, the same
//! immediate-mode contract as [`ButtonState`] and `text_field_editable`'s
//! `caret_visible` — this module never tracks interaction state itself.
//! `disabled` dims the resolved chrome/label by [`DISABLED_ALPHA_FACTOR`] so
//! a disabled button reads as visually distinct regardless of variant, and
//! both `button`/`button_with` return [`ButtonRegions`] whose `hit_rect` is
//! `None` for a disabled button — a caller that only ever registers
//! `hit_rect.map(...)`/`if let Some(hit_rect) = ...` into its hit-test layout
//! structurally cannot fire a disabled button's action. `focused` draws an
//! outer ring ([`ButtonStyle::focus_ring_color`]) outset from the button by
//! [`FOCUS_RING_GAP_PX`] — driven only by this explicit flag, never by mouse
//! position, so a mouse hover alone never produces a ring. Wiring an actual
//! keyboard/gamepad focus-traversal manager to set `focused` is tracked
//! separately (#203); this module only renders the signal it's handed.
//!
//! When enabled, `hit_rect` is `rect` expanded to at least
//! [`MIN_TOUCH_TARGET_PX`] on each axis (via
//! [`crate::types::Rect::expand_to_min_size`]) — the *visual* button can
//! stay small (e.g. an icon button) while the clickable/tappable area still
//! meets a touch-friendly minimum, mirroring [`super::slider::SliderRegions`]'s
//! `hit_rect` pattern.
//!
//! ## Chrome rendering (#140)
//!
//! The rounded-rect fill + border forks on the `slug` feature, resolved at
//! compile time in [`emit_chrome`] (two separate definitions below, each
//! `#[cfg]`-gated): off, it is the pre-existing triangulated [`Quad`], its
//! corners faceted by the native tessellator (#212); on, it is two
//! [`Primitive::VectorPath`]s — a fill-only rounded rect at the full rect
//! size, and a stroke-only rounded rect inset by half the border width
//! (Sprint 8 Wave 2 — `card`/`dot` follow the same shape in #141/#142). The
//! inset keeps the border's *outer* edge flush with `rect`, matching the
//! triangulated tessellator (its border ring starts exactly at `rect`'s own
//! boundary and never extends past it) — a plain shared centreline would
//! stroke outward past `rect` by half the border width (#213 Codex round 1).
//! [`emit_chrome`]'s own fork is entirely internal — it never sees
//! `disabled`/`focused`, which are handled once in `button_with` by dimming
//! the colors handed to it and, for the ring, by a separate always-`Quad`
//! [`emit_focus_ring`] call (a focus ring is a plain decorative outline, not
//! part of the `Quad`-vs-`VectorPath` chrome-fidelity contract #213 guards).

use crate::theme::{ButtonSize, ButtonState, ButtonStyle, ButtonVariant, WgpuTheme};
use crate::types::{
    Color, CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, TextAlign, TextStyle,
};

#[cfg(feature = "slug")]
use crate::types::PlacedVectorPath;
#[cfg(feature = "slug")]
use mkui_vector2d_wgpu::{Bounds, FillRule, PathCommand, Stroke as PathStroke, Vec2, VectorPath};

use super::label;

/// Minimum clickable/tappable hit-target size (Apple HIG / Material both
/// land on 44-48px; this picks the 44px floor shared by both) (#186). A
/// button's *visual* rect may be smaller — e.g. a compact `ButtonSize::Icon`
/// — so `hit_rect` in [`ButtonRegions`] expands up to this floor rather than
/// shrinking the rendered chrome.
pub const MIN_TOUCH_TARGET_PX: f32 = 44.0;

/// Alpha multiplier applied to a disabled button's fill/stroke/label so it
/// reads as visually distinct regardless of variant (#186). A widget-level
/// geometry/look constant, not a themed color — same category as
/// `DotSize::diameter` per the theme module's token-sourcing rule.
const DISABLED_ALPHA_FACTOR: f32 = 0.45;

/// Gap, in px, between the button's own border and the outer focus ring
/// (#186). Widget-intrinsic geometry, not a themed token — see
/// [`DISABLED_ALPHA_FACTOR`]'s doc comment.
const FOCUS_RING_GAP_PX: f32 = 2.0;

/// Stroke width, in px, of the focus ring itself (#186).
const FOCUS_RING_WIDTH_PX: f32 = 2.0;

/// Regions [`button`]/[`button_with`] hand back for the caller to hit-test
/// and lay out, mirroring [`super::slider::SliderRegions`] (#186).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonRegions {
    /// The rect actually painted — unchanged from the caller's input `rect`.
    pub visual_rect: Rect,
    /// Where the caller should hit-test pointer/touch input: `visual_rect`
    /// expanded to [`MIN_TOUCH_TARGET_PX`] on each axis, or `None` when the
    /// button is disabled. A caller that only ever registers `Some(rect)`
    /// into its hit-test layout can never make a disabled button fire.
    pub hit_rect: Option<Rect>,
}

/// Low-level two-state button. Takes a pre-resolved `ButtonStyle` — reserved
/// for callers that need a one-off style. Most call sites should use the
/// themed [`button`] instead.
///
/// `disabled` dims the chrome/label and suppresses `hit_rect`; `focused`
/// draws the style's `focus_ring_color` outset from `rect`. Both are
/// caller-supplied every frame — see the module doc.
pub fn button_with(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    state: ButtonState,
    disabled: bool,
    focused: bool,
    style: &ButtonStyle,
) -> ButtonRegions {
    let (mut fill, mut stroke_color, mut label_color) = match state {
        ButtonState::Idle => (style.idle_fill, style.idle_stroke, style.idle_label),
        ButtonState::Active => (style.active_fill, style.active_stroke, style.active_label),
    };
    if disabled {
        fill = fill.multiply_alpha(DISABLED_ALPHA_FACTOR);
        stroke_color = stroke_color.multiply_alpha(DISABLED_ALPHA_FACTOR);
        label_color = label_color.multiply_alpha(DISABLED_ALPHA_FACTOR);
    }
    emit_chrome(
        scene,
        rect,
        fill,
        stroke_color,
        style.corner_radius,
        style.stroke_width,
    );
    if focused {
        emit_focus_ring(scene, rect, style.corner_radius, style.focus_ring_color);
    }
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

    ButtonRegions {
        visual_rect: rect,
        hit_rect: (!disabled)
            .then(|| rect.expand_to_min_size(Size::new(MIN_TOUCH_TARGET_PX, MIN_TOUCH_TARGET_PX))),
    }
}

/// Two-state button with shadcn-style `variant` + `size`. Resolves through
/// the provided [`WgpuTheme`] so a theme swap is a one-line change at every
/// call site.
// allow: `disabled` + `focused` (#186) added alongside the existing
// variant/size/state/theme axes; bundling them into a param-struct would
// diverge from every other themed widget in this module (`chip_group` makes
// the same call for its two extra params).
#[allow(clippy::too_many_arguments)]
pub fn button(
    scene: &mut Scene,
    rect: Rect,
    label_text: &str,
    variant: ButtonVariant,
    size: ButtonSize,
    state: ButtonState,
    disabled: bool,
    focused: bool,
    theme: &WgpuTheme,
) -> ButtonRegions {
    let style = theme.button_style(variant, size);
    button_with(scene, rect, label_text, state, disabled, focused, &style)
}

/// Stroke-only ring drawn outside `rect`, leaving a [`FOCUS_RING_GAP_PX`]
/// gap to the button's own edge. A `Quad` stroke draws *inside* its own
/// `rect` — from `rect`'s edge inward by `width`, never centred on it (see
/// `push_rounded_rect_stroke`'s inset-boundary construction, the same
/// convention [`emit_chrome`]'s triangulated border relies on) — so the
/// ring's own `rect` is outset by the full `gap + width`: that outer
/// boundary becomes the ring's *outer* edge, and the inset-by-`width` inner
/// boundary lands exactly `gap` past the button. `corner_radii` grows by
/// the same outset so the ring stays concentric with `corner_radius`.
/// Always a plain [`Quad`], independent of the `slug` feature: a focus ring
/// is a decorative affordance, not part of the chrome-fidelity contract
/// `emit_chrome`'s fork guards (#213).
fn emit_focus_ring(scene: &mut Scene, rect: Rect, corner_radius: f32, color: Color) {
    let outset = FOCUS_RING_GAP_PX + FOCUS_RING_WIDTH_PX;
    scene.quad(Quad {
        rect: rect.expand(outset),
        fill: Color::TRANSPARENT,
        corner_radii: CornerRadii::all(corner_radius + outset),
        stroke: Some(Stroke {
            color,
            width: FOCUS_RING_WIDTH_PX,
        }),
    });
}

/// Triangulated rounded-rect fill + border. `corner_radii` is faceted into a
/// many-sided polygon by `tessellation::tessellate_quad` (#212) — rounded,
/// but with hard, non-anti-aliased edges, unlike the vector lane's true
/// curve below.
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
            false,
            false,
            &theme,
        );
        button(
            &mut active_scene,
            Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0)),
            "Sel",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Active,
            false,
            false,
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
            false,
            false,
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
            false,
            false,
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
        button_with(
            &mut scene,
            rect,
            "",
            ButtonState::Idle,
            false,
            false,
            &style,
        );

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

    fn small_rect() -> Rect {
        Rect::new(Point::new(10.0, 10.0), Size::new(60.0, 40.0))
    }

    /// Acceptance: all three variants (primary = `Default`, ghost, icon-sized)
    /// render and each resolves an independent fill via theme tokens (#186).
    #[test]
    fn primary_ghost_and_icon_variants_resolve_distinct_idle_fills() {
        let theme = WgpuTheme::default();
        let primary = theme.button_style(ButtonVariant::Default, ButtonSize::Default);
        let ghost = theme.button_style(ButtonVariant::Ghost, ButtonSize::Default);
        let icon = theme.button_style(ButtonVariant::Default, ButtonSize::Icon);

        assert_ne!(
            primary.idle_fill, ghost.idle_fill,
            "primary and ghost must resolve to different idle fills"
        );
        // Icon shares Default's color tokens (only the size axis differs) —
        // independence here means its own geometry (corner radius), not fill.
        assert_ne!(
            primary.corner_radius, icon.corner_radius,
            "icon size must resolve its own corner radius independent of Default size"
        );

        let mut scene = Scene::new(Size::new(200.0, 80.0));
        for (variant, size) in [
            (ButtonVariant::Default, ButtonSize::Default),
            (ButtonVariant::Ghost, ButtonSize::Default),
            (ButtonVariant::Default, ButtonSize::Icon),
        ] {
            button(
                &mut scene,
                small_rect(),
                "",
                variant,
                size,
                ButtonState::Idle,
                false,
                false,
                &theme,
            );
        }
        assert!(
            !scene.primitives.is_empty(),
            "all three variants must emit chrome"
        );
    }

    #[test]
    fn disabled_button_dims_fill_and_label_and_returns_no_hit_rect() {
        let theme = WgpuTheme::default();
        let mut enabled_scene = Scene::new(Size::new(200.0, 80.0));
        let mut disabled_scene = Scene::new(Size::new(200.0, 80.0));
        let rect = small_rect();

        let enabled = button(
            &mut enabled_scene,
            rect,
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            false,
            false,
            &theme,
        );
        let disabled = button(
            &mut disabled_scene,
            rect,
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            true,
            false,
            &theme,
        );

        assert_ne!(
            first_fill(&enabled_scene),
            first_fill(&disabled_scene),
            "a disabled button must be visually distinct from an enabled one"
        );
        assert!(
            enabled.hit_rect.is_some(),
            "an enabled button must expose a hit rect"
        );
        assert!(
            disabled.hit_rect.is_none(),
            "a disabled button must expose no hit rect, so a caller that \
             only registers `Some` hit rects can never fire its action"
        );
    }

    #[test]
    fn hit_rect_meets_the_minimum_touch_target_even_for_a_small_visual_rect() {
        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        // A compact icon-sized button, well under the 44px floor.
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(20.0, 20.0));
        let regions = button(
            &mut scene,
            rect,
            "",
            ButtonVariant::Ghost,
            ButtonSize::Icon,
            ButtonState::Idle,
            false,
            false,
            &theme,
        );

        let hit_rect = regions.hit_rect.expect("enabled button has a hit rect");
        assert_eq!(regions.visual_rect, rect, "the painted rect is unchanged");
        assert!(hit_rect.size.width >= MIN_TOUCH_TARGET_PX);
        assert!(hit_rect.size.height >= MIN_TOUCH_TARGET_PX);
        // Expansion is centered on the visual rect.
        let visual_center_x = rect.origin.x + rect.size.width * 0.5;
        let hit_center_x = hit_rect.origin.x + hit_rect.size.width * 0.5;
        assert!((visual_center_x - hit_center_x).abs() < 1e-4);
    }

    #[test]
    fn hit_rect_is_unchanged_when_the_visual_rect_already_meets_the_touch_target() {
        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(200.0, 80.0));
        let rect = Rect::new(Point::new(10.0, 10.0), Size::new(120.0, 48.0));
        let regions = button(
            &mut scene,
            rect,
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            false,
            false,
            &theme,
        );
        assert_eq!(regions.hit_rect, Some(rect));
    }

    /// Acceptance: a focus ring appears only when `focused` is explicitly
    /// set — never implicitly from hover, since this module has no hover
    /// concept at all; `focused` is the sole signal (#186).
    #[test]
    fn focus_ring_only_renders_when_focused_is_true() {
        let theme = WgpuTheme::default();
        let rect = small_rect();

        let mut unfocused_scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut unfocused_scene,
            rect,
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            false,
            false,
            &theme,
        );
        let mut focused_scene = Scene::new(Size::new(200.0, 80.0));
        button(
            &mut focused_scene,
            rect,
            "Go",
            ButtonVariant::Default,
            ButtonSize::Default,
            ButtonState::Idle,
            false,
            true,
            &theme,
        );

        fn quad_count(scene: &Scene) -> usize {
            scene
                .primitives
                .iter()
                .filter(|p| matches!(p, Primitive::Quad(_)))
                .count()
        }
        assert_eq!(
            quad_count(&focused_scene),
            quad_count(&unfocused_scene) + 1,
            "a focused button must emit exactly one extra Quad for the ring"
        );

        let ring = focused_scene
            .primitives
            .iter()
            .find_map(|p| match p {
                Primitive::Quad(q) if q.rect != rect => Some(q),
                _ => None,
            })
            .expect("focused button must emit a ring Quad outside its own rect");
        assert!(
            ring.rect.origin.x < rect.origin.x && ring.rect.origin.y < rect.origin.y,
            "the ring must be outset from the button, not drawn on top of it"
        );
        // The ring Quad's own `rect` is its *outer* edge (a `Quad` stroke
        // draws inward from its own `rect`, never centred on it — see
        // `emit_focus_ring`'s doc comment) — so the ring's *inner* edge,
        // `width` inside that outer `rect`, must land exactly
        // `FOCUS_RING_GAP_PX` past the button's own edge, not overlap it.
        let stroke = ring.stroke.expect("ring quad must carry a stroke");
        let ring_inner_left = ring.rect.origin.x + stroke.width;
        assert!(
            (ring_inner_left - (rect.origin.x - FOCUS_RING_GAP_PX)).abs() < 1e-4,
            "ring's inner edge must sit exactly {FOCUS_RING_GAP_PX}px past the button's own \
             edge: got {ring_inner_left}, expected {}",
            rect.origin.x - FOCUS_RING_GAP_PX
        );
        assert_eq!(ring.fill, Color::TRANSPARENT, "the ring must not fill");
        assert!(ring.stroke.is_some(), "the ring must stroke");
    }
}
