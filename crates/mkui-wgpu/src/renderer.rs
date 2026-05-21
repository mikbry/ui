use std::sync::Arc;

use mkui_text::{BitmapTextSystem, TextSystem};

use crate::{tessellate_scene_with_text, GuiTriangle, Scene};

#[derive(Clone)]
pub struct WgpuRenderer {
    text_system: Arc<dyn TextSystem>,
}

impl Default for WgpuRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl WgpuRenderer {
    pub fn new() -> Self {
        Self {
            text_system: Arc::new(BitmapTextSystem::new()),
        }
    }

    pub fn with_text_system(text_system: Arc<dyn TextSystem>) -> Self {
        Self { text_system }
    }

    pub fn text_system(&self) -> &Arc<dyn TextSystem> {
        &self.text_system
    }

    pub fn tessellate(&self, scene: &Scene) -> Vec<GuiTriangle> {
        tessellate_scene_with_text(scene, &*self.text_system)
    }
}

impl std::fmt::Debug for WgpuRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WgpuRenderer")
            .field("text_system", &"Arc<dyn TextSystem>")
            .finish()
    }
}
