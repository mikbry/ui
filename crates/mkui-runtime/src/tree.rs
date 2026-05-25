//! `AppTree` and its slot-arena nodes.
//!
//! The tree is a single shared substrate every binding builds into:
//!
//! - **Rust** — the ergonomic `mkui_core::components` builder lowers into
//!   the tree behind the scenes via [`AppTree::push_view`] /
//!   [`AppTree::push_text`] / [`AppTree::push_button`].
//! - **C** — `mkui_c` exposes opaque handles for the app and node-id; FFI
//!   functions like `mkui_app_view_child(parent, "class")` mutate the tree
//!   in place.
//! - **Python** — `mkui_py` mirrors the C shape with `app.view_child(parent,
//!   class)` etc. on the same `AppTree`.
//!
//! Construction across all three frontends produces byte-identical JSON
//! snapshots (see [`crate::snapshot`]); that's the parity gate.

use serde::{Deserialize, Serialize};

use crate::actions::{ActionId, ActionRegistry};
use crate::props::{ButtonVariant, TextVariant};
use crate::style::{ClassParseError, ResolvedStyle, StyleClass};

/// Opaque handle to a node in the [`AppTree`].
///
/// The `(index, generation)` pair guards against use-after-free: removing a
/// node bumps the slot's generation, so a later lookup with the stale id
/// returns `None` instead of accidentally touching a recycled node (Codex
/// round-7 anti-pattern guard).
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct NodeId {
    index: u32,
    generation: u32,
}

impl NodeId {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Build a `NodeId` from raw parts — used by FFI shims that need to
    /// reconstruct the handle from two `u32`s passed across the boundary.
    pub fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

/// One node in the tree. `kind` carries variant-specific data; `class` +
/// `resolved` carry the layout/style projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
    pub kind: NodeKind,
    pub class: StyleClass,
    pub resolved: ResolvedStyle,
}

/// The variant payload stored on a [`Node`].
///
/// `Root` is a synthetic top-level container the tree always carries —
/// keeping it explicit means every real node has a parent and the JSON
/// snapshot has a single deterministic entry point.
///
/// `Custom` is the registry-style extension slot: the binding stores a
/// type name + an opaque `serde_json::Value` of properties. The renderer
/// dispatches by `type_name`. The Sprint 4 `TestWidget` extension proof
/// uses this slot (see the parity test suite).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NodeKind {
    Root,
    View(ViewProps),
    Text(TextProps),
    Button(ButtonProps),
    /// Extension slot. `type_name` identifies the component (`"test_widget"`,
    /// `"separator"`, …) and `props` is a renderer-readable JSON payload.
    Custom {
        type_name: String,
        props: serde_json::Value,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewProps {}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextProps {
    pub content: String,
    pub variant: TextVariant,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ButtonProps {
    pub label: String,
    pub variant: ButtonVariant,
    pub on_press: Option<ActionId>,
}

/// Arena-backed scene graph. Every binding builds into this same shape.
///
/// `nodes` is indexed by `NodeId.index`. A node lives in `Some(_)` until it
/// is removed; the slot then becomes `None` and `free` records the index so
/// the next allocation can reuse it (bumping `generation`).
///
/// `actions` lives alongside the nodes so `ButtonProps::on_press` can hold
/// an [`ActionId`] that resolves through [`AppTree::actions`] /
/// [`AppTree::actions_mut`].
pub struct AppTree {
    nodes: Vec<Slot>,
    free: Vec<u32>,
    root: NodeId,
    dirty: bool,
    actions: ActionRegistry,
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    node: Option<Node>,
}

impl AppTree {
    /// Create an empty tree with a synthetic [`NodeKind::Root`].
    pub fn new() -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            free: Vec::new(),
            root: NodeId::from_raw(0, 0),
            dirty: false,
            actions: ActionRegistry::new(),
        };
        // The root is allocated like any other node so its NodeId follows the
        // same `(index, generation)` shape FFI handles use.
        let root = tree.alloc_node(None, NodeKind::Root, StyleClass::default());
        tree.root = root;
        tree
    }

    pub fn root(&self) -> NodeId {
        self.root
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark the tree dirty so the next render frame redraws. Call sites:
    /// any structural mutation (push_*, set_*) and any fired action that
    /// itself sets `RuntimeCtx::mark_dirty`.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    pub fn actions(&self) -> &ActionRegistry {
        &self.actions
    }

    pub fn actions_mut(&mut self) -> &mut ActionRegistry {
        &mut self.actions
    }

    /// Look up a node by id. Returns `None` if the slot is empty or the id
    /// is stale (different generation).
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        let slot = self.nodes.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()
    }

    /// Mutable lookup. Same staleness guard as [`AppTree::get`].
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        let slot = self.nodes.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_mut()
    }

    /// Iterate every live node in insertion order. Order is load-bearing
    /// for parity snapshots.
    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.iter().filter_map(|s| s.node.as_ref())
    }

    /// Number of live nodes (excludes the root from caller-visible counts
    /// nowhere — root is always counted; bindings looking for "nodes added
    /// by user code" should subtract 1).
    pub fn len(&self) -> usize {
        self.nodes.iter().filter(|s| s.node.is_some()).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // ---- builder-friendly helpers used by mkui-core's Rust lowering ----

    /// Append a `View` under `parent`. Parses `class` eagerly so the parse
    /// error surfaces at construction time, not first render.
    pub fn push_view(
        &mut self,
        parent: NodeId,
        class: impl Into<String>,
    ) -> Result<NodeId, ClassParseError> {
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(self.push_view_unchecked(parent, style))
    }

    /// `push_view` variant that skips the class parse. Used by FFI shims
    /// that already validated the class on the binding side, and by tests
    /// asserting structural shape independent of parse rules.
    pub fn push_view_unchecked(&mut self, parent: NodeId, class: StyleClass) -> NodeId {
        let kind = NodeKind::View(ViewProps::default());
        self.alloc_and_attach(parent, kind, class)
    }

    /// Append a `Text` under `parent`.
    pub fn push_text(
        &mut self,
        parent: NodeId,
        content: impl Into<String>,
        variant: TextVariant,
        class: impl Into<String>,
    ) -> Result<NodeId, ClassParseError> {
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(self.push_text_unchecked(parent, content, variant, style))
    }

    pub fn push_text_unchecked(
        &mut self,
        parent: NodeId,
        content: impl Into<String>,
        variant: TextVariant,
        class: StyleClass,
    ) -> NodeId {
        let kind = NodeKind::Text(TextProps {
            content: content.into(),
            variant,
        });
        self.alloc_and_attach(parent, kind, class)
    }

    /// Append a `Button` under `parent`.
    pub fn push_button(
        &mut self,
        parent: NodeId,
        label: impl Into<String>,
        variant: ButtonVariant,
        class: impl Into<String>,
        on_press: Option<ActionId>,
    ) -> Result<NodeId, ClassParseError> {
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(self.push_button_unchecked(parent, label, variant, style, on_press))
    }

    pub fn push_button_unchecked(
        &mut self,
        parent: NodeId,
        label: impl Into<String>,
        variant: ButtonVariant,
        class: StyleClass,
        on_press: Option<ActionId>,
    ) -> NodeId {
        let kind = NodeKind::Button(ButtonProps {
            label: label.into(),
            variant,
            on_press,
        });
        self.alloc_and_attach(parent, kind, class)
    }

    /// Append a custom (registry-style) node under `parent`. Used by the
    /// `TestWidget` extension proof and any downstream component the
    /// binding's lowering registry exposes.
    pub fn push_custom(
        &mut self,
        parent: NodeId,
        type_name: impl Into<String>,
        props: serde_json::Value,
        class: impl Into<String>,
    ) -> Result<NodeId, ClassParseError> {
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        let kind = NodeKind::Custom {
            type_name: type_name.into(),
            props,
        };
        Ok(self.alloc_and_attach(parent, kind, style))
    }

    fn alloc_and_attach(&mut self, parent: NodeId, kind: NodeKind, class: StyleClass) -> NodeId {
        assert!(
            self.get(parent).is_some(),
            "AppTree::push_*: parent NodeId {parent:?} is stale or never existed"
        );
        let id = self.alloc_node(Some(parent), kind, class);
        // Attach to parent in a second step so the borrow checker is happy.
        if let Some(parent_node) = self.get_mut(parent) {
            parent_node.children.push(id);
        }
        self.mark_dirty();
        id
    }

    fn alloc_node(&mut self, parent: Option<NodeId>, kind: NodeKind, class: StyleClass) -> NodeId {
        let resolved = class.parse().unwrap_or_default();
        if let Some(idx) = self.free.pop() {
            let slot = &mut self.nodes[idx as usize];
            slot.generation = slot.generation.wrapping_add(1);
            let id = NodeId::from_raw(idx, slot.generation);
            slot.node = Some(Node {
                id,
                parent,
                children: Vec::new(),
                kind,
                class,
                resolved,
            });
            id
        } else {
            let index = self.nodes.len() as u32;
            let id = NodeId::from_raw(index, 0);
            self.nodes.push(Slot {
                generation: 0,
                node: Some(Node {
                    id,
                    parent,
                    children: Vec::new(),
                    kind,
                    class,
                    resolved,
                }),
            });
            id
        }
    }
}

impl Default for AppTree {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for AppTree {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppTree")
            .field("len", &self.len())
            .field("root", &self.root)
            .field("dirty", &self.dirty)
            .field("actions", &self.actions)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_tree_has_a_single_root_node() {
        let tree = AppTree::new();
        assert_eq!(tree.len(), 1);
        let root = tree.get(tree.root()).expect("root must exist");
        assert!(matches!(root.kind, NodeKind::Root));
        assert!(root.parent.is_none());
        assert!(root.children.is_empty());
    }

    #[test]
    fn push_view_attaches_to_parent() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let view = tree.push_view(root, "flex").unwrap();
        let parent = tree.get(root).unwrap();
        assert_eq!(parent.children, vec![view]);
        let child = tree.get(view).unwrap();
        assert_eq!(child.parent, Some(root));
        assert!(matches!(child.kind, NodeKind::View(_)));
        assert!(child.resolved.flex);
    }

    #[test]
    fn push_text_and_push_button_carry_variants() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let text = tree
            .push_text(root, "hi", TextVariant::Heading1, "")
            .unwrap();
        let action = tree.actions_mut().register_local(|ctx| ctx.mark_dirty());
        let button = tree
            .push_button(root, "ok", ButtonVariant::Primary, "", Some(action))
            .unwrap();

        let NodeKind::Text(t) = &tree.get(text).unwrap().kind else {
            panic!("expected Text")
        };
        assert_eq!(t.content, "hi");
        assert_eq!(t.variant, TextVariant::Heading1);

        let NodeKind::Button(b) = &tree.get(button).unwrap().kind else {
            panic!("expected Button")
        };
        assert_eq!(b.label, "ok");
        assert_eq!(b.variant, ButtonVariant::Primary);
        assert_eq!(b.on_press, Some(action));
    }

    #[test]
    fn push_view_rejects_unknown_class() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let err = tree.push_view(root, "not-a-known-token").unwrap_err();
        let ClassParseError::UnknownToken(t) = err;
        assert_eq!(t, "not-a-known-token");
    }

    #[test]
    #[should_panic(expected = "stale or never existed")]
    fn push_with_stale_parent_panics_loudly() {
        let mut tree = AppTree::new();
        let stale = NodeId::from_raw(99, 0);
        let _ = tree.push_view(stale, "");
    }

    #[test]
    fn mutation_marks_tree_dirty() {
        let mut tree = AppTree::new();
        assert!(!tree.is_dirty());
        tree.push_view(tree.root(), "").unwrap();
        assert!(tree.is_dirty());
        tree.clear_dirty();
        assert!(!tree.is_dirty());
    }

    #[test]
    fn node_ids_use_generation_counters() {
        // Smoke: a fresh allocation should have generation 0 (Codex Q6 sanity).
        let mut tree = AppTree::new();
        let id = tree.push_view(tree.root(), "").unwrap();
        // Root is 0,0; first user node should be 1,0.
        assert_eq!(id.index(), 1);
        assert_eq!(id.generation(), 0);
    }
}
