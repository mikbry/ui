//! Winit `ApplicationHandler` shell that drives the mkui-wgpu renderer.
//!
//! Sprint 5 (issue #56): the app shell now drives the
//! [`mkui_runtime::AppTree`] declarative path end-to-end. On each frame
//! it walks the tree into a `Scene`, hands the scene to the renderer,
//! and routes pointer input through the per-frame hit-test vector. The
//! `Scene`-only constructor (`WgpuApp::new`) is retained as the
//! low-level escape hatch — see [`crate::Mkui::with_scene`] and ADR
//! 0006 §"`with_scene` as the retained low-level escape hatch".

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use mkui_text::{BitmapTextSystem, TextSystem};

use crate::bridge::WgpuRendererRegistry;
use crate::theme::WgpuTheme;
use crate::types::{Scene, Size};

#[cfg(not(target_arch = "wasm32"))]
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::input::{left_button_state, PointerState};
#[cfg(not(target_arch = "wasm32"))]
use crate::render::{RenderOutcome, Renderer};
#[cfg(not(target_arch = "wasm32"))]
use crate::walker::{walk_app_tree, HitTestEntry, WalkOptions};
#[cfg(not(target_arch = "wasm32"))]
use mkui_runtime::RuntimeSignal;
#[cfg(not(target_arch = "wasm32"))]
use winit::event::{ElementState, KeyEvent};
#[cfg(not(target_arch = "wasm32"))]
use winit::keyboard::{KeyCode, PhysicalKey};

/// Application shell. Owns the active scene + text system (for the
/// scene-only escape hatch path) and, when constructed via
/// [`WgpuApp::with_app_tree`], the runtime [`mkui_core::components::Mkui`]
/// + the [`WgpuRendererRegistry`] for custom-component dispatch.
///
/// Native targets additionally hold the `Renderer` state and the
/// per-frame hit-test buffer. Both are `Option` so the same struct shape
/// compiles on wasm consumers that don't pull in winit.
pub struct WgpuApp {
    scene: Scene,
    text_system: Arc<dyn TextSystem>,
    theme: WgpuTheme,
    /// Either a runtime tree (declarative path) or `None` (raw-scene
    /// escape hatch). When `Some`, `Renderer::render` is called against
    /// a tree-derived scene the app rebuilds per frame; when `None`, the
    /// original `scene` field is rendered verbatim.
    core: Option<Rc<RefCell<mkui_core::components::Mkui>>>,
    registry: Option<WgpuRendererRegistry>,
    #[cfg(not(target_arch = "wasm32"))]
    state: Option<WgpuAppState>,
    #[cfg(not(target_arch = "wasm32"))]
    window_title: String,
    #[cfg(not(target_arch = "wasm32"))]
    headless: bool,
    /// Most recent per-frame hit-test list. Replaced wholesale on each
    /// rebuild via [`walk_app_tree`]'s [`WalkOutput`]; the input router
    /// reads this slice (no `&mut` is ever handed to a renderer or
    /// action closure).
    #[cfg(not(target_arch = "wasm32"))]
    hit_tests: Vec<HitTestEntry>,
    #[cfg(not(target_arch = "wasm32"))]
    pointer: PointerState,
}

#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
struct WgpuAppState {
    window: Arc<Window>,
    renderer: Renderer,
}

impl WgpuApp {
    pub fn new(scene: Scene) -> Self {
        Self {
            scene,
            text_system: Arc::new(BitmapTextSystem::new()),
            theme: WgpuTheme::default(),
            core: None,
            registry: None,
            #[cfg(not(target_arch = "wasm32"))]
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            window_title: "mkui".to_string(),
            #[cfg(not(target_arch = "wasm32"))]
            headless: false,
            #[cfg(not(target_arch = "wasm32"))]
            hit_tests: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            pointer: PointerState::new(),
        }
    }

    /// Build the app with an explicit text system. Renderer-side hosts that
    /// need to swap implementations (bitmap → Slug) construct via this entry
    /// rather than `new`.
    pub fn with_text_system(scene: Scene, text_system: Arc<dyn TextSystem>) -> Self {
        let mut app = Self::new(scene);
        app.text_system = text_system;
        app
    }

    /// Build a declarative-path app from a runtime [`mkui_core::components::Mkui`]
    /// plus a custom-component registry. The event loop rebuilds the scene
    /// from the tree per frame (eager rebuild — Sprint 4 substrate
    /// contract, ADR 0006).
    pub fn with_app_tree(
        core: mkui_core::components::Mkui,
        registry: WgpuRendererRegistry,
    ) -> Self {
        let mut app = Self::new(Scene::new(Size::new(1280.0, 720.0)));
        app.core = Some(Rc::new(RefCell::new(core)));
        app.registry = Some(registry);
        app
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
    }

    pub fn text_system(&self) -> &Arc<dyn TextSystem> {
        &self.text_system
    }

    pub fn set_text_system(&mut self, text_system: Arc<dyn TextSystem>) {
        self.text_system = text_system;
    }

    pub fn theme(&self) -> &WgpuTheme {
        &self.theme
    }

    pub fn set_theme(&mut self, theme: WgpuTheme) {
        self.theme = theme;
    }

    /// Borrow the underlying core Mkui (and thus the runtime AppTree),
    /// if the app was constructed via [`WgpuApp::with_app_tree`]. Tests
    /// and FFI shims call this to inspect or mutate the tree without
    /// going through the event loop.
    pub fn core(&self) -> Option<&Rc<RefCell<mkui_core::components::Mkui>>> {
        self.core.as_ref()
    }

    pub fn registry(&self) -> Option<&WgpuRendererRegistry> {
        self.registry.as_ref()
    }

    /// Override the window title used on the next `resumed` event.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_window_title(mut self, title: impl Into<String>) -> Self {
        self.window_title = title.into();
        self
    }

    /// Run the event loop in headless mode — build the scene once, then
    /// exit without opening a window. The `HEADLESS=1` smoke-test gate
    /// (acceptance criterion #18) routes through this path so CI can
    /// validate the bridge end-to-end without a display server.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_headless(mut self, headless: bool) -> Self {
        self.headless = headless;
        self
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn is_headless(&self) -> bool {
        self.headless
    }

    /// Pull the runtime tree's `is_dirty` state, rebuild the scene from
    /// it via the walker, and clear the dirty flag. No-op when the app
    /// is in scene-only mode (the legacy `with_scene` path).
    ///
    /// Exposed so headless tests can drive the walker once without
    /// running the full event loop.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn rebuild_scene_from_tree(&mut self) {
        let Some(core) = self.core.as_ref() else {
            return;
        };
        let Some(registry) = self.registry.as_ref() else {
            return;
        };
        let options = WalkOptions {
            viewport: self.scene.viewport,
            theme: self.theme,
        };
        let output = {
            let core_ref = core.borrow();
            // v0.6.0 never returns Err from the walker; surface a render
            // error if a future extension renderer changes that.
            match walk_app_tree(core_ref.tree(), registry, &options) {
                Ok(out) => out,
                Err(err) => {
                    eprintln!("mkui-wgpu: walk_app_tree error: {err}");
                    return;
                }
            }
        };
        // Clear dirty after the walk completes so a future action firing
        // mid-walk does not get its dirty bit clobbered.
        core.borrow_mut().tree_mut().clear_dirty();
        self.scene = output.scene;
        self.hit_tests = output.hit_tests;
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn window_attributes(&self) -> WindowAttributes {
        let viewport = self.scene.viewport;
        Window::default_attributes()
            .with_title(self.window_title.clone())
            .with_inner_size(winit::dpi::LogicalSize::new(
                viewport.width as f64,
                viewport.height as f64,
            ))
    }

    /// Fire the action attached to `click_hit` through the tree's
    /// `ActionRegistry`. Returns true if a redraw signal was raised.
    /// Caller must have already resolved `click_hit` via the press-to-
    /// arm state machine (see [`crate::input::PointerState::on_release`]).
    #[cfg(not(target_arch = "wasm32"))]
    fn fire_action(&mut self, click_hit: crate::input::ClickHit) -> bool {
        let Some(action_id) = click_hit.action else {
            return false;
        };
        let Some(core) = self.core.as_ref() else {
            return false;
        };
        // Borrow scoping: `fire` only needs `&self`, so we drop the
        // immutable borrow before re-borrowing mutably to propagate the
        // dirty bit. Holding both at once is the round-7 anti-pattern.
        let mut ctx = core.borrow().tree().actions().fire(action_id);
        if ctx.is_dirty() {
            core.borrow_mut().tree_mut().mark_dirty();
        }
        ctx.drain_emitted()
            .iter()
            .any(|s| matches!(s, RuntimeSignal::RequestRedraw))
    }
}

impl std::fmt::Debug for WgpuApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("WgpuApp");
        builder
            .field("scene", &self.scene)
            .field("text_system", &"Arc<dyn TextSystem>")
            .field("has_core", &self.core.is_some())
            .field("has_registry", &self.registry.is_some());
        #[cfg(not(target_arch = "wasm32"))]
        builder
            .field("state", &self.state)
            .field("window_title", &self.window_title)
            .field("headless", &self.headless)
            .field("hit_tests", &self.hit_tests.len());
        builder.finish()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ApplicationHandler for WgpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        // Headless mode: walk once for the smoke gate, then exit without
        // ever creating a window or wgpu surface. Lets CI validate the
        // walker + registry wiring end-to-end on machines without a
        // display server (acceptance criterion #18).
        if self.headless {
            self.rebuild_scene_from_tree();
            event_loop.exit();
            return;
        }

        // `resumed` can fire more than once on Android-style lifecycles;
        // bail out if we already have a window so we don't drop the
        // current renderer.
        if self.state.is_some() {
            return;
        }
        let window = match event_loop.create_window(self.window_attributes()) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                eprintln!("mkui-wgpu: failed to create window: {error}");
                event_loop.exit();
                return;
            }
        };
        match pollster::block_on(Renderer::new(window.clone())) {
            Ok(renderer) => {
                self.state = Some(WgpuAppState { window, renderer });
                if self.core.is_some() {
                    self.rebuild_scene_from_tree();
                }
                if let Some(state) = self.state.as_ref() {
                    state.window.request_redraw();
                }
            }
            Err(error) => {
                eprintln!("mkui-wgpu: failed to initialize renderer: {error}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let window_matches = self
            .state
            .as_ref()
            .is_some_and(|state| state.window.id() == window_id);
        if !window_matches {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let (new_viewport, window) = {
                    let Some(state) = self.state.as_mut() else {
                        return;
                    };
                    state.renderer.resize(size.width, size.height);
                    let scale = state.window.scale_factor() as f32;
                    (
                        Size::new(
                            size.width as f32 / scale.max(f32::EPSILON),
                            size.height as f32 / scale.max(f32::EPSILON),
                        ),
                        Arc::clone(&state.window),
                    )
                };
                self.scene = Scene::new(new_viewport);
                if let Some(core) = self.core.as_ref() {
                    core.borrow_mut().tree_mut().mark_dirty();
                }
                self.rebuild_scene_from_tree();
                window.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer.update_cursor(position);
            }
            WindowEvent::CursorLeft { .. } => {
                // Mouse left the window: cancel any armed press without
                // firing (Codex round-10 Q4 — drag-cancel affordance).
                self.pointer.cancel();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(KeyCode::Escape),
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                // User-driven cancel: Escape clears the armed slot
                // without firing.
                self.pointer.cancel();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(state) = left_button_state(button, state) else {
                    return;
                };
                let scale = self
                    .state
                    .as_ref()
                    .map(|s| s.window.scale_factor())
                    .unwrap_or(1.0);
                let Some(cursor) = self.pointer.cursor_logical(scale) else {
                    return;
                };
                match state {
                    ElementState::Pressed => {
                        // Press-to-arm: record the topmost-hit node;
                        // never fire on press (Codex round-10 Q4).
                        self.pointer.on_press(&self.hit_tests, cursor);
                    }
                    ElementState::Released => {
                        let Some(click) = self.pointer.on_release(&self.hit_tests, cursor) else {
                            // Release on different node / empty space /
                            // after cancel — armed slot already cleared
                            // inside `on_release`. Nothing to fire.
                            return;
                        };
                        if self.fire_action(click) {
                            // Action requested a redraw — rebuild from
                            // the (now-dirty) tree and ask winit for a
                            // redraw. The `request_redraw` call is made
                            // HERE in the event loop, NOT from inside
                            // the action closure (Sprint 4 anti-pattern
                            // guard).
                            self.rebuild_scene_from_tree();
                            if let Some(state) = self.state.as_ref() {
                                state.window.request_redraw();
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // If the tree dirtied between events, rebuild before painting.
                let needs_rebuild = self
                    .core
                    .as_ref()
                    .map(|c| c.borrow().tree().is_dirty())
                    .unwrap_or(false);
                if needs_rebuild {
                    self.rebuild_scene_from_tree();
                }
                if let Some(state) = self.state.as_mut() {
                    let outcome = state.renderer.render(&self.scene, &*self.text_system);
                    match outcome {
                        Ok(RenderOutcome::Drawn) | Ok(RenderOutcome::Skipped) => {}
                        Ok(RenderOutcome::NeedsReconfigure) => {
                            let (w, h) = state.renderer.size();
                            state.renderer.resize(w, h);
                            state.window.request_redraw();
                        }
                        Err(error) => {
                            eprintln!("mkui-wgpu: render error: {error}");
                            event_loop.exit();
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Size;

    #[test]
    fn new_holds_initial_scene() {
        let scene = Scene::new(Size::new(640.0, 480.0));
        let app = WgpuApp::new(scene);
        assert_eq!(app.scene().viewport, Size::new(640.0, 480.0));
        assert!(app.core().is_none(), "scene-only constructor has no core");
    }

    #[test]
    fn set_scene_replaces_held_scene() {
        let mut app = WgpuApp::new(Scene::new(Size::new(100.0, 100.0)));
        app.set_scene(Scene::new(Size::new(200.0, 200.0)));
        assert_eq!(app.scene().viewport, Size::new(200.0, 200.0));
    }

    #[test]
    fn with_app_tree_attaches_core_and_registry() {
        let core = mkui_core::components::Mkui::new();
        let registry = WgpuRendererRegistry::with_defaults();
        let app = WgpuApp::with_app_tree(core, registry);
        assert!(app.core().is_some());
        assert!(app.registry().is_some());
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rebuild_scene_from_tree_runs_the_walker() {
        use mkui_core::components::{Mkui as CoreMkui, Text};
        use mkui_runtime::TextVariant;

        let core = CoreMkui::new().child(Text::new("hi").variant(TextVariant::Heading1));
        let registry = WgpuRendererRegistry::with_defaults();
        let mut app = WgpuApp::with_app_tree(core, registry);
        app.rebuild_scene_from_tree();
        let has_text = app
            .scene()
            .primitives
            .iter()
            .any(|p| matches!(p, crate::types::Primitive::Text(_)));
        assert!(has_text, "walker should emit at least one text primitive");
    }
}
