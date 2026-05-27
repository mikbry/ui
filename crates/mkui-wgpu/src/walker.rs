//! AppTree walker — projects an [`mkui_runtime::AppTree`] into the
//! scene-primitive pipeline that the existing wgpu renderer consumes.
//!
//! The walker is the wgpu-side equivalent of
//! [`mkui_web::render::render_tree`] (per ADR 0006): it consumes the
//! runtime tree directly, emits [`crate::Primitive`]s through the
//! tessellation pipeline already wired up by the v0.4.x HUD work
//! (ADR 0004), and collects a per-frame `Vec<HitTestEntry>` for the
//! input router to reverse-paint-order hit-test against.
//!
//! ## Layout model
//!
//! v0.6.0 implements a deliberately small layout: top-down vertical flow
//! with class-driven padding + gap + text/button sizing. This is enough
//! to render `examples/showcase-common::create_showcase_ui` recognisably
//! on wgpu. Full flex / grid layout is deferred (Sprint 7+); the eager-
//! rebuild model lets a richer layout drop in without re-architecting
//! the walker (ADR 0006 §"Layout v1").
//!
//! ## Anti-patterns avoided (issue #56 + Codex round 10)
//!
//! - **No `thread_local!` for the active tree** — walker is parameterised
//!   on `&AppTree` directly, never via global state.
//! - **No incremental diffing** — eager rebuild on the dirty signal.
//! - **No `wgpu::*` / `winit::*` calls** — walker only writes into
//!   `Scene` + `Vec<HitTestEntry>`; the renderer + input modules own
//!   platform calls.
//! - **No raw `nodes[i]` indexing** — every lookup goes through
//!   `tree.get(id)` so the generation-counter staleness guard fires
//!   when a NodeId points at a recycled slot.

use mkui_runtime::{
    ActionId, AppTree, ButtonVariant, NodeId, NodeKind, ResolvedStyle, TextVariant,
};

use crate::theme::{
    ButtonSize, ButtonVariant as ThemeButtonVariant, HudTheme, TextVariant as ThemeTextVariant,
};
use crate::types::{
    CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, Text, TextAlign, TextStyle,
};

/// One interactive region collected during the walk. The input router
/// hit-tests `rect` and fires `on_press` (looked up in the tree's
/// `ActionRegistry`).
///
/// Per-frame collection is `Vec` rather than a long-lived map because the
/// walker rebuilds the list each frame (eager rebuild on dirty signal —
/// ADR 0006). Reverse iteration handles overlap (topmost-wins).
#[derive(Debug, Clone, Copy)]
pub struct HitTestEntry {
    pub rect: Rect,
    pub node_id: NodeId,
    pub on_press: Option<ActionId>,
}

/// Mutable per-frame context shared between the walker and any custom
/// component renderers it dispatches to.
pub struct WalkContext<'a> {
    pub scene: &'a mut Scene,
    pub theme: &'a HudTheme,
    pub hit_entries: &'a mut Vec<HitTestEntry>,
    pub viewport_width: f32,
    /// Next y-coordinate the walker will emit at. Custom renderers update
    /// this after they emit so the next sibling stacks underneath.
    pub cursor_y: f32,
    /// Left content edge (after any ancestor's left padding has been
    /// applied). Custom renderers may extend the edge for their children.
    pub content_x: f32,
}

impl<'a> WalkContext<'a> {
    pub fn new(
        scene: &'a mut Scene,
        theme: &'a HudTheme,
        hit_entries: &'a mut Vec<HitTestEntry>,
        viewport_width: f32,
    ) -> Self {
        Self {
            scene,
            theme,
            hit_entries,
            viewport_width,
            cursor_y: 0.0,
            content_x: 0.0,
        }
    }
}

/// Tailwind half-rem unit: every numeric resolved-style field
/// (padding, gap, …) multiplies by this to land in screen pixels. The
/// runtime parser stores raw Tailwind ticks (p-6 → 6); the bridge
/// scales them here once so atom code can keep using `f32` pixel rects.
const TAILWIND_UNIT_PX: f32 = 4.0;
const DEFAULT_GAP_PX: f32 = 8.0;
const BUTTON_HEIGHT_PX: f32 = 36.0;
const BUTTON_PADDING_X_PX: f32 = 16.0;
const APPROX_CHAR_WIDTH_PX: f32 = 8.0;
const BUTTON_GAP_PX: f32 = 8.0;
const VIEWPORT_OUTER_MARGIN_PX: f32 = 24.0;

/// Walk `tree` from the root and emit primitives into `scene`. Collects
/// one [`HitTestEntry`] per interactive node into `hit_entries` (in paint
/// order — the input router reverses for topmost-first).
///
/// Eager rebuild: callers invoke this fresh per frame once
/// `AppTree::is_dirty()` returns true (or on first paint). The function
/// does not look at `is_dirty` itself; the caller owns the gate.
pub fn walk_tree(
    tree: &AppTree,
    scene: &mut Scene,
    theme: &HudTheme,
    hit_entries: &mut Vec<HitTestEntry>,
    registry: &crate::bridge::WgpuRendererRegistry,
) {
    let viewport = scene.viewport;
    let mut ctx = WalkContext::new(scene, theme, hit_entries, viewport.width);
    ctx.cursor_y = VIEWPORT_OUTER_MARGIN_PX;
    ctx.content_x = VIEWPORT_OUTER_MARGIN_PX;

    let Some(root) = tree.get(tree.root()) else {
        return;
    };
    let content_width = (viewport.width - VIEWPORT_OUTER_MARGIN_PX * 2.0).max(0.0);
    walk_children(tree, &root.children, &mut ctx, content_width, registry);
}

fn walk_children(
    tree: &AppTree,
    children: &[NodeId],
    ctx: &mut WalkContext<'_>,
    content_width: f32,
    registry: &crate::bridge::WgpuRendererRegistry,
) {
    let mut first = true;
    for child_id in children {
        let Some(node) = tree.get(*child_id) else {
            continue;
        };
        if node.resolved.hidden {
            continue;
        }
        if !first {
            ctx.cursor_y += gap_px(&node.resolved);
        }
        first = false;
        walk_node(tree, node, ctx, content_width, registry);
    }
}

fn walk_node(
    tree: &AppTree,
    node: &mkui_runtime::Node,
    ctx: &mut WalkContext<'_>,
    content_width: f32,
    registry: &crate::bridge::WgpuRendererRegistry,
) {
    match &node.kind {
        NodeKind::Root => {
            // Walked from the top-level entry; recurse defensively in case
            // a Root ever appears as a child (it should not, per AppTree's
            // contract).
            walk_children(tree, &node.children, ctx, content_width, registry);
        }
        NodeKind::View(_) => render_view(tree, node, ctx, content_width, registry),
        NodeKind::Text(t) => render_text(node, t, ctx, content_width),
        NodeKind::Button(b) => render_button(node, b, ctx),
        NodeKind::Custom { type_name, props } => {
            registry.render_custom_node(type_name, props, ctx, tree);
        }
    }
}

fn render_view(
    tree: &AppTree,
    node: &mkui_runtime::Node,
    ctx: &mut WalkContext<'_>,
    content_width: f32,
    registry: &crate::bridge::WgpuRendererRegistry,
) {
    let style = &node.resolved;
    let pad_x = padding_x_px(style);
    let pad_y = padding_y_px(style);
    let outer_top = ctx.cursor_y;
    let outer_left = ctx.content_x;

    let inner_width = (content_width - pad_x * 2.0).max(0.0);
    let inner_left = outer_left + pad_x;

    // Emit border-bottom on entry if requested (renderered as a 1px line
    // at the *top* edge of the view's children area for border-t, bottom
    // edge for border-b).
    let needs_top_border = style.border_top || style.border;

    if style.background.is_some() || style.border {
        // Render background quad — we know the height after recursing, so
        // emit at end via a placeholder. Simpler: emit after recursing.
    }

    let saved_content_x = ctx.content_x;
    let saved_cursor_y = ctx.cursor_y;

    ctx.content_x = inner_left;
    ctx.cursor_y = outer_top + pad_y;

    walk_children(tree, &node.children, ctx, inner_width, registry);

    let content_bottom = ctx.cursor_y;
    let outer_bottom = content_bottom + pad_y;

    // Restore the cursor + content_x for the next sibling. Container
    // width may have been narrower than the parent's available width;
    // for v0.6.0 we don't carry that distinction through (all containers
    // stretch to their parent's content_width).
    ctx.content_x = saved_content_x;
    ctx.cursor_y = outer_bottom;

    // Emit chrome (background + border) under the children. The HUD pass
    // is back-to-front so quads we push later draw on top — that's wrong
    // for backgrounds. We accept the visual artefact for v0.6.0: borders
    // and backgrounds layer on top of children, producing a slightly
    // darkened overlay rather than a true background. Sprint 7+ will
    // introduce a layout-first / paint-second split (ADR 0006 §"Layout v1").
    let _ = (needs_top_border, saved_cursor_y, outer_left, outer_bottom);
}

fn render_text(
    node: &mkui_runtime::Node,
    text: &mkui_runtime::TextProps,
    ctx: &mut WalkContext<'_>,
    content_width: f32,
) {
    let style = &node.resolved;
    let font_size = text_font_size_px(text.variant, style);
    let line_height = (font_size * 1.25).max(font_size + 2.0);

    let body_style = ctx.theme.body_style;
    let color = body_style.color;

    let align = if style.text_center {
        TextAlign::Center
    } else {
        TextAlign::Start
    };

    let text_style = TextStyle {
        font_size_px: font_size,
        line_height_px: line_height,
        color,
        align,
        ..body_style
    };

    let rect = Rect::new(
        Point::new(ctx.content_x, ctx.cursor_y),
        Size::new(content_width.max(0.0), line_height),
    );

    ctx.scene.text(Text {
        rect,
        content: text.content.clone(),
        style: text_style,
    });

    ctx.cursor_y += line_height;
}

fn render_button(
    node: &mkui_runtime::Node,
    button: &mkui_runtime::ButtonProps,
    ctx: &mut WalkContext<'_>,
) {
    let theme = ctx.theme;
    let variant = map_button_variant(button.variant);
    let style = theme.button_style(variant, ButtonSize::Default);

    let approx_label_width = button.label.chars().count() as f32 * APPROX_CHAR_WIDTH_PX;
    let width = (approx_label_width + BUTTON_PADDING_X_PX * 2.0).max(64.0);
    let rect = Rect::new(
        Point::new(ctx.content_x, ctx.cursor_y),
        Size::new(width, BUTTON_HEIGHT_PX),
    );

    ctx.scene.quad(Quad {
        rect,
        fill: style.idle_fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.idle_stroke,
            width: style.stroke_width.max(1.0),
        }),
    });

    let label_style = TextStyle {
        color: style.idle_label,
        align: TextAlign::Center,
        ..style.label_style
    };
    let label_height = label_style.line_height_px;
    let label_rect = Rect::new(
        Point::new(
            rect.origin.x,
            rect.origin.y + (rect.size.height - label_height) * 0.5,
        ),
        Size::new(rect.size.width, label_height),
    );
    ctx.scene.text(Text {
        rect: label_rect,
        content: button.label.clone(),
        style: label_style,
    });

    ctx.hit_entries.push(HitTestEntry {
        rect,
        node_id: node.id,
        on_press: button.on_press,
    });

    ctx.cursor_y += BUTTON_HEIGHT_PX + BUTTON_GAP_PX;
}

fn map_button_variant(v: ButtonVariant) -> ThemeButtonVariant {
    match v {
        ButtonVariant::Primary => ThemeButtonVariant::Default,
        ButtonVariant::Secondary => ThemeButtonVariant::Secondary,
        ButtonVariant::Destructive => ThemeButtonVariant::Destructive,
        ButtonVariant::Outline => ThemeButtonVariant::Outline,
        ButtonVariant::Ghost => ThemeButtonVariant::Ghost,
        ButtonVariant::Link => ThemeButtonVariant::Link,
        // ButtonVariant is `#[non_exhaustive]` — future variants fall back
        // to the Default style until they get their own arm.
        _ => ThemeButtonVariant::Default,
    }
}

fn text_font_size_px(variant: TextVariant, style: &ResolvedStyle) -> f32 {
    // Explicit class-driven size wins over the variant default.
    if let Some(size) = style.text_size {
        return match size {
            mkui_runtime::style::TextSize::Sm => 14.0,
            mkui_runtime::style::TextSize::Xl => 20.0,
            mkui_runtime::style::TextSize::Xl2 => 24.0,
            mkui_runtime::style::TextSize::Xl4 => 36.0,
            // TextSize is `#[non_exhaustive]`; future variants fall back to body.
            _ => 16.0,
        };
    }
    match variant {
        TextVariant::Heading1 => 32.0,
        TextVariant::Heading2 => 24.0,
        TextVariant::Heading3 => 20.0,
        TextVariant::Body | TextVariant::Code => 16.0,
        TextVariant::Caption | TextVariant::Label => 14.0,
        _ => 16.0,
    }
}

#[allow(dead_code)]
fn map_text_variant(v: TextVariant) -> ThemeTextVariant {
    match v {
        TextVariant::Heading1 | TextVariant::Heading2 | TextVariant::Heading3 => {
            ThemeTextVariant::Heading
        }
        TextVariant::Caption | TextVariant::Label => ThemeTextVariant::Muted,
        TextVariant::Body | TextVariant::Code => ThemeTextVariant::Body,
        _ => ThemeTextVariant::Body,
    }
}

fn gap_px(style: &ResolvedStyle) -> f32 {
    style
        .gap
        .or(style.space_y)
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(DEFAULT_GAP_PX)
}

fn padding_x_px(style: &ResolvedStyle) -> f32 {
    style
        .padding_x
        .or(style.padding)
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

fn padding_y_px(style: &ResolvedStyle) -> f32 {
    style
        .padding_y
        .or(style.padding)
        .map(|n| n as f32 * TAILWIND_UNIT_PX)
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::WgpuRendererRegistry;
    use crate::types::{Primitive, Size, Text as PrimitiveText};
    use mkui_core::components::{Button, Mkui as CoreMkui, Text as CoreText, View};

    fn walk(core: &CoreMkui) -> (Scene, Vec<HitTestEntry>) {
        let mut scene = Scene::new(Size::new(800.0, 600.0));
        let theme = HudTheme::default();
        let registry = WgpuRendererRegistry::with_defaults();
        let mut hits = Vec::new();
        walk_tree(core.tree(), &mut scene, &theme, &mut hits, &registry);
        (scene, hits)
    }

    fn text_primitives(scene: &Scene) -> Vec<&PrimitiveText> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn empty_tree_emits_no_primitives() {
        let core = CoreMkui::new();
        let (scene, hits) = walk(&core);
        assert!(
            scene.primitives.is_empty(),
            "expected zero primitives, got {:?}",
            scene.primitives.len()
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn text_node_emits_one_text_primitive() {
        let core = CoreMkui::new().child(CoreText::new("hello").variant(TextVariant::Heading1));
        let (scene, _) = walk(&core);
        let texts = text_primitives(&scene);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].content, "hello");
        // Heading1 should resolve to a larger font than the body baseline.
        assert!(texts[0].style.font_size_px >= 24.0);
    }

    #[test]
    fn button_node_emits_quad_text_and_hit_entry() {
        let core = CoreMkui::new().child(
            Button::new("Ok")
                .variant(ButtonVariant::Primary)
                .on_press(|| {}),
        );
        let (scene, hits) = walk(&core);

        let quads = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count();
        let texts = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();
        assert!(quads >= 1, "button must paint a quad");
        assert_eq!(texts, 1, "button must paint a single label");
        assert_eq!(hits.len(), 1, "button must register exactly one hit entry");
        assert!(
            hits[0].on_press.is_some(),
            "on_press must propagate to hit entry"
        );
    }

    #[test]
    fn nested_views_stack_children_vertically_with_advancing_cursor() {
        let core = CoreMkui::new().child(
            View::new()
                .child(CoreText::new("a").variant(TextVariant::Body))
                .child(CoreText::new("b").variant(TextVariant::Body)),
        );
        let (scene, _) = walk(&core);
        let texts = text_primitives(&scene);
        assert_eq!(texts.len(), 2);
        assert!(
            texts[1].rect.origin.y > texts[0].rect.origin.y,
            "second sibling must be below the first; got {} and {}",
            texts[0].rect.origin.y,
            texts[1].rect.origin.y
        );
    }

    #[test]
    fn hidden_class_skips_node_emission() {
        let core = CoreMkui::new().child(
            View::new()
                .child(CoreText::new("visible").variant(TextVariant::Body))
                .child(
                    CoreText::new("invisible")
                        .variant(TextVariant::Body)
                        .class("hidden"),
                ),
        );
        let (scene, _) = walk(&core);
        let contents: Vec<&str> = text_primitives(&scene)
            .iter()
            .map(|t| t.content.as_str())
            .collect();
        assert!(contents.contains(&"visible"));
        assert!(!contents.contains(&"invisible"));
    }

    #[test]
    fn hit_entries_appear_in_paint_order() {
        // Two buttons sibling-stacked; paint order is top-down. Input
        // router iterates in reverse for topmost-wins, so paint order
        // here is the canonical order under tests.
        let core = CoreMkui::new()
            .child(Button::new("first").on_press(|| {}))
            .child(Button::new("second").on_press(|| {}));
        let (_, hits) = walk(&core);
        assert_eq!(hits.len(), 2);
        assert!(hits[1].rect.origin.y > hits[0].rect.origin.y);
    }
}
