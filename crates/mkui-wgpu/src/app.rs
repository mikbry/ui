//! Winit `ApplicationHandler` shell that drives the mkui-wgpu renderer.
//!
//! `WgpuApp` owns the scene + the active text system and (after the first
//! `resumed` event) the window + the async-initialized
//! [`render::Renderer`]. Downstream native apps don't usually instantiate
//! this directly — they call [`crate::Mkui::run`], which wraps the
//! event-loop boilerplate.
//!
//! [`render::Renderer`]: crate::render::Renderer

use std::sync::Arc;

use mkui_text::{BitmapTextSystem, TextSystem};

use crate::Scene;

#[cfg(not(target_arch = "wasm32"))]
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowAttributes, WindowId},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::render::{RenderOutcome, Renderer};

/// Application shell. Wraps a mutable [`Scene`] + the
/// [`Arc<dyn TextSystem>`] the tessellator delegates glyph layout to,
/// plus — on native targets — the [`Renderer`] state created on
/// `resumed`.
///
/// Constructed via [`WgpuApp::new`] (defaults to [`BitmapTextSystem`]) or
/// [`WgpuApp::with_text_system`]. The [`ApplicationHandler`] impl is
/// only present on native targets so the same scene-only constructor
/// keeps compiling on wasm consumers that don't pull in winit.
pub struct WgpuApp {
    scene: Scene,
    text_system: Arc<dyn TextSystem>,
    #[cfg(not(target_arch = "wasm32"))]
    state: Option<WgpuAppState>,
    #[cfg(not(target_arch = "wasm32"))]
    window_title: String,
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
            #[cfg(not(target_arch = "wasm32"))]
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            window_title: "mkui".to_string(),
        }
    }

    /// Build the app with an explicit text system. Renderer-side hosts that
    /// need to swap implementations (bitmap → Slug) construct via this entry
    /// rather than `new`.
    pub fn with_text_system(scene: Scene, text_system: Arc<dyn TextSystem>) -> Self {
        Self {
            scene,
            text_system,
            #[cfg(not(target_arch = "wasm32"))]
            state: None,
            #[cfg(not(target_arch = "wasm32"))]
            window_title: "mkui".to_string(),
        }
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

    /// Override the window title used on the next `resumed` event.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn with_window_title(mut self, title: impl Into<String>) -> Self {
        self.window_title = title.into();
        self
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
}

impl std::fmt::Debug for WgpuApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut builder = f.debug_struct("WgpuApp");
        builder
            .field("scene", &self.scene)
            .field("text_system", &"Arc<dyn TextSystem>");
        #[cfg(not(target_arch = "wasm32"))]
        builder
            .field("state", &self.state)
            .field("window_title", &self.window_title);
        builder.finish()
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl ApplicationHandler for WgpuApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
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
                if let Some(state) = self.state.as_mut() {
                    state.renderer.resize(size.width, size.height);
                    state.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
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
    }

    #[test]
    fn set_scene_replaces_held_scene() {
        let mut app = WgpuApp::new(Scene::new(Size::new(100.0, 100.0)));
        app.set_scene(Scene::new(Size::new(200.0, 200.0)));
        assert_eq!(app.scene().viewport, Size::new(200.0, 200.0));
    }
}
