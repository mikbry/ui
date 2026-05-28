//! High-level [`Mkui`] entry point for the wgpu backend.
//!
//! Sprint 5 (issue #56): the entry point now mirrors the web + console
//! backends — `Mkui::new()?.child(...).run()` — built on the
//! `mkui_runtime::AppTree` substrate via [`mkui_core::components::Mkui`].
//! The scene is rebuilt from the tree on every render (eager rebuild on
//! dirty signal, per ADR 0006 + Codex round-7 Q3).
//!
//! [`Mkui::with_scene`] is the **retained low-level escape hatch** for
//! renderer tests, custom HUDs, headless tessellation demos, and future
//! direct-GPU work (Slug glyph rendering, mkui-vector2d primitives).
//! Both surfaces — declarative `Mkui::new` and low-level `with_scene` —
//! coexist as documented public API. See ADR 0006 §"`with_scene` as
//! the retained low-level escape hatch".

use mkui_core::components::Component;
use mkui_core::error::MkuiError;

use crate::bridge::{WgpuRenderable, WgpuRendererRegistry};
use crate::{Scene, Size, WgpuApp};

/// High-level mkui-wgpu entry point. Wraps the [`WgpuApp`] event-loop
/// shell so downstream consumers can hand the bridge crate a single
/// type and let it decide whether to drive a winit event loop, a
/// headless tessellation pass, or (later) a web canvas.
///
/// Two construction paths, both supported as documented public API:
///
/// - **Declarative (recommended for app code)** — `Mkui::new()?.child(...).run()`.
///   Builds an `mkui_runtime::AppTree` via `mkui_core`'s lowering
///   registry; the walker projects it into a scene per frame. This is
///   the cross-binding identity the web and console backends also expose.
/// - **Low-level escape hatch** — `Mkui::with_scene(scene).run()`. Hands
///   a pre-built `Scene` to the renderer unchanged. The right surface
///   for renderer tests, custom HUDs, headless tessellation demos, and
///   future direct-GPU work (Slug glyph rendering, mkui-vector2d
///   primitives). See ADR 0006.
pub struct Mkui {
    app: WgpuApp,
    /// Stored alongside `app` so `.child()` can lower into the runtime
    /// tree without unwrapping the `Option`. Moved into the app on
    /// `.run()`.
    core: Option<mkui_core::components::Mkui>,
    registry: Option<WgpuRendererRegistry>,
}

impl Mkui {
    pub fn new() -> Result<Self, MkuiError> {
        Ok(Self {
            app: WgpuApp::new(Scene::new(Size::new(1280.0, 720.0))),
            core: Some(mkui_core::components::Mkui::new()),
            registry: Some(WgpuRendererRegistry::with_defaults()),
        })
    }

    /// Wrap a pre-built [`mkui_core::components::Mkui`] (and its
    /// `AppTree`) so the wgpu backend can run it. Used by examples and
    /// FFI bindings that build the tree directly via `tree.push_custom`
    /// for `NodeKind::Custom` nodes (the `examples/atoms-on-wgpu`
    /// showcase routes through this).
    pub fn from_core(core: mkui_core::components::Mkui) -> Result<Self, MkuiError> {
        Ok(Self {
            app: WgpuApp::new(Scene::new(Size::new(1280.0, 720.0))),
            core: Some(core),
            registry: Some(WgpuRendererRegistry::with_defaults()),
        })
    }

    /// Build a `Mkui` from a pre-built [`Scene`] of raw render primitives.
    ///
    /// This is the **low-level escape hatch** for renderer tests, custom
    /// HUDs, headless tessellation demos, and future direct-GPU
    /// experiments (Slug glyph rendering, mkui-vector2d primitives).
    /// For typical declarative UI, use [`Mkui::new`] and build a runtime
    /// tree via [`Component`]s instead.
    ///
    /// `with_scene` is a **permanent** low-level surface, not a
    /// deprecated path. The declarative `Mkui::new` API is the cross-
    /// binding public identity; `with_scene` continues to exist as a
    /// documented direct-to-renderer entry point. See ADR 0006
    /// §"`with_scene` as the retained low-level escape hatch".
    pub fn with_scene(scene: Scene) -> Self {
        Self {
            app: WgpuApp::new(scene),
            core: None,
            registry: None,
        }
    }

    /// Append a child to the runtime tree. Panics on a class-parse error
    /// to match the v0.4.x infallible builder surface (`mkui-core`'s
    /// `Mkui::child` does the same — `Mkui::try_child` is the fallible
    /// variant when callers need to surface the error).
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        if let Some(core) = self.core.take() {
            self.core = Some(core.child(child));
        }
        self
    }

    /// Register a custom-component renderer with the wgpu registry.
    pub fn register<T: WgpuRenderable>(mut self, component: T) -> Self {
        if let Some(registry) = self.registry.as_mut() {
            registry.register(component);
        }
        self
    }

    /// Install a fallback renderer for unregistered custom node types.
    /// The fallback's own `type_name` return value is ignored — the
    /// registry routes unknown-`type_name` lookups to it directly.
    pub fn fallback<T: WgpuRenderable>(mut self, fallback: T) -> Self {
        if let Some(registry) = self.registry.as_mut() {
            registry.set_fallback(fallback);
        }
        self
    }

    pub fn scene(&self) -> &Scene {
        self.app.scene()
    }

    pub fn set_scene(&mut self, scene: Scene) {
        self.app.set_scene(scene);
    }

    /// Native entry point: builds a winit event loop, hands the wrapped
    /// `WgpuApp` to it, and runs until the window is closed.
    ///
    /// Calling `run` consumes the `Mkui` because winit's event loop
    /// expects exclusive ownership of the application handler.
    ///
    /// When the `HEADLESS=1` environment variable is set, the event loop
    /// exits after a single walk pass — the smoke-test gate (acceptance
    /// criterion #18) routes through this so CI can validate the bridge
    /// without a display server.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run(mut self) -> Result<(), MkuiError> {
        let event_loop = winit::event_loop::EventLoop::new()
            .map_err(|e| MkuiError::initialization(format!("event loop init failed: {e}")))?;

        let mut app = if let (Some(core), Some(registry)) = (self.core.take(), self.registry.take())
        {
            WgpuApp::with_app_tree(core, registry)
        } else {
            self.app
        };

        if std::env::var_os("HEADLESS").is_some() {
            app = app.with_headless(true);
        }

        event_loop
            .run_app(&mut app)
            .map_err(|e| MkuiError::rendering(format!("event loop run failed: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button, Text, View};
    use mkui_runtime::{ButtonVariant, TextVariant};

    #[test]
    fn new_starts_with_a_declarative_core() {
        let mkui = Mkui::new().expect("Mkui::new");
        // Internals are private, but children_len through the bridge is
        // observable through the test build of the rebuild path.
        assert!(mkui.core.is_some());
        assert!(mkui.registry.is_some());
    }

    #[test]
    fn child_appends_to_the_runtime_tree() {
        let mkui = Mkui::new()
            .expect("Mkui::new")
            .child(View::new().child(Text::new("hello").variant(TextVariant::Heading1)));
        let core = mkui.core.as_ref().expect("core present");
        assert_eq!(core.children_len(), 1, "one top-level child under root");
    }

    #[test]
    fn child_chains_multiple_builders() {
        let mkui = Mkui::new()
            .expect("Mkui::new")
            .child(Text::new("title").variant(TextVariant::Heading1))
            .child(
                Button::new("Press")
                    .variant(ButtonVariant::Primary)
                    .on_press(|| {}),
            );
        let core = mkui.core.as_ref().expect("core present");
        assert_eq!(core.children_len(), 2);
    }

    #[test]
    fn with_scene_skips_the_runtime_path() {
        let scene = Scene::new(Size::new(640.0, 480.0));
        let mkui = Mkui::with_scene(scene);
        assert!(
            mkui.core.is_none(),
            "with_scene must not allocate a runtime core; it's the raw-scene escape hatch"
        );
    }
}
