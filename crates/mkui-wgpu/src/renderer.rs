use crate::{tessellate_scene, GuiTriangle, Scene};

#[derive(Debug, Default, Clone, Copy)]
pub struct WgpuRenderer;

impl WgpuRenderer {
    pub fn new() -> Self {
        Self
    }

    pub fn tessellate(&self, scene: &Scene) -> Vec<GuiTriangle> {
        tessellate_scene(scene)
    }
}
