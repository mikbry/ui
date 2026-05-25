#![forbid(unsafe_code)]
//! Native (WGPU) backend for mkui.
//!
//! Sprint 4 reshape: this backend now walks the shared `mkui_runtime::AppTree`
//! produced by `mkui_core::Mkui` rather than the deprecated
//! `Vec<Box<dyn Component>>` shape. The full WGPU declarative bridge is
//! Sprint 5 scope; what already exists is the contract this backend speaks
//! against.

use mkui_core::error::MkuiError;
use mkui_core::theme::Theme;
use mkui_runtime::{AppTree, Node, NodeKind};

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

/// Scene built from a `mkui-runtime` AppTree.
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

    /// Walk an [`AppTree`] starting at its root and append visited nodes
    /// to the scene. Unknown / custom node kinds surface as
    /// `SceneNode::Unknown` so renderer dumps see them.
    pub fn collect(&mut self, tree: &AppTree) -> Result<(), MkuiError> {
        let root = tree
            .get(tree.root())
            .ok_or_else(|| MkuiError::rendering("AppTree root is missing — tree is corrupted"))?;
        // Root itself is synthetic; walk its children at depth 0.
        for child_id in &root.children {
            if let Some(child) = tree.get(*child_id) {
                self.collect_at(tree, child, 0);
            }
        }
        Ok(())
    }

    fn collect_at(&mut self, tree: &AppTree, node: &Node, depth: usize) {
        match &node.kind {
            NodeKind::View(_) => {
                self.nodes.push(SceneNode::Container {
                    class: node.class.raw().to_string(),
                    depth,
                });
                for child_id in &node.children {
                    if let Some(child) = tree.get(*child_id) {
                        self.collect_at(tree, child, depth + 1);
                    }
                }
            }
            NodeKind::Text(t) => {
                self.nodes.push(SceneNode::Text {
                    content: t.content.clone(),
                    class: node.class.raw().to_string(),
                    depth,
                });
            }
            NodeKind::Button(b) => {
                self.nodes.push(SceneNode::Button {
                    label: b.label.clone(),
                    class: node.class.raw().to_string(),
                    depth,
                });
            }
            NodeKind::Root | NodeKind::Custom { .. } => {
                self.nodes.push(SceneNode::Unknown { depth });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button, Mkui, Text, View};
    use mkui_core::headless::ButtonVariant;

    #[test]
    fn scene_collects_the_same_component_tree_other_backends_consume() {
        let app = Mkui::new().child(
            View::new()
                .child(Text::new("hello"))
                .child(Button::new("ok").variant(ButtonVariant::Primary)),
        );

        let mut scene = NativeScene::new(Theme::default());
        scene.collect(app.tree()).expect("collect");

        assert_eq!(
            scene.nodes(),
            &[
                SceneNode::Container {
                    class: String::new(),
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
}
