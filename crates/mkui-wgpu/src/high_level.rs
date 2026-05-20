use crate::{Scene, Size, WgpuApp};
use mkui_core::error::MkuiError;

#[derive(Debug, Clone)]
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
}
