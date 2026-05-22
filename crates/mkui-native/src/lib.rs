#![forbid(unsafe_code)]
//! Native (WGPU) backend for mkui.
//!
//! The full WGPU scene layer is still being imported; what already exists is
//! the contract this backend speaks against. Every node we render comes from
//! the shared component model in [`mkui_core::components`], the same types
//! consumed by `mkui-web` and `mkui-console`. This file documents that
//! boundary and provides a minimal scene walker that future WGPU work can
//! grow into without re-introducing backend-specific component contracts.

use mkui_core::components::{Button, Component, Text, View};
use mkui_core::error::MkuiError;
use mkui_core::theme::Theme;

/// One drawable record produced by [`NativeScene::collect`].
///
/// This intentionally stays plain data — a future WGPU pipeline can turn
/// these records into draw calls without changing the contract crate.
#[derive(Debug, Clone, PartialEq)]
pub enum SceneNode {
    Container {
        class: String,
        depth: usize,
    },
    Text {
        content: String,
        class: String,
        depth: usize,
    },
    Button {
        label: String,
        class: String,
        depth: usize,
    },
    Unknown {
        depth: usize,
    },
}

/// Scene built from a `mkui-core` component tree.
///
/// `NativeScene` is the entry point for the native backend. It holds a
/// flattened list of drawable nodes plus the active theme, and exposes a
/// `collect` entry point that backends compose with their own renderer.
pub struct NativeScene {
    theme: Theme,
    nodes: Vec<SceneNode>,
}

impl NativeScene {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            nodes: Vec::new(),
        }
    }

    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn nodes(&self) -> &[SceneNode] {
        &self.nodes
    }

    /// Walk a component tree rooted at `component` and append the visited
    /// nodes to this scene. Unknown component types are recorded as
    /// `SceneNode::Unknown` so user-defined components surface in renderer
    /// dumps instead of being silently dropped.
    pub fn collect(&mut self, component: &dyn Component) -> Result<(), MkuiError> {
        self.collect_at(component, 0)
    }

    fn collect_at(&mut self, component: &dyn Component, depth: usize) -> Result<(), MkuiError> {
        let any = component as &dyn std::any::Any;

        if let Some(view) = any.downcast_ref::<View>() {
            self.nodes.push(SceneNode::Container {
                class: view.class_name().to_string(),
                depth,
            });
            for child in view.children() {
                self.collect_at(child.as_ref(), depth + 1)?;
            }
        } else if let Some(text) = any.downcast_ref::<Text>() {
            self.nodes.push(SceneNode::Text {
                content: text.content().to_string(),
                class: text.class_name().to_string(),
                depth,
            });
        } else if let Some(button) = any.downcast_ref::<Button>() {
            self.nodes.push(SceneNode::Button {
                label: button.content().to_string(),
                class: button.class_name().to_string(),
                depth,
            });
        } else {
            self.nodes.push(SceneNode::Unknown { depth });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::Mkui;
    use mkui_core::headless::ButtonVariant;

    #[test]
    fn scene_collects_the_same_component_tree_other_backends_consume() {
        let app = Mkui::new().child(
            View::new()
                .class("root")
                .child(Text::new("hello"))
                .child(Button::new("ok").variant(ButtonVariant::Primary)),
        );

        let mut scene = NativeScene::new(Theme::default());
        for child in app.children() {
            scene.collect(child.as_ref()).expect("collect");
        }

        assert_eq!(
            scene.nodes(),
            &[
                SceneNode::Container {
                    class: "root".to_string(),
                    depth: 0
                },
                SceneNode::Text {
                    content: "hello".to_string(),
                    class: String::new(),
                    depth: 1
                },
                SceneNode::Button {
                    label: "ok".to_string(),
                    class: String::new(),
                    depth: 1
                },
            ]
        );
    }

    #[test]
    fn unknown_components_are_surfaced_not_dropped() {
        struct Custom;
        impl Component for Custom {}

        let mut scene = NativeScene::new(Theme::default());
        scene.collect(&Custom).expect("collect");

        assert_eq!(scene.nodes(), &[SceneNode::Unknown { depth: 0 }]);
    }
}
