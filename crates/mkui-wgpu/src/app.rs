use crate::Scene;

#[derive(Debug, Clone)]
pub struct WgpuApp {
    scene: Scene,
}

impl WgpuApp {
    pub fn new(scene: Scene) -> Self {
        Self { scene }
    }

    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    pub fn set_scene(&mut self, scene: Scene) {
        self.scene = scene;
    }
}
