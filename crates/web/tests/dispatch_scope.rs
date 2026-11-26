#![cfg(target_arch = "wasm32")]
//! Browser regression test for #305.
//!
//! `mkui-web` used to resolve every button's action through a single
//! thread-local `ACTIVE_TREE` slot, set by whichever `Mkui::run()` mounted
//! most recently. Two trees mounted in the same document — each a fresh
//! `ActionRegistry`, so each one's first registered action gets the same
//! raw `(index, generation)` — would route a click on the *first* tree's
//! button through the *second* tree's registry once it mounted. This test
//! mounts two such trees and clicks both buttons, confirming each only
//! ever updates its own state.
//!
//! Runs against the DOM, so it needs a real browser:
//! `wasm-pack test --headless --chrome crates/web` (or `--firefox`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mkui_core::components::{Button, Mkui as CoreMkui};
use mkui_runtime::{ActionId, NodeKind};
use mkui_web::render::{render_tree, WebRendererRegistry};
use mkui_web::utils::document;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

/// Build a tree with a single button that bumps `counter` on press, render
/// it into a wrapper appended to `<body>`, and return the button's
/// `on_press` id plus the actual DOM button element the test can `.click()`.
fn mount_counter_tree(counter: Rc<Cell<u32>>) -> (ActionId, web_sys::HtmlElement) {
    let core = CoreMkui::new().child(Button::new("bump").on_press(move || {
        counter.set(counter.get() + 1);
    }));
    let (tree, _unused_registry) = core.into_parts();
    let tree_rc = Rc::new(RefCell::new(tree));

    let action_id = {
        let tree_ref = tree_rc.borrow();
        let root = tree_ref.get(tree_ref.root()).expect("root node");
        let button_id = root.children[0];
        match &tree_ref.get(button_id).expect("button node").kind {
            NodeKind::Button(b) => b.on_press.expect("button registered an action"),
            other => panic!("expected a Button node, got {other:?}"),
        }
    };

    let doc = document();
    let registry = WebRendererRegistry::with_defaults();
    let wrapper = {
        let tree_ref = tree_rc.borrow();
        render_tree(&tree_ref, &doc, &registry, "", &tree_rc).expect("render_tree")
    };
    doc.body()
        .expect("document body")
        .append_child(&wrapper)
        .expect("append wrapper");

    let button = wrapper
        .query_selector("button")
        .expect("query_selector")
        .expect("a button element")
        .dyn_into::<web_sys::HtmlElement>()
        .expect("button is an HtmlElement");

    (action_id, button)
}

#[wasm_bindgen_test]
fn two_mounts_with_colliding_raw_action_ids_dispatch_independently() {
    let counter_a = Rc::new(Cell::new(0u32));
    let counter_b = Rc::new(Cell::new(0u32));

    let (action_a, button_a) = mount_counter_tree(Rc::clone(&counter_a));
    let (action_b, button_b) = mount_counter_tree(Rc::clone(&counter_b));

    // Precondition: both trees are fresh `ActionRegistry`s, so the first
    // action registered in each gets the exact same raw id — the collision
    // #305 describes, not a contrived one.
    assert_eq!(action_a.index(), action_b.index());
    assert_eq!(action_a.generation(), action_b.generation());

    button_a.click();
    assert_eq!(
        counter_a.get(),
        1,
        "clicking A's button must fire A's action"
    );
    assert_eq!(
        counter_b.get(),
        0,
        "clicking A's button must not fire B's action"
    );

    button_b.click();
    assert_eq!(counter_a.get(), 1);
    assert_eq!(
        counter_b.get(),
        1,
        "clicking B's button must fire B's action"
    );

    button_a.click();
    assert_eq!(
        counter_a.get(),
        2,
        "A's button still resolves against A's own tree after B mounted"
    );
    assert_eq!(counter_b.get(), 1);
}
