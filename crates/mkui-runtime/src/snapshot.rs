//! Canonical JSON snapshots of an [`AppTree`].
//!
//! The parity gate (issue #51 acceptance criterion #2) asserts that an
//! `AppTree` built from Rust, C, and Python produces a byte-identical JSON
//! string. To make byte-identity achievable across languages we control:
//!
//! 1. **Iteration order**: traverse from the root depth-first, children in
//!    declaration order. The arena's free-list reuse never affects this
//!    order — children store node ids and we follow them, not the slot
//!    vector.
//! 2. **Field order**: fixed by the `serde::Serialize` derives. We use the
//!    derived order, not `serde_json::to_value` (which would `BTreeMap`-sort
//!    object keys). Field order is structural, not alphabetical, because
//!    the snapshot's job is to mirror the typed shape — adding a field
//!    is intentionally a breaking change to the snapshot.
//! 3. **No whitespace**: compact serialization. Pretty-print is opt-in.
//!
//! Snapshots are intentionally **structural**: the raw class string is
//! emitted alongside the resolved structure, but renderers may layer
//! additional state (cursors, focus) that the snapshot does not see. A
//! parity test that needs renderer state should bypass the snapshot.

use serde::Serialize;

use crate::tree::{AppTree, Node, NodeId};

/// Snapshot of an [`AppTree`] suitable for cross-binding diff. Ordered the
/// same way every time so byte-identical comparisons work.
#[derive(Debug, Serialize)]
pub struct TreeSnapshot {
    pub root: NodeId,
    pub nodes: Vec<NodeSnapshot>,
}

/// Snapshot of a single node. Mirrors [`Node`] but inlines children so the
/// JSON is self-contained.
#[derive(Debug, Serialize)]
pub struct NodeSnapshot {
    pub id: NodeId,
    pub parent: Option<NodeId>,
    #[serde(flatten)]
    pub kind: KindSnapshot,
    pub class: String,
    pub resolved: crate::style::ResolvedStyle,
    pub children: Vec<NodeSnapshot>,
}

/// Variant payload — flattened tag, mirrors [`crate::tree::NodeKind`] but
/// owns its data (the snapshot tree may outlive the arena slot).
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum KindSnapshot {
    Root,
    View,
    Text {
        content: String,
        variant: crate::props::TextVariant,
    },
    Button {
        label: String,
        variant: crate::props::ButtonVariant,
        on_press: Option<crate::actions::ActionId>,
    },
    Custom {
        type_name: String,
        props: serde_json::Value,
    },
}

impl TreeSnapshot {
    /// Build a snapshot rooted at the tree's root. Nodes are emitted in
    /// depth-first declaration order so the resulting JSON is stable.
    pub fn of(tree: &AppTree) -> Self {
        let root = tree.root();
        let root_node = tree.get(root).expect("root must exist");
        Self {
            root,
            nodes: vec![NodeSnapshot::of(tree, root_node)],
        }
    }

    /// Compact JSON. Use this for byte-identical comparisons.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("TreeSnapshot must serialize without error")
    }

    /// Pretty-printed JSON. Use for diagnostics only — two pretty strings
    /// can match while a compact comparison detects an extra space or a
    /// different field order, so the parity gate uses [`Self::to_json`].
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("TreeSnapshot must serialize without error")
    }
}

impl NodeSnapshot {
    fn of(tree: &AppTree, node: &Node) -> Self {
        let kind = match &node.kind {
            crate::tree::NodeKind::Root => KindSnapshot::Root,
            crate::tree::NodeKind::View(_) => KindSnapshot::View,
            crate::tree::NodeKind::Text(t) => KindSnapshot::Text {
                content: t.content.clone(),
                variant: t.variant,
            },
            crate::tree::NodeKind::Button(b) => KindSnapshot::Button {
                label: b.label.clone(),
                variant: b.variant,
                on_press: b.on_press,
            },
            crate::tree::NodeKind::Custom { type_name, props } => KindSnapshot::Custom {
                type_name: type_name.clone(),
                props: props.clone(),
            },
        };
        let children: Vec<NodeSnapshot> = node
            .children
            .iter()
            .filter_map(|cid| tree.get(*cid).map(|c| NodeSnapshot::of(tree, c)))
            .collect();
        Self {
            id: node.id,
            parent: node.parent,
            kind,
            class: node.class.raw().to_string(),
            resolved: node.resolved.clone(),
            children,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::props::{ButtonVariant, TextVariant};

    fn build_sample_tree() -> AppTree {
        // Mirror the showcase shape: View → (Text + Button) under root.
        let mut tree = AppTree::new();
        let root = tree.root();
        let container = tree.push_view(root, "flex items-center").unwrap();
        tree.push_text(container, "hello", TextVariant::Heading1, "text-2xl")
            .unwrap();
        let action = tree.actions_mut().register_local(|ctx| ctx.mark_dirty());
        tree.push_button(container, "ok", ButtonVariant::Primary, "p-6", Some(action))
            .unwrap();
        tree
    }

    #[test]
    fn snapshot_roundtrip_is_stable_across_calls() {
        // Same tree → same JSON, every time. This is the byte-identity
        // property the cross-binding parity tests rely on.
        let tree = build_sample_tree();
        let a = TreeSnapshot::of(&tree).to_json();
        let b = TreeSnapshot::of(&tree).to_json();
        assert_eq!(a, b);
    }

    #[test]
    fn snapshot_reflects_declaration_order() {
        let tree = build_sample_tree();
        let json = TreeSnapshot::of(&tree).to_json();
        // Text declared before Button under the container → must appear
        // first in the children array.
        let text_pos = json.find("\"hello\"").expect("text content present");
        let button_pos = json.find("\"ok\"").expect("button label present");
        assert!(
            text_pos < button_pos,
            "child order must match declaration order"
        );
    }

    #[test]
    fn snapshot_does_not_include_dropped_action_state() {
        // The snapshot carries the action id (so two construction frontends
        // can compare the *shape* of action wiring) but not the closure
        // itself — closures are not portable across languages.
        let tree = build_sample_tree();
        let json = TreeSnapshot::of(&tree).to_json();
        assert!(json.contains("on_press"));
        // The string "RefCell" / "Rc" never appears in a portable JSON.
        assert!(!json.contains("RefCell"));
    }

    #[test]
    fn two_trees_built_the_same_way_serialize_identically() {
        // Sanity: byte-identity does hold when both trees are built with
        // matching steps. Parity tests across bindings rely on this.
        // Action ids start at (0,0) on a fresh tree, so as long as both
        // sequences allocate in the same order the snapshots match.
        let a = build_sample_tree();
        let b = build_sample_tree();
        assert_eq!(
            TreeSnapshot::of(&a).to_json(),
            TreeSnapshot::of(&b).to_json()
        );
    }
}
