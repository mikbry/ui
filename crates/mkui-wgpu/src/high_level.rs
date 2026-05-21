use crate::{Scene, Size, WgpuApp};
use mkui_core::error::MkuiError;

/// High-level mkui-wgpu entry point. Wraps the [`WgpuApp`] event-loop
/// shell so downstream consumers can hand the bridge crate a single
/// type and let it decide whether to drive a winit event loop, a
/// headless tessellation pass, or (later) a web canvas.
#[derive(Debug)]
pub struct Mkui {
    app: WgpuApp,
}

impl Mkui {
    pub fn new() -> Result<Self, MkuiError> {
        Ok(Self {
            app: WgpuApp::new(Scene::new(Size::new(1280.0, 720.0))),
        })
    }

    pub fn with_scene(scene: Scene) -> Self {
        Self {
            app: WgpuApp::new(scene),
        }
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
    #[cfg(not(target_arch = "wasm32"))]
    pub fn run(self) -> Result<(), MkuiError> {
        let event_loop = winit::event_loop::EventLoop::new()
            .map_err(|e| MkuiError::initialization(format!("event loop init failed: {e}")))?;
        let mut app = self.app;
        event_loop
            .run_app(&mut app)
            .map_err(|e| MkuiError::rendering(format!("event loop run failed: {e}")))
    }
}
