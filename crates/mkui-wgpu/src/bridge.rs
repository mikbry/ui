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
//! ## Adding a custom component
//!
//! ```ignore
//! use mkui_wgpu::bridge::{CustomWgpuRenderable, WgpuRendererRegistry, WalkContext};
//! use mkui_runtime::AppTree;
//!
//! struct Card;
//! impl CustomWgpuRenderable for Card {
//!     fn type_name(&self) -> &str { "card" }
//!     fn render_custom(
//!         &self,
//!         props: &serde_json::Value,
//!         ctx: &mut WalkContext<'_>,
//!         _registry: &WgpuRendererRegistry,
//!         _tree: &AppTree,
//!     ) {
//!         // Emit primitives into ctx.scene at ctx.cursor; push hit entries
//!         // into ctx.hit_entries for any interactive sub-rects.
//!     }
//! }
//! ```

use std::collections::HashMap;

use mkui_runtime::AppTree;

use crate::walker::WalkContext;

/// Backend-specific render trait for [`mkui_runtime::NodeKind::Custom`] node
/// types on the wgpu backend.
///
/// Built-in `View` / `Text` / `Button` rendering is hard-wired inside
/// [`crate::walker`]; only extension types go through this trait. The
/// callback receives a [`WalkContext`] so the custom renderer can push
/// primitives + hit entries through the same allocator the built-ins use
/// (per ADR 0006 — no per-frame Vec allocations from extension code).
pub trait CustomWgpuRenderable: 'static {
    /// `type_name` the registry keys this renderer by. Must match the
    /// `type_name` field stored on the [`mkui_runtime::NodeKind::Custom`]
    /// nodes the binding produces.
    fn type_name(&self) -> &str;

    /// Render `props` into `ctx`. The callback may consult the full
    /// `tree` for hierarchical lookups and the `registry` if it wants
    /// to recursively dispatch to other custom components.
    fn render_custom(
        &self,
        props: &serde_json::Value,
        ctx: &mut WalkContext<'_>,
        registry: &WgpuRendererRegistry,
        tree: &AppTree,
    );
}

type CustomRenderFn =
    Box<dyn Fn(&serde_json::Value, &mut WalkContext<'_>, &WgpuRendererRegistry, &AppTree)>;

/// Registry of custom-component handlers keyed by `type_name`.
///
/// Built-in node kinds bypass this registry; only `NodeKind::Custom` nodes
/// look up their renderer here. Unknown types either hit the configured
/// fallback or are silently skipped (debug-asserted so tests catch the
/// missing registration).
pub struct WgpuRendererRegistry {
    custom: HashMap<String, CustomRenderFn>,
    fallback: Option<CustomRenderFn>,
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
        registry.register(crate::bridge::builtins::BadgeRenderer);
        registry.register(crate::bridge::builtins::DotRenderer);
        registry
    }

    /// Register a custom renderable.
    pub fn register<T: CustomWgpuRenderable>(&mut self, component: T) -> &mut Self {
        let type_name = component.type_name().to_string();
        self.custom.insert(
            type_name,
            Box::new(move |props, ctx, registry, tree| {
                component.render_custom(props, ctx, registry, tree);
            }),
        );
        self
    }

    /// Install a deliberate fallback handler for `NodeKind::Custom` nodes
    /// whose `type_name` is not registered.
    pub fn set_fallback<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&serde_json::Value, &mut WalkContext<'_>, &WgpuRendererRegistry, &AppTree) + 'static,
    {
        self.fallback = Some(Box::new(f));
        self
    }

    pub fn has_renderer_for(&self, type_name: &str) -> bool {
        self.custom.contains_key(type_name)
    }

    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    pub(crate) fn render_custom_node(
        &self,
        type_name: &str,
        props: &serde_json::Value,
        ctx: &mut WalkContext<'_>,
        tree: &AppTree,
    ) {
        if let Some(handler) = self.custom.get(type_name) {
            handler(props, ctx, self, tree);
            return;
        }
        if let Some(fallback) = &self.fallback {
            fallback(props, ctx, self, tree);
            return;
        }
        debug_assert!(
            false,
            "mkui-wgpu: no renderer registered for custom node type {type_name:?}. \
             Register a CustomWgpuRenderable via Mkui::register or install a fallback."
        );
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
    use mkui_runtime::AppTree;

    use super::{CustomWgpuRenderable, WgpuRendererRegistry};
    use crate::theme::{BadgeSize, BadgeVariant, DotSize, DotVariant};
    use crate::types::{DotAnimation, Point, Rect, Size};
    use crate::walker::WalkContext;

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

    impl CustomWgpuRenderable for BadgeRenderer {
        fn type_name(&self) -> &str {
            "badge"
        }

        fn render_custom(
            &self,
            props: &serde_json::Value,
            ctx: &mut WalkContext<'_>,
            _registry: &WgpuRendererRegistry,
            _tree: &AppTree,
        ) {
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

    impl CustomWgpuRenderable for DotRenderer {
        fn type_name(&self) -> &str {
            "dot"
        }

        fn render_custom(
            &self,
            props: &serde_json::Value,
            ctx: &mut WalkContext<'_>,
            _registry: &WgpuRendererRegistry,
            _tree: &AppTree,
        ) {
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
    use crate::theme::HudTheme;
    use crate::types::{Scene, Size};
    use crate::walker::HitTestEntry;

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

    #[test]
    fn fallback_hook_is_installed_when_set() {
        let mut registry = WgpuRendererRegistry::new();
        assert!(!registry.has_fallback());
        registry.set_fallback(|_props, _ctx, _registry, _tree| {});
        assert!(registry.has_fallback());
    }

    struct TestWidget;
    impl CustomWgpuRenderable for TestWidget {
        fn type_name(&self) -> &str {
            "test_widget"
        }
        fn render_custom(
            &self,
            _props: &serde_json::Value,
            _ctx: &mut WalkContext<'_>,
            _registry: &WgpuRendererRegistry,
            _tree: &AppTree,
        ) {
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
        impl CustomWgpuRenderable for CountWidget {
            fn type_name(&self) -> &str {
                "count_widget"
            }
            fn render_custom(
                &self,
                _props: &serde_json::Value,
                _ctx: &mut WalkContext<'_>,
                _registry: &WgpuRendererRegistry,
                _tree: &AppTree,
            ) {
                self.fired.set(self.fired.get() + 1);
            }
        }

        let fired = Rc::new(Cell::new(0u32));
        let mut registry = WgpuRendererRegistry::new();
        registry.register(CountWidget {
            fired: Rc::clone(&fired),
        });

        let tree = AppTree::new();
        let theme = HudTheme::default();
        let mut scene = Scene::new(Size::new(100.0, 100.0));
        let mut hits: Vec<HitTestEntry> = Vec::new();
        let mut ctx = WalkContext::new(&mut scene, &theme, &mut hits, 100.0);

        registry.render_custom_node("count_widget", &serde_json::Value::Null, &mut ctx, &tree);
        assert_eq!(fired.get(), 1);
    }
}
