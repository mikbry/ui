//! Cross-binding parity tests for the `mkui-runtime` substrate.
//!
//! Issue #51 acceptance criterion #2: "Rust building `Mkui::new()?.child(...)`,
//! C building `mkui_app_view_child(...)`, and Python building
//! `app.view_child(...)` all produce byte-identical JSON snapshots of the
//! resulting `AppTree`."
//!
//! Acceptance criterion #10: "Tiny custom test component (extension proof)
//! registered + tested per the runtime's extension model (NOT a real shadcn
//! component)."
//!
//! These tests construct the same minimal tree three ways:
//!   1. **Direct runtime calls** — what every binding ultimately funnels into.
//!   2. **Rust ergonomic builder** — `Mkui::new().child(View::new()...)`.
//!   3. **Custom extension** — a `TestWidget` registered through the
//!      runtime's `NodeKind::Custom` extension slot.
//!
//! C and Python parity is exercised via their own crate test suites (each
//! constructs the same shape and emits `snapshot_json()`); this file
//! anchors the comparison structurally and documents the gate.

use mkui_runtime::{
    snapshot::TreeSnapshot, AppTree, ButtonVariant, NodeId, StyleClass, TextVariant,
};
use serde_json::json;

/// The canonical "parity tree" — what every binding builds to produce the
/// reference snapshot.
fn build_parity_tree() -> AppTree {
    let mut tree = AppTree::new();
    let root = tree.root();
    let header = tree
        .push_view(root, "flex items-center justify-between")
        .unwrap();
    tree.push_text(header, "Title", TextVariant::Heading1, "text-2xl font-bold")
        .unwrap();
    let action = tree.actions_mut().register_remote();
    tree.push_button(header, "OK", ButtonVariant::Primary, "p-6", Some(action))
        .unwrap();
    tree
}

#[test]
fn parity_tree_snapshot_is_byte_stable() {
    let a = build_parity_tree();
    let b = build_parity_tree();
    let json_a = TreeSnapshot::of(&a).to_json();
    let json_b = TreeSnapshot::of(&b).to_json();
    assert_eq!(
        json_a, json_b,
        "byte-identical snapshots are the parity-test foundation",
    );
}

#[test]
fn rust_ergonomic_builder_matches_direct_runtime_construction() {
    // Direct runtime build.
    let direct = build_parity_tree();
    let direct_json = TreeSnapshot::of(&direct).to_json();

    // Ergonomic builder (mkui-core) — should lower to the same shape.
    use mkui_core::components::{Button, Mkui, Text, View};
    use mkui_core::headless::{ButtonVariant as CoreButtonVariant, TextVariant as CoreTextVariant};
    let app = Mkui::new().child(
        View::new()
            .class("flex items-center justify-between")
            .child(
                Text::new("Title")
                    .variant(CoreTextVariant::Heading1)
                    .class("text-2xl font-bold"),
            )
            .child(
                Button::new("OK")
                    .variant(CoreButtonVariant::Primary)
                    .class("p-6")
                    .on_press(|| {}),
            ),
    );
    let ergonomic_json = TreeSnapshot::of(app.tree()).to_json();

    // Structural parity: both trees must serialise to the same shape.
    // (The action id of a registered local closure is allocated at the
    // same slot index — generation 0 in both cases — so the byte strings
    // match.)
    assert_eq!(
        direct_json, ergonomic_json,
        "ergonomic builder and direct runtime must produce identical snapshots"
    );
}

/// Tiny custom test component. NOT a real shadcn component (Separator /
/// Tabs / Checkbox / … are Sprint 6+ scope). This exists to satisfy
/// acceptance criterion #10 — prove the `NodeKind::Custom` extension slot
/// works end-to-end.
fn push_test_widget(tree: &mut AppTree, parent: NodeId, label: &str) -> NodeId {
    let props = json!({ "kind": "test_widget", "label": label });
    tree.push_custom(parent, "test_widget", props, "").unwrap()
}

#[test]
fn test_widget_extension_round_trips_through_snapshot() {
    let mut tree = AppTree::new();
    let root = tree.root();
    let id = push_test_widget(&mut tree, root, "I am the extension proof");

    let json = TreeSnapshot::of(&tree).to_json();
    assert!(
        json.contains("\"test_widget\""),
        "type_name must surface in JSON"
    );
    assert!(
        json.contains("I am the extension proof"),
        "props must surface in JSON"
    );

    // The custom node is real — the runtime carries it like any built-in.
    let node = tree.get(id).expect("test_widget node must exist");
    assert!(matches!(node.kind, mkui_runtime::NodeKind::Custom { .. }));
}

#[test]
fn action_id_has_generation_counter() {
    // Codex anti-pattern guard: (index, generation) must travel together so
    // a stale id returns None instead of touching a recycled slot.
    let mut tree = AppTree::new();
    let id1 = tree.actions_mut().register_local(|_| {});
    let id2 = tree.actions_mut().register_local(|_| {});
    assert_ne!(id1, id2);
    // First slot allocated has generation 0.
    assert_eq!(id1.generation(), 0);
    assert_eq!(id2.generation(), 0);
}

#[test]
fn class_parser_rejects_unknown_tier_3_token() {
    let style = StyleClass::from_str("flex unknown-token items-center");
    let err = style.parse().expect_err("T3 token must reject");
    let mkui_runtime::ClassParseError::UnknownToken(t) = err;
    assert_eq!(t, "unknown-token");
}

#[test]
fn fire_action_emits_dirty_and_request_redraw_signals() {
    // Codex round-8 P1 regression test: actions must mark the tree dirty
    // and emit `RequestRedraw`. The web + console dispatch sites used to
    // drop the `RuntimeCtx`, which silently broke the substrate's redraw
    // contract.
    use std::cell::Cell;
    use std::rc::Rc;

    let counter = Rc::new(Cell::new(0u32));
    let counter_in = Rc::clone(&counter);

    let mut tree = AppTree::new();
    let action = tree.actions_mut().register_local(move |ctx| {
        counter_in.set(counter_in.get() + 1);
        ctx.mark_dirty();
    });

    assert!(!tree.is_dirty(), "freshly built tree must not be dirty");
    let mut ctx = tree.actions().fire(action);

    assert_eq!(counter.get(), 1, "closure must fire exactly once");
    assert!(ctx.is_dirty(), "ctx must surface dirty");
    let signals = ctx.drain_emitted();
    assert!(
        signals
            .iter()
            .any(|s| matches!(s, mkui_runtime::RuntimeSignal::RequestRedraw)),
        "ctx must surface RequestRedraw, got: {signals:?}",
    );

    // The dispatch site (web/console high_level) is responsible for
    // routing the ctx back to `tree.mark_dirty()`. Simulate the propagation
    // and assert the bit transitions on the tree itself.
    if ctx.is_dirty() {
        tree.mark_dirty();
    }
    assert!(tree.is_dirty(), "post-propagation, tree's dirty bit is set");
}

#[test]
fn snapshot_format_includes_resolved_field() {
    // Schema-level guard: the JSON shape must contain `resolved` for every
    // node so renderers consume the typed projection, not the raw class
    // string. (Issue #51 §5 — class parser owns the projection.)
    let tree = build_parity_tree();
    let json = TreeSnapshot::of(&tree).to_json();
    assert!(json.contains("\"resolved\""));
    // The Tier 1 tokens used in the parity tree must surface as set fields.
    assert!(json.contains("\"flex\":true"));
    assert!(json.contains("\"items_center\":true"));
}
