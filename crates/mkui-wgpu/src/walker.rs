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
use mkui_text::{FontId, HintingMode, LayoutSpec, TextSystem, VariationSettings};

use crate::bridge::{WgpuRenderCtx, WgpuRenderOutcome, WgpuRendererRegistry};
use crate::tessellation::map_align;
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
    /// Corner radii of the rendered silhouette this entry hit-tests against
    /// (#176). Hit-testing against `rect` alone would accept clicks in the
    /// triangular slivers the renderer chamfers away from a rounded quad;
    /// carrying the same radii here lets [`crate::pointer::hit_test`] reject
    /// those pixels via [`crate::types::rect_contains_rounded`].
    pub corner_radii: CornerRadii,
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
/// threading a lifetime through every internal helper. `text_system` is
/// the same text system the later render pass tessellates glyphs with
/// (#179 review follow-up) — `render_text` measures each node's real
/// wrapped line count against it, so the rect height it reserves and
/// what `tessellate_text` actually draws can never disagree. No `Debug`
/// derive: `&dyn TextSystem` isn't `Debug`.
#[derive(Clone, Copy)]
pub struct WalkOptions<'a> {
    pub viewport: Size,
    pub theme: WgpuTheme,
    pub text_system: &'a dyn TextSystem,
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
    options: &WalkOptions<'_>,
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
            text_system: options.text_system,
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

    // #179 (review follow-up): reserve exactly as many lines as the real
    // text system produces at this width — not a character-count
    // estimate. An estimate ignores word boundaries and explicit
    // newlines, so it can (and did) under-reserve for ordinary settings
    // text; `tessellate_text` caps the lines it actually draws at
    // `floor(rect.height / line_height)`, so under-reserving silently
    // drops the rest. `wrapped_line_count` calls the identical `layout`
    // mkui-text method with the identical spec `tessellate_text` will use
    // later (only `max_lines` differs, and that's derived FROM this
    // rect's height) — same input, so the two passes can never disagree.
    let line_count = wrapped_line_count(
        ctx.text_system,
        &text.content,
        font_size,
        line_height,
        align,
        content_width,
    );
    let rect_height = line_height * line_count as f32;

    let rect = Rect::new(
        Point::new(ctx.content_x, ctx.cursor_y),
        Size::new(content_width.max(0.0), rect_height),
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

    ctx.cursor_y += rect_height;
}

/// How many lines `text_system` actually wraps `content` into at
/// `content_width` (#179 review follow-up — replaces an earlier
/// character-count estimate). Builds the identical [`LayoutSpec`]
/// `tessellation::tessellate_text` builds for the same `Text` primitive
/// (down to the bitmap-only `font_id` — see that function's own comment
/// for why), with `max_lines: None` so this call learns the *unclamped*
/// line count rather than reproducing a clamp. `render_text`'s call site
/// reserves that many lines of rect height, and `tessellate_text` later
/// derives its own `max_lines` from that height — same text system, same
/// spec, same width, so the two can never disagree.
fn wrapped_line_count(
    text_system: &dyn TextSystem,
    content: &str,
    font_size_px: f32,
    line_height_px: f32,
    align: TextAlign,
    content_width: f32,
) -> u32 {
    let spec = LayoutSpec {
        font_id: FontId::BITMAP,
        font_generation: 0,
        font_size_px,
        line_height_px,
        align: map_align(align),
        max_lines: None,
        hinting: HintingMode::None,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
    };
    let runs = text_system.layout(content, &spec, Some(content_width.max(1.0)));
    (runs.len() as u32).max(1)
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

    let corner_radii = CornerRadii::all(style.corner_radius);
    ctx.scene.quad(Quad {
        rect,
        fill: style.idle_fill,
        corner_radii,
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
        corner_radii,
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
    use mkui_text::BitmapTextSystem;

    fn walk(core: &CoreMkui) -> WalkOutput {
        let registry = WgpuRendererRegistry::with_defaults();
        let text_system = BitmapTextSystem::new();
        let options = WalkOptions {
            viewport: Size::new(800.0, 600.0),
            theme: WgpuTheme::default(),
            text_system: &text_system,
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

    /// Focused regression for the reviewer's #179 repro: a character-count
    /// estimate reserved 2 lines for "Accessibility configuration
    /// preferences" as a Heading1 at 1280x720 @ ui_scale 3 (viewport
    /// 426.67x240, the same folding `WgpuApp::set_ui_scale` does), while
    /// the real bitmap layout needs 3 — `tessellate_text`'s
    /// `max_lines = floor(rect.height / line_height)` clamp then silently
    /// dropped the third line. Asserts the walker's reserved height
    /// matches the real, *unclamped* text-system layout exactly, so no
    /// line can ever be clamped away — independent of what the exact
    /// line count happens to be for this string/font/width combination.
    #[test]
    fn render_text_never_under_reserves_height_for_a_wrapped_heading() {
        let viewport = Size::new(1280.0 / 3.0, 720.0 / 3.0);
        let core = CoreMkui::new().child(
            CoreText::new("Accessibility configuration preferences").variant(TextVariant::Heading1),
        );
        let registry = WgpuRendererRegistry::with_defaults();
        let text_system = BitmapTextSystem::new();
        let options = WalkOptions {
            viewport,
            theme: WgpuTheme::default(),
            text_system: &text_system,
        };
        let out = walk_app_tree(core.tree(), &registry, &options).expect("walk");

        let text = text_primitives(&out.scene)[0];
        let line_height = text.style.line_height_px.max(1.0);
        let reserved_lines = ((text.rect.size.height / line_height).round() as usize).max(1);

        let spec = LayoutSpec {
            font_id: FontId::BITMAP,
            font_generation: 0,
            font_size_px: text.style.font_size_px,
            line_height_px: line_height,
            align: map_align(text.style.align),
            max_lines: None,
            hinting: HintingMode::None,
            variations: VariationSettings::empty(),
            synthesis_flags: 0,
        };
        let unclamped_runs = text_system.layout(&text.content, &spec, Some(text.rect.size.width));

        assert!(
            unclamped_runs.len() > 1,
            "this repro must actually wrap into more than one line, or it \
             isn't exercising the bug it regresses — got {} line(s) at \
             width {}",
            unclamped_runs.len(),
            text.rect.size.width
        );
        assert_eq!(
            reserved_lines,
            unclamped_runs.len(),
            "render_text reserved {reserved_lines} line(s) but the real, \
             unclamped layout needs {} — tessellate_text's max_lines clamp \
             would silently drop the rest",
            unclamped_runs.len()
        );
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
        let text_system = BitmapTextSystem::new();
        let options = WalkOptions {
            viewport: Size::new(800.0, 600.0),
            theme,
            text_system: &text_system,
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

    /// #179 acceptance criterion #1: "menus, HUD and settings stay readable
    /// and unclipped at every listed size, at 1x/2x/3x scale factors."
    ///
    /// `WgpuApp` folds `ui_scale` into the viewport it hands this walker
    /// (`viewport = physical / (scale_factor * ui_scale)`, ADR 0006
    /// §"UI-scale setting") — so, from the walker's point of view, "every
    /// listed size at 1x/2x/3x" is exactly "every listed size divided by
    /// 1/2/3". This walks a small realistic menu (a heading + two short
    /// buttons — representative content, not an unbounded stress string)
    /// at that full size x scale matrix and asserts every laid-out rect
    /// stays inside the viewport, and every text run's wrapped width stays
    /// inside its own rect. The ~390x844 phone-portrait size is excluded —
    /// the issue ties it to the WASM/mobile gate tracked separately (#180).
    ///
    /// This intentionally does not cover arbitrarily long content: a
    /// button's width (`BUTTON_PADDING_X_PX` + label width) is not clamped
    /// to `content_width`, and there is no scroll area yet (#194, an
    /// existing, orthogonal gap predating this issue) — long enough content
    /// at a small enough effective viewport can still overflow. That is a
    /// pre-existing limitation of the fixed-size walker, not something
    /// `ui_scale` introduces; `ui_scale` just makes the effective viewport
    /// smaller, the same way a narrow physical window already could.
    #[test]
    fn menu_content_stays_within_the_viewport_across_the_179_acceptance_matrix() {
        let logical_sizes = [(1280.0_f32, 800.0_f32), (1280.0, 720.0), (3440.0, 1440.0)];
        let ui_scales = [1.0_f32, 2.0, 3.0];
        let text_system = BitmapTextSystem::new();
        // Tracks the most lines any text in the matrix actually wrapped
        // into, so the test can confirm it exercised wrapping at all
        // (see the `assert!` after the loops) rather than passing because
        // "Display & resolution settings" happened to always fit on one
        // line — which would make every assertion below pass trivially.
        let mut max_runs_seen = 0usize;

        for (width, height) in logical_sizes {
            for ui_scale in ui_scales {
                let viewport = Size::new(width / ui_scale, height / ui_scale);
                let core = CoreMkui::new().child(
                    View::new()
                        // A realistic settings heading, long enough to
                        // actually need wrapping at the narrowest
                        // size/ui_scale combo in this matrix — "Settings"
                        // alone never would have exercised the wrap path
                        // at all, so it couldn't have caught the #182
                        // wrap-vs-#179-layout mismatch this test guards.
                        .child(
                            CoreText::new("Display & resolution settings")
                                .variant(TextVariant::Heading1),
                        )
                        .child(
                            Button::new("Save")
                                .variant(ButtonVariant::Primary)
                                .on_press(|| {}),
                        )
                        .child(
                            Button::new("Cancel")
                                .variant(ButtonVariant::Outline)
                                .on_press(|| {}),
                        ),
                );
                let registry = WgpuRendererRegistry::with_defaults();
                let options = WalkOptions {
                    viewport,
                    theme: WgpuTheme::default(),
                    text_system: &text_system,
                };
                let out = walk_app_tree(core.tree(), &registry, &options).expect("walk");

                for layout in &out.layouts {
                    assert!(
                        layout.rect.origin.x + layout.rect.size.width <= viewport.width + 0.5,
                        "node {:?} rect {:?} overflows viewport width {} \
                         at {width}x{height} @ ui_scale {ui_scale}",
                        layout.node,
                        layout.rect,
                        viewport.width
                    );
                    // Vertical containment, not just horizontal: the last
                    // sibling's bottom edge is the walker's own running
                    // `cursor_y`, so this also catches an estimate that
                    // reserves too little height for a wrapped heading
                    // (the overflow would show up as the *next* sibling
                    // sitting off the bottom of the viewport, not as the
                    // heading's own rect).
                    assert!(
                        layout.rect.origin.y + layout.rect.size.height <= viewport.height + 0.5,
                        "node {:?} rect {:?} overflows viewport height {} \
                         at {width}x{height} @ ui_scale {ui_scale}",
                        layout.node,
                        layout.rect,
                        viewport.height
                    );
                }

                for text in text_primitives(&out.scene) {
                    let spec = LayoutSpec {
                        font_id: FontId::BITMAP,
                        font_generation: 0,
                        font_size_px: text.style.font_size_px,
                        line_height_px: text.style.line_height_px,
                        align: map_align(text.style.align),
                        // `None` here mirrors the real wrap pass, not the
                        // renderer's clamp — `max_lines` is asserted
                        // separately below, against the renderer's own
                        // formula, so this test fails if the walker's
                        // line-count *estimate* (`render_text`'s call to
                        // `estimate_text_line_count`) ever falls short of
                        // what the text system actually produces.
                        max_lines: None,
                        hinting: HintingMode::None,
                        variations: VariationSettings::empty(),
                        synthesis_flags: 0,
                    };
                    let runs = text_system.layout(&text.content, &spec, Some(text.rect.size.width));
                    max_runs_seen = max_runs_seen.max(runs.len());
                    let line_height = text.style.line_height_px.max(1.0);
                    let rendered_max_lines =
                        ((text.rect.size.height / line_height).floor() as usize).max(1);
                    assert!(
                        runs.len() <= rendered_max_lines,
                        "text {:?} wraps into {} lines but render_text's rect \
                         (height {}) only reserved {} — tessellate_text will \
                         silently drop the rest at {width}x{height} @ ui_scale {ui_scale}",
                        text.content,
                        runs.len(),
                        text.rect.size.height,
                        rendered_max_lines
                    );
                    for run in &runs {
                        for glyph in &run.glyphs {
                            let right_edge = run.origin_x_px + glyph.x_px + glyph.x_advance_px;
                            assert!(
                                right_edge <= text.rect.size.width + 0.5,
                                "text {:?} wraps past its own rect width {} \
                                 (glyph right edge {right_edge}) at {width}x{height} @ ui_scale {ui_scale}",
                                text.content,
                                text.rect.size.width
                            );
                        }
                    }
                }
            }
        }

        assert!(
            max_runs_seen > 1,
            "the matrix never produced a wrapped (multi-line) text run — \
             this test would pass even if render_text's line-count \
             reservation were reverted to always exactly one line, so it \
             isn't actually exercising the wrap path it claims to guard"
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
