//! `WgpuRenderable` trait + `WgpuRendererRegistry` — wgpu-side extension
//! point for [`mkui_runtime::NodeKind::Custom`] nodes.
//!
//! Built-in node kinds (`View` / `Text` / `Button`) render through the
//! fixed paths in [`crate::walker`]; only `Custom` dispatches through the
//! registry, keyed by `type_name`. The shape mirrors
//! [`mkui_web::render::WebRendererRegistry`] — backend-local trait
//! placement per ADR 0006 + Codex round-7 Q1 ratification (the wgpu
//! renderer trait stays in `mkui-wgpu`, not in `mkui-runtime` or
//! `mkui-core`).
//!
//! ## Trait shape (Codex round-10 §"Concrete Shape")
//!
//! ```ignore
//! pub trait WgpuRenderable: 'static {
//!     fn type_name(&self) -> &str;
//!     fn render(
//!         &self,
//!         node: &mkui_runtime::Node,
//!         props: &serde_json::Value,
//!         ctx: &mut WgpuRenderCtx<'_>,
//!     ) -> Result<WgpuRenderOutcome, MkuiError>;
//! }
//! ```
//!
//! Returning `WgpuRenderOutcome::RecurseChildren` tells the walker to
//! continue into the node's children after the custom render returns
//! (the layout convention for transparent container-style extensions);
//! `WgpuRenderOutcome::ChildrenHandled` tells it to skip — the renderer
//! either consumed the children itself or chose not to walk them.

use std::collections::HashMap;

use mkui_core::error::MkuiError;
use mkui_runtime::{AppTree, Node};

use crate::theme::WgpuTheme;
use crate::types::Scene;
use crate::walker::HitTestEntry;

/// Per-render outcome returned by a [`WgpuRenderable`]. The walker uses
/// it to decide whether to recurse into the node's children after the
/// custom render returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WgpuRenderOutcome {
    /// The renderer emitted its own primitives; the walker should
    /// continue into the node's children to render them too. Use this
    /// for transparent containers (e.g. a `Card` extension that just
    /// adds a background quad).
    RecurseChildren,
    /// The renderer emitted everything it wanted from the node and its
    /// children. The walker skips children. Use this for atoms that
    /// have no children (Badge, Dot) or for extensions that walked
    /// their own children directly via [`WgpuRenderCtx::tree`].
    ChildrenHandled,
}

/// Backend-specific render trait for [`mkui_runtime::NodeKind::Custom`]
/// node types on the wgpu backend.
///
/// Built-in `View` / `Text` / `Button` rendering is hard-wired inside
/// [`crate::walker`]; only extension types go through this trait. The
/// callback receives a [`WgpuRenderCtx`] so the custom renderer can
/// push primitives + hit entries through the same buffers the built-ins
/// use (per ADR 0006 — no per-frame Vec allocations from extension
/// code).
pub trait WgpuRenderable: 'static {
    /// `type_name` the registry keys this renderer by. Must match the
    /// `type_name` field stored on the [`mkui_runtime::NodeKind::Custom`]
    /// nodes the binding produces.
    fn type_name(&self) -> &str;

    /// Render `node`'s `props` into `ctx`. The callback may consult the
    /// full `ctx.tree` for hierarchical lookups and the `ctx.registry`
    /// if it wants to recursively dispatch to other custom components.
    ///
    /// Return [`WgpuRenderOutcome::RecurseChildren`] to ask the walker
    /// to continue into the node's children afterwards;
    /// [`WgpuRenderOutcome::ChildrenHandled`] to skip them.
    fn render(
        &self,
        node: &Node,
        props: &serde_json::Value,
        ctx: &mut WgpuRenderCtx<'_>,
    ) -> Result<WgpuRenderOutcome, MkuiError>;
}

/// Render context handed to a [`WgpuRenderable`]. Carries the buffers
/// the renderer mutates (`scene`, `hits`) plus the immutable read-only
/// state it consults (`tree`, `registry`, `theme`).
///
/// Layout-pass state (`cursor_y`, `content_x`, `viewport_width`) is
/// exposed so extension renderers can position primitives against the
/// walker's current flow. The round-10 §"Concrete Shape" sketch
/// specified the five core fields (`tree`, `registry`, `scene`, `theme`,
/// `hits`); the layout-state fields are documented walker extensions
/// the bridge needs for atoms that participate in vertical flow.
pub struct WgpuRenderCtx<'a> {
    pub tree: &'a AppTree,
    pub registry: &'a WgpuRendererRegistry,
    pub scene: &'a mut Scene,
    pub theme: &'a WgpuTheme,
    pub hits: &'a mut Vec<HitTestEntry>,
    /// Viewport width in logical pixels — extension renderers can use
    /// this to size full-width primitives.
    pub viewport_width: f32,
    /// Next y-coordinate the walker will emit at. Custom renderers may
    /// update this after they emit so the next sibling stacks
    /// underneath.
    pub cursor_y: f32,
    /// Left content edge (after any ancestor's left padding has been
    /// applied).
    pub content_x: f32,
}

/// Registry of custom-component handlers keyed by `type_name`. Built-in
/// node kinds bypass this registry; only `NodeKind::Custom` nodes look
/// up their renderer here.
///
/// `custom` holds `Box<dyn WgpuRenderable>` per the round-10 sketch.
/// `fallback` is also a `Box<dyn WgpuRenderable>` — its `type_name`
/// return value is ignored when invoked as a fallback (the registry
/// dispatches the lookup-missed `type_name` to it directly).
pub struct WgpuRendererRegistry {
    custom: HashMap<String, Box<dyn WgpuRenderable>>,
    fallback: Option<Box<dyn WgpuRenderable>>,
}

impl WgpuRendererRegistry {
    pub fn new() -> Self {
        Self {
            custom: HashMap::new(),
            fallback: None,
        }
    }

    /// Registry pre-populated with the wgpu-side built-in atoms (`Badge`,
    /// `Dot`). The built-in node kinds (`View` / `Text` / `Button`)
    /// render through fixed paths in [`crate::walker`]; this constructor
    /// adds the two scene-primitive atoms the wgpu backend ships so
    /// downstream apps can drop them into the AppTree via
    /// `NodeKind::Custom` without registering a renderer first.
    ///
    /// Acceptance criterion #6: built-ins (Badge, Dot, View, Text,
    /// Button) register at app construction.
    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(builtins::BadgeRenderer);
        registry.register(builtins::DotRenderer);
        registry
    }

    /// Register a custom renderable. Re-registering overwrites the
    /// previous entry for that `type_name`.
    pub fn register<T: WgpuRenderable>(&mut self, component: T) -> &mut Self {
        let type_name = component.type_name().to_string();
        self.custom.insert(type_name, Box::new(component));
        self
    }

    /// Install a deliberate fallback handler for `NodeKind::Custom` nodes
    /// whose `type_name` is not registered. The fallback's own
    /// `type_name` return value is ignored — the registry routes
    /// unknown lookups to it directly.
    pub fn set_fallback<T: WgpuRenderable>(&mut self, fallback: T) -> &mut Self {
        self.fallback = Some(Box::new(fallback));
        self
    }

    pub fn has_renderer_for(&self, type_name: &str) -> bool {
        self.custom.contains_key(type_name)
    }

    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// Dispatch a `NodeKind::Custom` node to its registered renderer.
    /// Returns the renderer's outcome so the walker knows whether to
    /// recurse into children.
    ///
    /// If `type_name` is unregistered and no fallback is installed, the
    /// dispatch logs a `debug_assert!` and returns
    /// [`WgpuRenderOutcome::ChildrenHandled`] so the walker still
    /// terminates the subtree cleanly.
    pub(crate) fn render_custom_node(
        &self,
        type_name: &str,
        node: &Node,
        props: &serde_json::Value,
        ctx: &mut WgpuRenderCtx<'_>,
    ) -> Result<WgpuRenderOutcome, MkuiError> {
        if let Some(handler) = self.custom.get(type_name) {
            return handler.render(node, props, ctx);
        }
        if let Some(fallback) = self.fallback.as_ref() {
            return fallback.render(node, props, ctx);
        }
        debug_assert!(
            false,
            "mkui-wgpu: no renderer registered for custom node type {type_name:?}. \
             Register a WgpuRenderable via Mkui::register or install a fallback."
        );
        Ok(WgpuRenderOutcome::ChildrenHandled)
    }
}

impl Default for WgpuRendererRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl std::fmt::Debug for WgpuRendererRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuRendererRegistry")
            .field("custom_count", &self.custom.len())
            .field("has_fallback", &self.fallback.is_some())
            .finish()
    }
}

/// Built-in custom renderers shipped with [`WgpuRendererRegistry::with_defaults`].
///
/// `BadgeRenderer` and `DotRenderer` give the wgpu backend access to the
/// scene-primitive atoms it has shipped since v0.4.0
/// ([`crate::components::badge`] / [`crate::components::dot`]) through
/// the AppTree `NodeKind::Custom` path. Props are JSON value bags;
/// missing fields fall back to documented defaults so an author can
/// drop a `tree.push_custom(parent, "badge", json!({...}), "")` in
/// without a schema lookup.
pub mod builtins {
    use mkui_core::error::MkuiError;
    use mkui_runtime::Node;

    use super::{WgpuRenderCtx, WgpuRenderOutcome, WgpuRenderable};
    use crate::theme::{BadgeSize, BadgeVariant, DotSize, DotVariant};
    use crate::types::{DotAnimation, Point, Rect, Size};

    /// Built-in `Badge` renderer. Props:
    ///
    /// - `label` (string, required) — chip label text
    /// - `variant` (string, default `"default"`) — one of
    ///   `default / destructive / outline / secondary / ghost / link`
    /// - `size` (string, default `"default"`) — `default / sm`
    /// - `x`, `y` (number, optional) — absolute origin; defaults to the
    ///   walker's current `(content_x, cursor_y)`
    /// - `width`, `height` (number, optional) — defaults to `(80, 22)`
    pub struct BadgeRenderer;

    impl WgpuRenderable for BadgeRenderer {
        fn type_name(&self) -> &str {
            "badge"
        }

        fn render(
            &self,
            _node: &Node,
            props: &serde_json::Value,
            ctx: &mut WgpuRenderCtx<'_>,
        ) -> Result<WgpuRenderOutcome, MkuiError> {
            let label = props
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("badge");
            let variant = badge_variant(props.get("variant").and_then(|v| v.as_str()));
            let size = badge_size(props.get("size").and_then(|v| v.as_str()));
            let width = props
                .get("width")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(80.0);
            let height = props
                .get("height")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(22.0);
            let x = props
                .get("x")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(ctx.content_x);
            let y = props
                .get("y")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(ctx.cursor_y);
            let rect = Rect::new(Point::new(x, y), Size::new(width, height));
            crate::components::badge(ctx.scene, rect, label, variant, size, ctx.theme);
            // Advance the walker cursor underneath the badge so adjacent
            // declarative children stack normally below it.
            ctx.cursor_y = (y + height).max(ctx.cursor_y);
            Ok(WgpuRenderOutcome::ChildrenHandled)
        }
    }

    /// Built-in `Dot` renderer. Props:
    ///
    /// - `variant` (string, default `"ok"`) — `ok / warn / danger / neutral`
    /// - `size` (string, default `"sm"`) — `sm / md`
    /// - `x`, `y` (number, optional) — center; defaults to the walker's
    ///   current cursor location
    /// - `halo` (bool, default `false`)
    /// - `animation` (string, default `"none"`) — `none / pulse / pulse_urgent / spin`
    pub struct DotRenderer;

    impl WgpuRenderable for DotRenderer {
        fn type_name(&self) -> &str {
            "dot"
        }

        fn render(
            &self,
            _node: &Node,
            props: &serde_json::Value,
            ctx: &mut WgpuRenderCtx<'_>,
        ) -> Result<WgpuRenderOutcome, MkuiError> {
            let variant = dot_variant(props.get("variant").and_then(|v| v.as_str()));
            let size = dot_size(props.get("size").and_then(|v| v.as_str()));
            let halo = props.get("halo").and_then(|v| v.as_bool()).unwrap_or(false);
            let animation = dot_animation(props.get("animation").and_then(|v| v.as_str()));
            let x = props
                .get("x")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(ctx.content_x);
            let y = props
                .get("y")
                .and_then(|v| v.as_f64())
                .map(|v| v as f32)
                .unwrap_or(ctx.cursor_y);
            crate::components::dot(
                ctx.scene,
                Point::new(x, y),
                variant,
                size,
                halo,
                animation,
                ctx.theme,
            );
            Ok(WgpuRenderOutcome::ChildrenHandled)
        }
    }

    fn badge_variant(s: Option<&str>) -> BadgeVariant {
        match s.unwrap_or("default") {
            "destructive" => BadgeVariant::Destructive,
            "outline" => BadgeVariant::Outline,
            "secondary" => BadgeVariant::Secondary,
            "ghost" => BadgeVariant::Ghost,
            "link" => BadgeVariant::Link,
            _ => BadgeVariant::Default,
        }
    }

    fn badge_size(s: Option<&str>) -> BadgeSize {
        match s.unwrap_or("default") {
            "sm" => BadgeSize::Sm,
            _ => BadgeSize::Default,
        }
    }

    fn dot_variant(s: Option<&str>) -> DotVariant {
        match s.unwrap_or("ok") {
            "warn" => DotVariant::Warn,
            "danger" => DotVariant::Danger,
            "neutral" => DotVariant::Neutral,
            _ => DotVariant::Ok,
        }
    }

    fn dot_size(s: Option<&str>) -> DotSize {
        match s.unwrap_or("sm") {
            "md" => DotSize::Md,
            _ => DotSize::Sm,
        }
    }

    fn dot_animation(s: Option<&str>) -> DotAnimation {
        match s.unwrap_or("none") {
            "pulse" => DotAnimation::Pulse,
            "pulse_urgent" => DotAnimation::PulseUrgent,
            "spin" => DotAnimation::Spin,
            _ => DotAnimation::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Scene, Size};
    use mkui_runtime::AppTree;

    #[test]
    fn with_defaults_registers_builtin_atoms() {
        let registry = WgpuRendererRegistry::with_defaults();
        assert!(registry.has_renderer_for("badge"));
        assert!(registry.has_renderer_for("dot"));
        assert!(!registry.has_renderer_for("anything-else"));
        assert!(!registry.has_fallback());
    }

    #[test]
    fn new_registry_starts_empty() {
        let registry = WgpuRendererRegistry::new();
        assert!(!registry.has_renderer_for("badge"));
        assert!(!registry.has_renderer_for("dot"));
        assert!(!registry.has_fallback());
    }

    struct TestWidget;
    impl WgpuRenderable for TestWidget {
        fn type_name(&self) -> &str {
            "test_widget"
        }
        fn render(
            &self,
            _node: &Node,
            _props: &serde_json::Value,
            _ctx: &mut WgpuRenderCtx<'_>,
        ) -> Result<WgpuRenderOutcome, MkuiError> {
            Ok(WgpuRenderOutcome::ChildrenHandled)
        }
    }

    #[test]
    fn register_installs_renderer_for_type_name() {
        let mut registry = WgpuRendererRegistry::new();
        registry.register(TestWidget);
        assert!(registry.has_renderer_for("test_widget"));
        assert!(!registry.has_renderer_for("unknown"));
    }

    #[test]
    fn render_custom_node_routes_to_registered_handler() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct CountWidget {
            fired: Rc<Cell<u32>>,
        }
        impl WgpuRenderable for CountWidget {
            fn type_name(&self) -> &str {
                "count_widget"
            }
            fn render(
                &self,
                _node: &Node,
                _props: &serde_json::Value,
                _ctx: &mut WgpuRenderCtx<'_>,
            ) -> Result<WgpuRenderOutcome, MkuiError> {
                self.fired.set(self.fired.get() + 1);
                Ok(WgpuRenderOutcome::ChildrenHandled)
            }
        }

        let fired = Rc::new(Cell::new(0u32));
        let mut registry = WgpuRendererRegistry::new();
        registry.register(CountWidget {
            fired: Rc::clone(&fired),
        });

        // Build a tree with a single custom node so we have a real `Node`
        // to hand the renderer.
        let mut tree = AppTree::new();
        let root = tree.root();
        let custom_id = tree
            .push_custom(root, "count_widget", serde_json::Value::Null, "")
            .unwrap();
        let custom_node = tree.get(custom_id).expect("custom node");

        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(100.0, 100.0));
        let mut hits: Vec<HitTestEntry> = Vec::new();
        let mut ctx = WgpuRenderCtx {
            tree: &tree,
            registry: &registry,
            scene: &mut scene,
            theme: &theme,
            hits: &mut hits,
            viewport_width: 100.0,
            cursor_y: 0.0,
            content_x: 0.0,
        };

        let outcome = registry
            .render_custom_node(
                "count_widget",
                custom_node,
                &serde_json::Value::Null,
                &mut ctx,
            )
            .expect("dispatch ok");
        assert_eq!(outcome, WgpuRenderOutcome::ChildrenHandled);
        assert_eq!(fired.get(), 1);
    }

    #[test]
    fn fallback_is_routed_when_type_name_is_unregistered() {
        use std::cell::Cell;
        use std::rc::Rc;

        struct FallbackWidget {
            fired: Rc<Cell<u32>>,
        }
        impl WgpuRenderable for FallbackWidget {
            fn type_name(&self) -> &str {
                "__fallback__"
            }
            fn render(
                &self,
                _node: &Node,
                _props: &serde_json::Value,
                _ctx: &mut WgpuRenderCtx<'_>,
            ) -> Result<WgpuRenderOutcome, MkuiError> {
                self.fired.set(self.fired.get() + 1);
                Ok(WgpuRenderOutcome::ChildrenHandled)
            }
        }

        let fired = Rc::new(Cell::new(0u32));
        let mut registry = WgpuRendererRegistry::new();
        registry.set_fallback(FallbackWidget {
            fired: Rc::clone(&fired),
        });

        let mut tree = AppTree::new();
        let root = tree.root();
        let custom_id = tree
            .push_custom(root, "unregistered", serde_json::Value::Null, "")
            .unwrap();
        let custom_node = tree.get(custom_id).expect("custom node");

        let theme = WgpuTheme::default();
        let mut scene = Scene::new(Size::new(100.0, 100.0));
        let mut hits: Vec<HitTestEntry> = Vec::new();
        let mut ctx = WgpuRenderCtx {
            tree: &tree,
            registry: &registry,
            scene: &mut scene,
            theme: &theme,
            hits: &mut hits,
            viewport_width: 100.0,
            cursor_y: 0.0,
            content_x: 0.0,
        };

        registry
            .render_custom_node(
                "unregistered",
                custom_node,
                &serde_json::Value::Null,
                &mut ctx,
            )
            .expect("fallback dispatch ok");
        assert_eq!(fired.get(), 1, "fallback must fire for unknown type_name");
    }
}
