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

/// Payload for a `View` node — a layout-only container. It carries no
/// intrinsic data of its own; geometry and appearance come entirely from the
/// node's `class` / `resolved` style projection. The empty struct keeps the
/// `NodeKind::View` arm symmetric with the other variants.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewProps {}

/// Payload for a `Text` node: the string to render plus the semantic
/// [`TextVariant`] that drives the renderer's typographic scale.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextProps {
    pub content: String,
    pub variant: TextVariant,
}

/// Payload for a `Button` node: the visible `label`, its visual
/// [`ButtonVariant`], and an optional `on_press` [`ActionId`] resolved against
/// the [`ActionRegistry`] when the button fires.
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
    /// Monotonic counter bumped by every fine-grained mutation routed
    /// through [`AppTree::touch`] (`push_*`, `set_text`, `set_button_label`)
    /// (#229). Each touched node's slot, and every ancestor up to the root,
    /// records the counter's new value as its `subtree_revision` — see
    /// [`AppTree::subtree_revision`] for what a caller does with that.
    next_revision: u64,
    /// Bumped only by the coarse [`AppTree::mark_dirty`] escape hatch (the
    /// path an action closure uses to request a redraw after mutating state
    /// `touch` never sees — e.g. data a custom renderer reads from outside
    /// the tree). A per-subtree cache keyed on `subtree_revision` alone
    /// cannot see that kind of change, so it additionally gates on `epoch`
    /// (see [`AppTree::epoch`]): any bump invalidates every cached subtree,
    /// trading precision for safety on the untracked-mutation path.
    epoch: u64,
    actions: ActionRegistry,
}

#[derive(Debug)]
struct Slot {
    generation: u32,
    node: Option<Node>,
    /// Revision this node's own content last changed at (#229). `0` until
    /// the node's first mutation.
    own_revision: u64,
    /// `max` of `own_revision` and every descendant's `subtree_revision`
    /// (#229) — the value [`AppTree::subtree_revision`] returns.
    subtree_revision: u64,
}

impl AppTree {
    /// Create an empty tree with a synthetic [`NodeKind::Root`].
    pub fn new() -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            free: Vec::new(),
            root: NodeId::from_raw(0, 0),
            dirty: false,
            next_revision: 0,
            epoch: 0,
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
    /// any fired action that itself sets `RuntimeCtx::mark_dirty`. Also
    /// bumps [`AppTree::epoch`] (#229) — this is the coarse, whole-tree
    /// escape hatch, so a per-subtree cache has no finer signal to key on
    /// and must invalidate everything. `push_*`/`set_*` do **not** call
    /// this; they route through the private fine-grained revision tracker
    /// instead, which sets `dirty` without bumping `epoch`.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.epoch += 1;
    }

    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Bumped by [`AppTree::mark_dirty`] only (#229). A per-subtree render
    /// cache keyed on [`AppTree::subtree_revision`] must also invalidate
    /// everything whenever this changes — `mark_dirty` is the signal an
    /// action fires after mutating state outside the tracked setters, which
    /// `subtree_revision` cannot see.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The revision `id`'s subtree (the node itself plus every descendant)
    /// last changed at (#229), or `None` if `id` is stale/missing. `0`
    /// means untouched since construction. A caller that cached this
    /// subtree's rendered output tagged with a previous call's return value
    /// can treat an unchanged value as "nothing inside this subtree moved
    /// since" and reuse the cached output instead of re-walking — see
    /// `mkui-wgpu`'s `walker::WalkCache`.
    pub fn subtree_revision(&self, id: NodeId) -> Option<u64> {
        let slot = self.nodes.get(id.index() as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.node.as_ref()?;
        Some(slot.subtree_revision)
    }

    /// Route a mutation to node `id` through the fine-grained revision
    /// tracker (#229): bump the global counter, stamp `id`'s slot as
    /// changed at the new revision, and propagate that same revision up
    /// through every ancestor's `subtree_revision` (parent links only — no
    /// subtree walk) so an ancestor's cached subtree can also tell it needs
    /// a re-walk. Also sets the coarse `dirty` flag, same as the old
    /// `mark_dirty` call sites this replaces, but — deliberately — does
    /// **not** bump `epoch`: that stays reserved for mutations `touch`
    /// never sees (see [`AppTree::mark_dirty`]).
    fn touch(&mut self, id: NodeId) {
        self.next_revision += 1;
        let revision = self.next_revision;
        if let Some(slot) = self.nodes.get_mut(id.index() as usize) {
            if slot.generation == id.generation {
                slot.own_revision = revision;
                slot.subtree_revision = revision;
            }
        }
        let mut current = self.get(id).and_then(|n| n.parent);
        while let Some(ancestor) = current {
            let Some(slot) = self.nodes.get_mut(ancestor.index() as usize) else {
                break;
            };
            if slot.generation != ancestor.generation {
                break;
            }
            slot.subtree_revision = revision;
            current = slot.node.as_ref().and_then(|n| n.parent);
        }
        self.dirty = true;
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

    /// Iterate every live node in slot order. This is insertion order only
    /// until a [`AppTree::remove_node`] + reuse cycle recycles a
    /// lower-index slot (#83) — not load-bearing for parity: the byte-
    /// identical snapshot gate (`crate::snapshot::TreeSnapshot::of`) walks
    /// from the root through each node's `children` instead of this
    /// iterator, so a binding only needs to apply the same operations in
    /// the same order, not match this method's own order.
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

    // ---- in-place mutation for already-built nodes (#177) ----
    //
    // An embedding host that ticks the tree every frame (a HUD health
    // value, a score counter, …) writes the same value on most frames.
    // These setters compare before writing: an unchanged value returns
    // `false` and never touches `mark_dirty`, so a host can call them
    // unconditionally every frame without forcing a rebuild — and without
    // allocating, since `content: &str` (not `impl Into<String>`) keeps
    // the caller from allocating a `String` before the comparison runs.

    /// Overwrite a `Text` node's content in place. Returns `false` (no
    /// mutation, tree stays clean) when `content` already matches, `true`
    /// when it changed (the tree is now dirty). `false` is also returned
    /// if `id` is stale/missing or does not name a `Text` node.
    pub fn set_text(&mut self, id: NodeId, content: &str) -> bool {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return false;
            };
            let NodeKind::Text(text) = &mut node.kind else {
                return false;
            };
            if text.content == content {
                false
            } else {
                text.content.clear();
                text.content.push_str(content);
                true
            }
        };
        if changed {
            self.touch(id);
        }
        changed
    }

    /// Overwrite a `Button` node's label in place. Same no-op-on-equal
    /// contract as [`AppTree::set_text`].
    pub fn set_button_label(&mut self, id: NodeId, label: &str) -> bool {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return false;
            };
            let NodeKind::Button(button) = &mut node.kind else {
                return false;
            };
            if button.label == label {
                false
            } else {
                button.label.clear();
                button.label.push_str(label);
                true
            }
        };
        if changed {
            self.touch(id);
        }
        changed
    }

    /// Replace a node's class string in place, re-resolving its style —
    /// same no-op-on-equal, same-`NodeId`-survives contract as
    /// [`AppTree::set_text`] (#268). Returns `false` (no mutation) when
    /// `class` already matches `node.class.raw()`, when `id` is
    /// stale/missing, or on a parse error (never today — see
    /// [`StyleClass::parse`]).
    pub fn set_class(&mut self, id: NodeId, class: &str) -> Result<bool, ClassParseError> {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return Ok(false);
            };
            if node.class.raw() == class {
                false
            } else {
                node.class.set_raw(class);
                node.resolved = node.class.parse()?;
                true
            }
        };
        if changed {
            self.touch(id);
        }
        Ok(changed)
    }

    /// Wholesale replace a `Text` node's content *and* variant in one
    /// touch — the "replace a node's props in place" half of #268, for
    /// callers that need to change more than [`AppTree::set_text`]'s
    /// single field without losing the node's identity. No-op-on-equal:
    /// returns `false` if both fields already match (and does not touch
    /// the tree), `false` if `id` is stale/missing or not a `Text` node.
    pub fn set_text_props(&mut self, id: NodeId, content: &str, variant: TextVariant) -> bool {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return false;
            };
            let NodeKind::Text(text) = &mut node.kind else {
                return false;
            };
            if text.content == content && text.variant == variant {
                false
            } else {
                if text.content != content {
                    text.content.clear();
                    text.content.push_str(content);
                }
                text.variant = variant;
                true
            }
        };
        if changed {
            self.touch(id);
        }
        changed
    }

    /// Wholesale replace a `Button` node's label, variant, and
    /// `on_press` action in one touch. Same no-op-on-equal contract as
    /// [`AppTree::set_text_props`].
    pub fn set_button_props(
        &mut self,
        id: NodeId,
        label: &str,
        variant: ButtonVariant,
        on_press: Option<ActionId>,
    ) -> bool {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return false;
            };
            let NodeKind::Button(button) = &mut node.kind else {
                return false;
            };
            if button.label == label && button.variant == variant && button.on_press == on_press {
                false
            } else {
                if button.label != label {
                    button.label.clear();
                    button.label.push_str(label);
                }
                button.variant = variant;
                button.on_press = on_press;
                true
            }
        };
        if changed {
            self.touch(id);
        }
        changed
    }

    /// Wholesale replace a `Custom` node's `props` payload (`type_name`
    /// stays fixed — swapping it is a different node kind, not a props
    /// update). Same no-op-on-equal contract as [`AppTree::set_text_props`].
    ///
    /// Takes `props: &serde_json::Value` (not by value) for the same
    /// reason [`AppTree::set_text`] takes `content: &str`: a host ticking
    /// this every frame with an unchanged value must not pay for building
    /// (or cloning) a `Value` just to discover it matches. The clone only
    /// happens on the actually-changed path.
    pub fn set_custom_props(&mut self, id: NodeId, props: &serde_json::Value) -> bool {
        let changed = {
            let Some(node) = self.get_mut(id) else {
                return false;
            };
            let NodeKind::Custom { props: current, .. } = &mut node.kind else {
                return false;
            };
            if current == props {
                false
            } else {
                *current = props.clone();
                true
            }
        };
        if changed {
            self.touch(id);
        }
        changed
    }

    /// Reorder `parent`'s children in place: `order` must be a
    /// permutation of `parent`'s current children (same `NodeId`s, any
    /// sequence) — every child keeps its identity, no node is removed or
    /// re-added (#268). Returns `false` (no mutation) when `parent` is
    /// stale/missing, `order` is not a permutation of the current
    /// children (wrong length or a `NodeId` that doesn't belong), or
    /// `order` already matches the current sequence.
    ///
    /// Only `parent` is touched — not its children — so a child's own
    /// [`AppTree::subtree_revision`] is unaffected by its siblings moving
    /// around it, which lets a per-subtree render cache (keyed by
    /// `NodeId` + the child's own layout rect, e.g. `mkui-wgpu`'s
    /// `WalkCache`) keep reusing that child's cached paint output after a
    /// reorder.
    pub fn reorder_children(&mut self, parent: NodeId, order: &[NodeId]) -> bool {
        let Some(node) = self.get(parent) else {
            return false;
        };
        if node.children == order {
            return false;
        }
        if order.len() != node.children.len() {
            return false;
        }
        // Validate `order` is a permutation of the current children
        // before mutating anything (O(n^2) swap-remove match — fine for
        // the small sibling lists this targets; a caller with thousands
        // of siblings would want a different structure regardless).
        let mut remaining = node.children.clone();
        for id in order {
            match remaining.iter().position(|c| c == id) {
                Some(pos) => {
                    remaining.swap_remove(pos);
                }
                None => return false,
            }
        }
        let Some(node) = self.get_mut(parent) else {
            return false;
        };
        node.children.clear();
        node.children.extend_from_slice(order);
        self.touch(parent);
        true
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
        self.touch(id);
        id
    }

    /// Remove `id` and every descendant, releasing each freed slot's
    /// `NodeId` index back to `alloc_node`'s `free` pool and
    /// unregistering any [`ActionId`] a removed `Button` held. Returns the
    /// removed node (the subtree root only, not its descendants), or `None`
    /// if `id` is stale/missing **or names the root** — the root has no
    /// parent to detach from and the tree always carries exactly one, so
    /// removing it is rejected the same way a stale id is.
    ///
    /// Each freed slot's generation is bumped here, at removal time (not
    /// deferred to the next `alloc_node` reuse) — see
    /// [`NodeId`]'s docs: a lookup against the pre-removal id must return
    /// `None` immediately, not only after the slot happens to be recycled.
    ///
    /// Nothing prevents two `Button`s from sharing one [`ActionId`] (the C
    /// and Python bindings' `register_callback` explicitly allow it: hand
    /// the same id to two `*_child` calls). Removing either button
    /// unregisters that shared id via [`ActionRegistry::remove`] — the
    /// other button's `on_press` silently stops firing too. This is the
    /// issue's mandated behavior (unregister on removal, no refcounting),
    /// not an oversight; a caller that needs two live buttons to survive
    /// one removal must register two separate actions.
    pub fn remove_node(&mut self, id: NodeId) -> Option<Node> {
        let node = self.get(id)?;
        let parent = node.parent?;
        let removed = self.free_subtree(id);
        if let Some(parent_node) = self.get_mut(parent) {
            parent_node.children.retain(|child| *child != id);
        }
        self.touch(parent);
        removed
    }

    /// Post-order free of `id` and every descendant: children first, then
    /// `id`'s own slot. Does **not** touch `id`'s entry in its parent's
    /// `children` list — the caller ([`AppTree::remove_node`]) does that
    /// once, for the subtree root only; a freed descendant's parent is
    /// being freed in the same pass, so patching its `children` Vec would
    /// be wasted work.
    fn free_subtree(&mut self, id: NodeId) -> Option<Node> {
        let node = self.get(id)?.clone();
        for child in node.children.clone() {
            self.free_subtree(child);
        }
        if let NodeKind::Button(ButtonProps {
            on_press: Some(action_id),
            ..
        }) = &node.kind
        {
            self.actions.remove(*action_id);
        }
        let slot = &mut self.nodes[id.index() as usize];
        slot.generation = slot.generation.wrapping_add(1);
        slot.node = None;
        slot.own_revision = 0;
        slot.subtree_revision = 0;
        self.free.push(id.index());
        Some(node)
    }

    fn alloc_node(&mut self, parent: Option<NodeId>, kind: NodeKind, class: StyleClass) -> NodeId {
        let resolved = class.parse().unwrap_or_default();
        if let Some(idx) = self.free.pop() {
            let slot = &mut self.nodes[idx as usize];
            // Generation was already bumped by `free_subtree` when this slot
            // was freed — do not bump again here, or a lookup against the
            // id minted by *this* allocation would itself look one
            // generation too new relative to what `remove_node` guaranteed.
            slot.own_revision = 0;
            slot.subtree_revision = 0;
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
                own_revision: 0,
                subtree_revision: 0,
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
    fn readme_justify_center_example_survives_the_unchecked_ffi_path() {
        // Regression for #63 acceptance criterion #7. `mkui-c` / `mkui-py`
        // call `push_view_unchecked` (they validate nothing themselves), and
        // `alloc_node` falls back to `ResolvedStyle::default()` on *any*
        // parse error via `unwrap_or_default()` — silently, no panic. Before
        // `justify-center` was a known token, the README's own
        // `"flex-1 items-center justify-center"` example would hit exactly
        // that path and come out with every field unset, including
        // `flex_grow` and `items_center`, which the string's own other
        // tokens set just fine on their own.
        let mut tree = AppTree::new();
        let root = tree.root();
        let style = StyleClass::from_str("flex-1 items-center justify-center");
        let view = tree.push_view_unchecked(root, style);
        let node = tree.get(view).unwrap();
        assert!(node.resolved.flex_grow);
        assert!(node.resolved.items_center);
        assert!(node.resolved.justify_center);
    }

    #[test]
    fn push_view_accepts_unknown_class_and_drops_it() {
        // #269: an unrecognized class token no longer fails `push_view` — it
        // is dropped (and counted) while the node still gets created.
        let mut tree = AppTree::new();
        let root = tree.root();
        let view = tree
            .push_view(root, "flex not-a-known-token items-center")
            .expect("unknown token must not fail construction");
        let node = tree.get(view).unwrap();
        assert!(node.resolved.flex);
        assert!(node.resolved.items_center);
        assert_eq!(node.resolved.unknown_count, 1);
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

    #[test]
    fn set_text_with_unchanged_value_is_a_no_op() {
        let mut tree = AppTree::new();
        let text = tree
            .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
            .unwrap();
        tree.clear_dirty();
        assert!(!tree.set_text(text, "hp: 100"));
        assert!(!tree.is_dirty());
        let NodeKind::Text(t) = &tree.get(text).unwrap().kind else {
            panic!("expected Text")
        };
        assert_eq!(t.content, "hp: 100");
    }

    #[test]
    fn set_text_with_new_value_marks_dirty_and_updates() {
        let mut tree = AppTree::new();
        let text = tree
            .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
            .unwrap();
        tree.clear_dirty();
        assert!(tree.set_text(text, "hp: 90"));
        assert!(tree.is_dirty());
        let NodeKind::Text(t) = &tree.get(text).unwrap().kind else {
            panic!("expected Text")
        };
        assert_eq!(t.content, "hp: 90");
    }

    #[test]
    fn set_text_on_non_text_node_returns_false() {
        let mut tree = AppTree::new();
        let view = tree.push_view(tree.root(), "").unwrap();
        tree.clear_dirty();
        assert!(!tree.set_text(view, "hp: 90"));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn set_text_on_stale_node_returns_false() {
        let mut tree = AppTree::new();
        let stale = NodeId::from_raw(99, 0);
        assert!(!tree.set_text(stale, "anything"));
    }

    #[test]
    fn set_button_label_with_unchanged_value_is_a_no_op() {
        let mut tree = AppTree::new();
        let button = tree
            .push_button(tree.root(), "Pause", ButtonVariant::Primary, "", None)
            .unwrap();
        tree.clear_dirty();
        assert!(!tree.set_button_label(button, "Pause"));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn set_button_label_with_new_value_marks_dirty_and_updates() {
        let mut tree = AppTree::new();
        let button = tree
            .push_button(tree.root(), "Pause", ButtonVariant::Primary, "", None)
            .unwrap();
        tree.clear_dirty();
        assert!(tree.set_button_label(button, "Resume"));
        assert!(tree.is_dirty());
        let NodeKind::Button(b) = &tree.get(button).unwrap().kind else {
            panic!("expected Button")
        };
        assert_eq!(b.label, "Resume");
    }

    // ---- per-subtree revision tracking (#229) ----

    #[test]
    fn set_text_bumps_the_nodes_own_subtree_revision_and_every_ancestors() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let row = tree.push_view(root, "").unwrap();
        let text = tree
            .push_text(row, "hp: 100", TextVariant::Body, "")
            .unwrap();

        let root_rev_before = tree.subtree_revision(root).unwrap();
        let row_rev_before = tree.subtree_revision(row).unwrap();
        let text_rev_before = tree.subtree_revision(text).unwrap();

        assert!(tree.set_text(text, "hp: 90"));

        assert!(tree.subtree_revision(text).unwrap() > text_rev_before);
        assert!(
            tree.subtree_revision(row).unwrap() > row_rev_before,
            "a child's mutation must bump its parent's subtree_revision too"
        );
        assert!(
            tree.subtree_revision(root).unwrap() > root_rev_before,
            "a child's mutation must propagate all the way to the root"
        );
    }

    #[test]
    fn set_text_on_unchanged_value_does_not_bump_any_subtree_revision() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let text = tree
            .push_text(root, "hp: 100", TextVariant::Body, "")
            .unwrap();
        let root_rev = tree.subtree_revision(root).unwrap();
        let text_rev = tree.subtree_revision(text).unwrap();

        assert!(!tree.set_text(text, "hp: 100"));

        assert_eq!(tree.subtree_revision(text).unwrap(), text_rev);
        assert_eq!(tree.subtree_revision(root).unwrap(), root_rev);
    }

    #[test]
    fn mutating_one_sibling_leaves_an_unrelated_siblings_subtree_revision_untouched() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let dirty_row = tree
            .push_text(root, "hp: 100", TextVariant::Body, "")
            .unwrap();
        let clean_row = tree
            .push_text(root, "level 3", TextVariant::Body, "")
            .unwrap();
        let clean_rev = tree.subtree_revision(clean_row).unwrap();

        assert!(tree.set_text(dirty_row, "hp: 90"));

        assert_eq!(
            tree.subtree_revision(clean_row).unwrap(),
            clean_rev,
            "an unrelated sibling's subtree_revision must not change"
        );
    }

    #[test]
    fn push_bumps_the_new_parents_subtree_revision() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let container = tree.push_view(root, "").unwrap();
        let before = tree.subtree_revision(container).unwrap();

        tree.push_text(container, "new row", TextVariant::Body, "")
            .unwrap();

        assert!(tree.subtree_revision(container).unwrap() > before);
    }

    #[test]
    fn mark_dirty_bumps_epoch_but_set_text_does_not() {
        let mut tree = AppTree::new();
        let text = tree
            .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
            .unwrap();
        let epoch_after_push = tree.epoch();

        assert!(tree.set_text(text, "hp: 90"));
        assert_eq!(
            tree.epoch(),
            epoch_after_push,
            "a tracked setter must not bump epoch — only the coarse mark_dirty escape hatch does"
        );

        tree.mark_dirty();
        assert!(tree.epoch() > epoch_after_push);
    }

    // ---- reorder_children (#268) ----

    #[test]
    fn reorder_children_preserves_every_childs_identity() {
        // Acceptance criterion 1: reordering must not remove/re-add any
        // node — the same NodeIds come back, just in a new sequence.
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_text(root, "a", TextVariant::Body, "").unwrap();
        let b = tree.push_text(root, "b", TextVariant::Body, "").unwrap();
        let c = tree.push_text(root, "c", TextVariant::Body, "").unwrap();
        tree.clear_dirty();

        assert!(tree.reorder_children(root, &[c, a, b]));

        assert_eq!(tree.get(root).unwrap().children, vec![c, a, b]);
        // Every id is exactly the same (index, generation) pair — no
        // removal/recycle happened under the hood.
        for (id, content) in [(a, "a"), (b, "b"), (c, "c")] {
            let NodeKind::Text(t) = &tree.get(id).unwrap().kind else {
                panic!("expected Text")
            };
            assert_eq!(t.content, content);
        }
        assert!(tree.is_dirty());
    }

    #[test]
    fn reorder_children_to_the_same_order_is_a_no_op() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_view(root, "").unwrap();
        let b = tree.push_view(root, "").unwrap();
        tree.clear_dirty();

        assert!(!tree.reorder_children(root, &[a, b]));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn reorder_children_only_touches_the_parent_not_the_children() {
        // A reordered child's own subtree_revision must not move — that's
        // what lets a per-subtree render cache keep reusing its cached
        // output after the reorder (see the method's docs).
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_view(root, "").unwrap();
        let b = tree.push_view(root, "").unwrap();
        let a_rev_before = tree.subtree_revision(a).unwrap();
        let b_rev_before = tree.subtree_revision(b).unwrap();
        let root_rev_before = tree.subtree_revision(root).unwrap();

        assert!(tree.reorder_children(root, &[b, a]));

        assert_eq!(tree.subtree_revision(a).unwrap(), a_rev_before);
        assert_eq!(tree.subtree_revision(b).unwrap(), b_rev_before);
        assert!(tree.subtree_revision(root).unwrap() > root_rev_before);
    }

    #[test]
    fn reorder_children_rejects_a_foreign_node_id() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_view(root, "").unwrap();
        let foreign = NodeId::from_raw(99, 0);
        tree.clear_dirty();

        assert!(!tree.reorder_children(root, &[a, foreign]));
        assert!(!tree.is_dirty());
        assert_eq!(tree.get(root).unwrap().children, vec![a]);
    }

    #[test]
    fn reorder_children_rejects_wrong_length() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let a = tree.push_view(root, "").unwrap();
        let _b = tree.push_view(root, "").unwrap();
        tree.clear_dirty();

        assert!(!tree.reorder_children(root, &[a]));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn reorder_children_on_stale_parent_returns_false() {
        let mut tree = AppTree::new();
        let stale = NodeId::from_raw(99, 0);
        assert!(!tree.reorder_children(stale, &[]));
    }

    // ---- set_class / set_*_props (#268) ----

    #[test]
    fn set_class_replaces_class_and_resolved_style_keeping_the_same_node_id() {
        let mut tree = AppTree::new();
        let view = tree.push_view(tree.root(), "flex").unwrap();
        tree.clear_dirty();

        assert!(tree.set_class(view, "flex-col items-center").unwrap());

        let node = tree.get(view).unwrap();
        assert_eq!(node.id, view, "same NodeId must survive the class replace");
        assert_eq!(node.class.raw(), "flex-col items-center");
        assert!(node.resolved.flex_column);
        assert!(node.resolved.items_center);
        assert!(!node.resolved.flex, "old class's tokens must not linger");
        assert!(tree.is_dirty());
    }

    #[test]
    fn set_class_with_unchanged_value_is_a_no_op() {
        let mut tree = AppTree::new();
        let view = tree.push_view(tree.root(), "flex").unwrap();
        tree.clear_dirty();

        assert!(!tree.set_class(view, "flex").unwrap());
        assert!(!tree.is_dirty());
    }

    #[test]
    fn set_text_props_replaces_content_and_variant_keeping_the_same_node_id() {
        let mut tree = AppTree::new();
        let text = tree
            .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
            .unwrap();
        tree.clear_dirty();

        assert!(tree.set_text_props(text, "GAME OVER", TextVariant::Heading1));

        let node = tree.get(text).unwrap();
        assert_eq!(node.id, text);
        let NodeKind::Text(t) = &node.kind else {
            panic!("expected Text")
        };
        assert_eq!(t.content, "GAME OVER");
        assert_eq!(t.variant, TextVariant::Heading1);
        assert!(tree.is_dirty());
    }

    #[test]
    fn set_text_props_with_unchanged_values_is_a_no_op() {
        let mut tree = AppTree::new();
        let text = tree
            .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
            .unwrap();
        tree.clear_dirty();

        assert!(!tree.set_text_props(text, "hp: 100", TextVariant::Body));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn set_button_props_replaces_label_variant_and_action_keeping_the_same_node_id() {
        let mut tree = AppTree::new();
        let button = tree
            .push_button(tree.root(), "Pause", ButtonVariant::Primary, "", None)
            .unwrap();
        let action = tree.actions_mut().register_local(|ctx| ctx.mark_dirty());
        tree.clear_dirty();

        assert!(tree.set_button_props(button, "Resume", ButtonVariant::Secondary, Some(action)));

        let node = tree.get(button).unwrap();
        assert_eq!(node.id, button);
        let NodeKind::Button(b) = &node.kind else {
            panic!("expected Button")
        };
        assert_eq!(b.label, "Resume");
        assert_eq!(b.variant, ButtonVariant::Secondary);
        assert_eq!(b.on_press, Some(action));
        assert!(tree.is_dirty());
    }

    #[test]
    fn set_button_props_with_unchanged_values_is_a_no_op() {
        let mut tree = AppTree::new();
        let button = tree
            .push_button(tree.root(), "Pause", ButtonVariant::Primary, "", None)
            .unwrap();
        tree.clear_dirty();

        assert!(!tree.set_button_props(button, "Pause", ButtonVariant::Primary, None));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn set_custom_props_replaces_props_keeping_the_same_node_id_and_type_name() {
        let mut tree = AppTree::new();
        let custom = tree
            .push_custom(tree.root(), "badge", serde_json::json!({"count": 1}), "")
            .unwrap();
        tree.clear_dirty();

        assert!(tree.set_custom_props(custom, &serde_json::json!({"count": 2})));

        let node = tree.get(custom).unwrap();
        assert_eq!(node.id, custom);
        let NodeKind::Custom { type_name, props } = &node.kind else {
            panic!("expected Custom")
        };
        assert_eq!(type_name, "badge");
        assert_eq!(props, &serde_json::json!({"count": 2}));
        assert!(tree.is_dirty());
    }

    #[test]
    fn set_custom_props_with_unchanged_value_is_a_no_op() {
        let mut tree = AppTree::new();
        let custom = tree
            .push_custom(tree.root(), "badge", serde_json::json!({"count": 1}), "")
            .unwrap();
        tree.clear_dirty();

        assert!(!tree.set_custom_props(custom, &serde_json::json!({"count": 1})));
        assert!(!tree.is_dirty());
    }

    #[test]
    fn subtree_revision_of_a_stale_node_is_none() {
        let tree = AppTree::new();
        let stale = NodeId::from_raw(99, 0);
        assert!(tree.subtree_revision(stale).is_none());
    }

    // ---- remove_node (#83) ----

    #[test]
    fn remove_node_detaches_from_its_parents_children() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let view = tree.push_view(root, "").unwrap();
        tree.clear_dirty();

        let removed = tree.remove_node(view);
        assert!(removed.is_some());
        assert!(tree.get(root).unwrap().children.is_empty());
        assert!(
            tree.get(view).is_none(),
            "removed id must no longer resolve"
        );
        assert!(tree.is_dirty());
    }

    #[test]
    fn remove_node_recursively_removes_children() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let parent = tree.push_view(root, "").unwrap();
        let child = tree.push_text(parent, "hi", TextVariant::Body, "").unwrap();
        let grandchild = tree.push_view(child, "").unwrap();

        tree.remove_node(parent);

        assert!(tree.get(parent).is_none());
        assert!(tree.get(child).is_none(), "child must be removed too");
        assert!(
            tree.get(grandchild).is_none(),
            "grandchild must be removed too"
        );
        // root itself and its direct children list survive.
        assert!(tree.get(root).unwrap().children.is_empty());
    }

    #[test]
    fn remove_node_unregisters_a_buttons_on_press_action() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let action = tree.actions_mut().register_local(|ctx| ctx.mark_dirty());
        let button = tree
            .push_button(root, "ok", ButtonVariant::Primary, "", Some(action))
            .unwrap();

        tree.remove_node(button);

        assert!(
            tree.actions().get(action).is_none(),
            "the button's action must be unregistered on node removal"
        );
    }

    #[test]
    fn remove_node_on_a_stale_id_returns_none() {
        let mut tree = AppTree::new();
        let stale = NodeId::from_raw(99, 0);
        assert!(tree.remove_node(stale).is_none());
    }

    #[test]
    fn remove_node_twice_on_the_same_id_is_idempotent() {
        let mut tree = AppTree::new();
        let view = tree.push_view(tree.root(), "").unwrap();
        assert!(tree.remove_node(view).is_some());
        assert!(
            tree.remove_node(view).is_none(),
            "a second remove on the same (now-stale) id must be a no-op"
        );
    }

    #[test]
    fn remove_node_rejects_the_root() {
        let mut tree = AppTree::new();
        let root = tree.root();
        assert!(
            tree.remove_node(root).is_none(),
            "the root has no parent to detach from and must be protected"
        );
        assert!(tree.get(root).is_some(), "root must still be live");
    }

    #[test]
    fn removed_node_slot_is_recycled_with_a_bumped_generation() {
        let mut tree = AppTree::new();
        let root = tree.root();
        let first = tree.push_view(root, "").unwrap();
        tree.remove_node(first);

        let second = tree.push_view(root, "").unwrap();
        assert_eq!(
            second.index(),
            first.index(),
            "the freed slot must be reused"
        );
        assert_ne!(
            second.generation(),
            first.generation(),
            "the recycled slot must carry a new generation"
        );
        assert!(tree.get(first).is_none());
        assert!(tree.get(second).is_some());
    }
}
