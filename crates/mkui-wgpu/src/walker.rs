//! AppTree to Scene projection for the wgpu backend.
//! The walker consumes `mkui_runtime::AppTree`, emits `Scene` primitives, and
//! collects hit-test data for pointer routing.
//!
//! The walker is the wgpu-side equivalent of
//! `mkui_web::render::render_tree` (per ADR 0006): it consumes the
//! runtime tree directly, emits [`crate::Primitive`]s through the
//! tessellation pipeline already wired up by the v0.4.x tessellator port
//! (ADR 0004), and collects a per-frame `Vec<HitTestEntry>` for the
//! input router to reverse-paint-order hit-test against.
//!
//! ## Public entry
//!
//! ```ignore
//! pub fn walk_app_tree(
//!     tree: &mkui_runtime::AppTree,
//!     registry: &WgpuRendererRegistry,
//!     options: &WalkOptions,
//! ) -> Result<WalkOutput, MkuiError>;
//! ```
//!
//! This is the round-10 §"Concrete Shape" signature. The function
//! allocates a fresh `Scene` / hit-test vec / layouts vec per call,
//! returns them in a `WalkOutput`, and lets callers (`WgpuApp`,
//! tests, future tooling) consume the result without threading mut
//! refs through their event-loop code.
//!
//! ## Layout model
//!
//! v0.6.0 implements a deliberately small layout: top-down vertical
//! flow with class-driven padding + gap + text/button sizing. This is
//! enough to render `examples/showcase-common::create_showcase_ui`
//! recognisably on wgpu. Full flex / grid layout is deferred (Sprint
//! 7+); a future `mkui-layout` shared module is the reserved seam
//! (ADR 0006 §"Out of scope"). The eager-rebuild model lets a richer
//! layout drop in without re-architecting the walker.
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

use mkui_core::error::MkuiError;
use mkui_runtime::{
    ActionId, AppTree, ButtonVariant, ColorRole, NodeId, NodeKind, ResolvedStyle, TextVariant,
};

use crate::bridge::{WgpuRenderCtx, WgpuRenderOutcome, WgpuRendererRegistry};
use crate::theme::{
    ButtonSize, ButtonVariant as ThemeButtonVariant, TextVariant as ThemeTextVariant, WgpuTheme,
};
use crate::types::{
    Color, CornerRadii, Point, Primitive, Quad, Rect, Scene, Size, Stroke, Text, TextAlign,
    TextStyle,
};

/// One interactive region collected during the walk. The input router
/// hit-tests `rect` and fires `action` (looked up in the tree's
/// `ActionRegistry`).
///
/// Per-frame collection is `Vec` rather than a long-lived map because
/// the walker rebuilds the list each frame (eager rebuild on dirty
/// signal — ADR 0006). Reverse iteration handles overlap (topmost-wins).
///
/// Field names match the round-10 §"Concrete Shape" sketch (`node`,
/// `rect`, `action`).
#[derive(Debug, Clone, Copy)]
pub struct HitTestEntry {
    pub node: NodeId,
    pub rect: Rect,
    pub action: Option<ActionId>,
}

/// Per-node layout snapshot captured during the walk. The bridge
/// populates one [`NodeLayout`] per visible built-in node (View, Text,
/// Button) so future tooling (visual regression, devtools, focus
/// management) can introspect the laid-out geometry without re-running
/// the walker.
///
/// `Custom`-node renderers may push their own [`NodeLayout`] entries
/// through [`WalkOutput::layouts`] in a future sprint; v0.6.0 records
/// only the built-in nodes.
#[derive(Debug, Clone, Copy)]
pub struct NodeLayout {
    pub node: NodeId,
    pub rect: Rect,
}

/// Caller-supplied walker configuration.
///
/// `viewport` is the logical-pixel size of the surface to lay out
/// against. `theme` is consumed by value so the walker can hand a
/// shared `&WgpuTheme` to every node + extension renderer without
/// threading a lifetime through every internal helper.
#[derive(Debug, Clone, Copy)]
pub struct WalkOptions {
    pub viewport: Size,
    pub theme: WgpuTheme,
}

/// Output of one [`walk_app_tree`] pass: the freshly built scene, the
/// per-frame hit-test list, and the per-node layout snapshots. Returned
/// as a single struct so callers consume the walker through one move
/// rather than juggling three out-parameters.
#[derive(Debug)]
pub struct WalkOutput {
    pub scene: Scene,
    pub hit_tests: Vec<HitTestEntry>,
    pub layouts: Vec<NodeLayout>,
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

/// Walk `tree` from the root and return a fresh [`WalkOutput`] (scene,
/// hit-tests, per-node layouts).
///
/// Eager rebuild: callers invoke this fresh per frame once
/// `AppTree::is_dirty()` returns true (or on first paint). The function
/// does not look at `is_dirty` itself; the caller owns the gate.
///
/// `Result<_, MkuiError>` is plumbed through so future extension
/// renderers that surface real errors can flow them up the stack;
/// v0.6.0 never returns `Err`.
pub fn walk_app_tree(
    tree: &AppTree,
    registry: &WgpuRendererRegistry,
    options: &WalkOptions,
) -> Result<WalkOutput, MkuiError> {
    let mut scene = Scene::new(options.viewport);
    let mut hit_tests: Vec<HitTestEntry> = Vec::new();
    let mut layouts: Vec<NodeLayout> = Vec::new();

    let content_width = (options.viewport.width - VIEWPORT_OUTER_MARGIN_PX * 2.0).max(0.0);

    {
        let mut ctx = WgpuRenderCtx {
            tree,
            registry,
            scene: &mut scene,
            theme: &options.theme,
            hits: &mut hit_tests,
            viewport_width: options.viewport.width,
            cursor_y: VIEWPORT_OUTER_MARGIN_PX,
            content_x: VIEWPORT_OUTER_MARGIN_PX,
            current_foreground: options.theme.body_style().color,
        };

        if let Some(root) = tree.get(tree.root()) {
            walk_children(tree, &root.children, &mut ctx, content_width, &mut layouts)?;
        }
    }

    Ok(WalkOutput {
        scene,
        hit_tests,
        layouts,
    })
}

fn walk_children(
    tree: &AppTree,
    children: &[NodeId],
    ctx: &mut WgpuRenderCtx<'_>,
    content_width: f32,
    layouts: &mut Vec<NodeLayout>,
) -> Result<(), MkuiError> {
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
        walk_node(tree, node, ctx, content_width, layouts)?;
    }
    Ok(())
}

fn walk_node(
    tree: &AppTree,
    node: &mkui_runtime::Node,
    ctx: &mut WgpuRenderCtx<'_>,
    content_width: f32,
    layouts: &mut Vec<NodeLayout>,
) -> Result<(), MkuiError> {
    match &node.kind {
        NodeKind::Root => {
            // Walked from the top-level entry; recurse defensively in
            // case a Root ever appears as a child (it should not, per
            // AppTree's contract).
            walk_children(tree, &node.children, ctx, content_width, layouts)?;
        }
        NodeKind::View(_) => render_view(tree, node, ctx, content_width, layouts)?,
        NodeKind::Text(t) => render_text(node, t, ctx, content_width, layouts),
        NodeKind::Button(b) => render_button(node, b, ctx, layouts),
        NodeKind::Custom { type_name, props } => {
            let outcome = ctx
                .registry
                .render_custom_node(type_name, node, props, ctx)?;
            if matches!(outcome, WgpuRenderOutcome::RecurseChildren) {
                walk_children(tree, &node.children, ctx, content_width, layouts)?;
            }
        }
    }
    Ok(())
}

fn render_view(
    tree: &AppTree,
    node: &mkui_runtime::Node,
    ctx: &mut WgpuRenderCtx<'_>,
    content_width: f32,
    layouts: &mut Vec<NodeLayout>,
) -> Result<(), MkuiError> {
    let style = &node.resolved;
    let pad_x = padding_x_px(style);
    let pad_y = padding_y_px(style);
    let outer_top = ctx.cursor_y;
    let outer_left = ctx.content_x;

    let inner_width = (content_width - pad_x * 2.0).max(0.0);
    let inner_left = outer_left + pad_x;

    // `bg-card` (today the only class that sets `ResolvedStyle::background`)
    // paints a themed quad behind the container's children — previously
    // this role was parsed but never consumed, so the class had no visual
    // effect on wgpu (issue #205). The quad's final rect isn't known until
    // the children below have been laid out (height grows with content),
    // but it must still land *behind* them in `Scene::primitives`' paint
    // order, so a placeholder is reserved here and patched in place once
    // `outer_bottom` is known.
    let bg_index = style.background.map(|_| {
        ctx.scene.primitives.push(Primitive::Quad(Quad {
            rect: Rect::new(Point::new(outer_left, outer_top), Size::new(0.0, 0.0)),
            fill: Color::TRANSPARENT,
            corner_radii: CornerRadii::all(0.0),
            stroke: None,
        }));
        ctx.scene.primitives.len() - 1
    });

    let saved_content_x = ctx.content_x;
    // `text-*` on a container (e.g. the showcase's `bg-card
    // text-card-foreground` card) must cascade to descendant `Text` nodes
    // that set no color of their own — the same inheritance `mkui-web`
    // gets for free from CSS. Save/restore around the recursion mirrors
    // the existing `content_x` save/restore just above.
    let saved_foreground = ctx.current_foreground;
    if let Some(role) = style.foreground {
        ctx.current_foreground = color_role_foreground(role, ctx.theme);
    }

    ctx.content_x = inner_left;
    ctx.cursor_y = outer_top + pad_y;

    walk_children(tree, &node.children, ctx, inner_width, layouts)?;

    let content_bottom = ctx.cursor_y;
    let outer_bottom = content_bottom + pad_y;

    // Restore the cursor + content_x for the next sibling. Container
    // width may have been narrower than the parent's available width;
    // for v0.6.0 we don't carry that distinction through (all containers
    // stretch to their parent's content_width).
    ctx.content_x = saved_content_x;
    ctx.cursor_y = outer_bottom;
    ctx.current_foreground = saved_foreground;

    let rect = Rect::new(
        Point::new(outer_left, outer_top),
        Size::new(content_width.max(0.0), (outer_bottom - outer_top).max(0.0)),
    );

    if let (Some(role), Some(idx)) = (style.background, bg_index) {
        let fill = color_role_fill(role, ctx.theme);
        let card = ctx.theme.card();
        let corner_radius = if style.rounded_lg {
            card.corner_radius
        } else {
            0.0
        };
        let stroke = if style.border {
            Some(Stroke {
                color: card.stroke,
                width: card.stroke_width,
            })
        } else {
            None
        };
        ctx.scene.primitives[idx] = Primitive::Quad(Quad {
            rect,
            fill,
            corner_radii: CornerRadii::all(corner_radius),
            stroke,
        });
    }

    layouts.push(NodeLayout {
        node: node.id,
        rect,
    });

    Ok(())
}

fn render_text(
    node: &mkui_runtime::Node,
    text: &mkui_runtime::TextProps,
    ctx: &mut WgpuRenderCtx<'_>,
    content_width: f32,
    layouts: &mut Vec<NodeLayout>,
) {
    let style = &node.resolved;
    let font_size = text_font_size_px(text.variant, style);
    let line_height = (font_size * 1.25).max(font_size + 2.0);

    let body_style = ctx.theme.body_style();
    // The node's own `text-*` class (e.g. `text-foreground`) wins; absent
    // one, it inherits the nearest ancestor `View`'s foreground role (set
    // in `render_view`), falling back to the theme's default body color.
    let color = match style.foreground {
        Some(role) => color_role_foreground(role, ctx.theme),
        None => ctx.current_foreground,
    };

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

    layouts.push(NodeLayout {
        node: node.id,
        rect,
    });

    ctx.cursor_y += line_height;
}

fn render_button(
    node: &mkui_runtime::Node,
    button: &mkui_runtime::ButtonProps,
    ctx: &mut WgpuRenderCtx<'_>,
    layouts: &mut Vec<NodeLayout>,
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

    ctx.hits.push(HitTestEntry {
        rect,
        node: node.id,
        action: button.on_press,
    });
    layouts.push(NodeLayout {
        node: node.id,
        rect,
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

/// Resolve a `bg-*` [`ColorRole`] to the fill the themed background quad
/// paints. `mkui-web` gets this mapping for free from CSS variables
/// (`var(--card)`); the wgpu walker has to do it explicitly (issue #205).
fn color_role_fill(role: ColorRole, theme: &WgpuTheme) -> Color {
    match role {
        ColorRole::Card => theme.tokens.card_fill,
        // `ColorRole` is `#[non_exhaustive]` and today only `bg-card` ever
        // sets `ResolvedStyle::background` — the other roles are text-only.
        // Fall back to the neutral surface token rather than panic.
        _ => theme.tokens.surface,
    }
}

/// Resolve a `text-*` [`ColorRole`] to a concrete text color.
fn color_role_foreground(role: ColorRole, theme: &WgpuTheme) -> Color {
    match role {
        ColorRole::Card => theme.tokens.card_fill,
        ColorRole::CardForeground => theme.tokens.surface_foreground,
        ColorRole::Foreground => theme.tokens.surface_foreground,
        ColorRole::MutedForeground => theme.tokens.muted_foreground,
        _ => theme.tokens.surface_foreground,
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

// allow: forward-compat scaffold — the runtime→theme `TextVariant` mapping is
// not yet wired into the text render path (unlike its sibling
// `map_button_variant`, which has a live consumer). Kept here so the mapping
// lives beside `text_font_size_px` for when the theme text path lands.
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

    fn walk(core: &CoreMkui) -> WalkOutput {
        let registry = WgpuRendererRegistry::with_defaults();
        let options = WalkOptions {
            viewport: Size::new(800.0, 600.0),
            theme: WgpuTheme::default(),
        };
        walk_app_tree(core.tree(), &registry, &options).expect("walk")
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
        let out = walk(&core);
        assert!(
            out.scene.primitives.is_empty(),
            "expected zero primitives, got {:?}",
            out.scene.primitives.len()
        );
        assert!(out.hit_tests.is_empty());
        assert!(out.layouts.is_empty());
    }

    #[test]
    fn text_node_emits_one_text_primitive() {
        let core = CoreMkui::new().child(CoreText::new("hello").variant(TextVariant::Heading1));
        let out = walk(&core);
        let texts = text_primitives(&out.scene);
        assert_eq!(texts.len(), 1);
        assert_eq!(texts[0].content, "hello");
        // Heading1 should resolve to a larger font than the body baseline.
        assert!(texts[0].style.font_size_px >= 24.0);
        // Text node records a single layout entry.
        assert_eq!(out.layouts.len(), 1);
        assert_eq!(out.layouts[0].node, texts_first_node(&core));
    }

    fn texts_first_node(core: &CoreMkui) -> NodeId {
        let root = core.tree().get(core.tree().root()).expect("root");
        root.children[0]
    }

    #[test]
    fn button_node_emits_quad_text_and_hit_entry() {
        let core = CoreMkui::new().child(
            Button::new("Ok")
                .variant(ButtonVariant::Primary)
                .on_press(|| {}),
        );
        let out = walk(&core);

        let quads = out
            .scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count();
        let texts = out
            .scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();
        assert!(quads >= 1, "button must paint a quad");
        assert_eq!(texts, 1, "button must paint a single label");
        assert_eq!(
            out.hit_tests.len(),
            1,
            "button must register exactly one hit entry"
        );
        assert!(
            out.hit_tests[0].action.is_some(),
            "action must propagate to hit entry"
        );
    }

    #[test]
    fn nested_views_stack_children_vertically_with_advancing_cursor() {
        let core = CoreMkui::new().child(
            View::new()
                .child(CoreText::new("a").variant(TextVariant::Body))
                .child(CoreText::new("b").variant(TextVariant::Body)),
        );
        let out = walk(&core);
        let texts = text_primitives(&out.scene);
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
        let out = walk(&core);
        let contents: Vec<&str> = text_primitives(&out.scene)
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
        let out = walk(&core);
        assert_eq!(out.hit_tests.len(), 2);
        assert!(out.hit_tests[1].rect.origin.y > out.hit_tests[0].rect.origin.y);
    }

    #[test]
    fn walk_app_tree_returns_a_walk_output() {
        // Smoke that the round-10 §"Concrete Shape" return type holds:
        // a single struct with scene/hit_tests/layouts, not three out
        // parameters.
        let core = CoreMkui::new().child(Button::new("Ok").on_press(|| {}));
        let out = walk(&core);
        // All three fields are populated for this tree.
        assert!(!out.scene.primitives.is_empty());
        assert_eq!(out.hit_tests.len(), 1);
        assert!(!out.layouts.is_empty());
    }

    fn walk_with_theme(core: &CoreMkui, theme: WgpuTheme) -> WalkOutput {
        let registry = WgpuRendererRegistry::with_defaults();
        let options = WalkOptions {
            viewport: Size::new(800.0, 600.0),
            theme,
        };
        walk_app_tree(core.tree(), &registry, &options).expect("walk")
    }

    fn quads(scene: &Scene) -> Vec<&Quad> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect()
    }

    /// `bg-card` used to parse into `ResolvedStyle::background` and then
    /// go nowhere — the wgpu walker never consumed it, so a card-styled
    /// `View` painted no background at all (issue #205). It must now
    /// paint the themed card fill, behind its children in paint order.
    #[test]
    fn bg_card_class_paints_the_themed_card_fill_behind_its_children() {
        let core = CoreMkui::new().child(
            View::new()
                .class("bg-card")
                .child(CoreText::new("inside").variant(TextVariant::Body)),
        );
        let theme = WgpuTheme::default();
        let out = walk_with_theme(&core, theme);

        let card_quads = quads(&out.scene)
            .into_iter()
            .filter(|q| q.fill == theme.tokens.card_fill)
            .count();
        assert_eq!(
            card_quads, 1,
            "bg-card must paint exactly one quad filled with the theme's card_fill token"
        );

        let bg_pos = out
            .scene
            .primitives
            .iter()
            .position(|p| matches!(p, Primitive::Quad(q) if q.fill == theme.tokens.card_fill))
            .expect("card background quad");
        let text_pos = out
            .scene
            .primitives
            .iter()
            .position(|p| matches!(p, Primitive::Text(t) if t.content == "inside"))
            .expect("child text");
        assert!(
            bg_pos < text_pos,
            "the card background must paint before (behind) its child text"
        );
    }

    /// A `View`'s own `text-*` class (no `Text`-node class of its own)
    /// must cascade to child `Text` nodes — the same inheritance
    /// `mkui-web` gets for free from the CSS cascade.
    #[test]
    fn foreground_role_on_a_view_cascades_to_untagged_child_text() {
        let core = CoreMkui::new().child(
            View::new()
                .class("bg-card text-card-foreground")
                .child(CoreText::new("inherits").variant(TextVariant::Body)),
        );
        let theme = WgpuTheme::default();
        let out = walk_with_theme(&core, theme);

        let text = text_primitives(&out.scene)[0];
        assert_eq!(
            text.style.color,
            color_role_foreground(ColorRole::CardForeground, &theme),
            "text with no color class of its own must inherit the ancestor's text-card-foreground"
        );
    }

    /// A `Text` node's own `text-*` class wins over any ancestor role.
    #[test]
    fn foreground_role_on_the_text_node_itself_overrides_the_ancestor() {
        let core = CoreMkui::new().child(
            View::new().class("bg-card text-card-foreground").child(
                CoreText::new("overridden")
                    .variant(TextVariant::Body)
                    .class("text-muted-foreground"),
            ),
        );
        let theme = WgpuTheme::default();
        let out = walk_with_theme(&core, theme);

        let text = text_primitives(&out.scene)[0];
        assert_eq!(
            text.style.color,
            color_role_foreground(ColorRole::MutedForeground, &theme),
        );
    }

    /// Acceptance criterion #205.2, exercised end-to-end through the real
    /// declarative tree + walker path (not just the resolver unit tests
    /// in `theme.rs`): the identical tree, walked through the identical
    /// `walk_app_tree`/`render_view`/`render_text` code, must render a
    /// different background and text color when only the `WgpuTheme`
    /// swaps — no call site above the walk changes.
    #[test]
    fn swapping_the_theme_changes_rendered_colors_with_no_tree_change() {
        let core = CoreMkui::new().child(
            View::new()
                .class("bg-card text-card-foreground")
                .child(CoreText::new("swap me").variant(TextVariant::Body)),
        );

        let default_theme = WgpuTheme::default();
        let mut swapped = WgpuTheme::default();
        swapped.tokens.card_fill = Color::rgba(0.9, 0.1, 0.1, 1.0);
        swapped.tokens.surface_foreground = Color::rgba(0.1, 0.9, 0.1, 1.0);
        assert_ne!(
            default_theme, swapped,
            "the two themes must actually differ"
        );

        let default_out = walk_with_theme(&core, default_theme);
        let swapped_out = walk_with_theme(&core, swapped);

        let default_bg = quads(&default_out.scene)[0].fill;
        let swapped_bg = quads(&swapped_out.scene)[0].fill;
        assert_ne!(default_bg, swapped_bg);
        assert_eq!(swapped_bg, swapped.tokens.card_fill);

        let default_text = text_primitives(&default_out.scene)[0].style.color;
        let swapped_text = text_primitives(&swapped_out.scene)[0].style.color;
        assert_ne!(default_text, swapped_text);
        assert_eq!(swapped_text, swapped.tokens.surface_foreground);
    }
}
