//! Allocation-counting gate for the embedded-host per-frame contract (#177).
//!
//! `WgpuApp::update_frame` must be safe to call unconditionally, every
//! frame, from inside a host's own render loop (ADR 0006 §"Update
//! (#177)"). On a frame where nothing changed in the tree, that means
//! literally zero heap allocations — not "fewer", not "amortised", zero.
//!
//! This is its own integration-test binary (not a `#[cfg(test)] mod
//! tests` inside `src/`) because `#[global_allocator]` is process-wide:
//! installing a counting allocator here must not affect `mkui-wgpu`'s
//! other unit tests, which run in a separate test binary.
//!
//! File-level native-target guard (#177 review P2): `WgpuApp::update_frame`
//! is itself `#[cfg(not(target_arch = "wasm32"))]` (wasm32 has no
//! `RedrawRequested`-less host loop to serve), so a `wasm32-unknown-unknown`
//! check of this file errored with three E0599s before this guard existed.

#![cfg(not(target_arch = "wasm32"))]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use mkui_core::components::{Mkui as CoreMkui, Text};
use mkui_runtime::TextVariant;
use mkui_wgpu::{WgpuApp, WgpuRendererRegistry};

/// Counts allocations on the calling thread only. A `const`-initialised
/// `thread_local!` backs onto the fast TLS path (no lazy-init heap
/// allocation on first access), and per-thread counters mean the parallel
/// test harness's other threads never perturb this test's count.
struct CountingAllocator;

thread_local! {
    static ALLOC_COUNT: Cell<u64> = const { Cell::new(0) };
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        // SAFETY: forwards to `System::alloc` unchanged; `layout` is the
        // same value this method received, satisfying `GlobalAlloc`'s
        // contract exactly as `System` itself does.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: forwards to `System::dealloc` unchanged; `ptr`/`layout`
        // are the same values this method received, which the caller
        // guarantees came from a prior call to this allocator's `alloc`.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOC_COUNT.with(|c| c.set(c.get() + 1));
        // SAFETY: forwards to `System::realloc` unchanged, same contract
        // as the `alloc` arm above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn alloc_count() -> u64 {
    ALLOC_COUNT.with(|c| c.get())
}

fn build_app() -> (WgpuApp, mkui_runtime::NodeId) {
    let core = CoreMkui::new().child(Text::new("hp: 100").variant(TextVariant::Body));
    let registry = WgpuRendererRegistry::with_defaults();
    let mut app = WgpuApp::with_app_tree(core, registry);

    // Warm-up: the tree is dirty from construction, so this first
    // `update_frame` is expected to allocate (fresh Scene, hit-test vec,
    // layout vec). It must happen *before* the counted window below.
    app.update_frame();

    let text_id = {
        let core = app.core().unwrap().borrow();
        let root = core.tree().get(core.tree().root()).unwrap();
        root.children[0]
    };
    (app, text_id)
}

#[test]
fn update_frame_allocates_nothing_on_an_unchanged_frame() {
    let (mut app, text_id) = build_app();

    let before = alloc_count();
    for _ in 0..1_000 {
        // A real embedded host writes the same HUD value every frame
        // (health bar, score, …): `set_text` must recognise the write
        // as a no-op and never dirty the tree, so `update_frame` takes
        // its zero-allocation early-return path.
        let changed = app
            .core()
            .unwrap()
            .borrow_mut()
            .tree_mut()
            .set_text(text_id, "hp: 100");
        assert!(!changed, "writing the same value must not be a 'change'");
        assert!(
            !app.update_frame(),
            "a clean tree must not trigger a rebuild"
        );
    }
    let after = alloc_count();

    assert_eq!(
        before,
        after,
        "1000 no-op frames allocated {} time(s); the per-frame path must be allocation-free",
        after - before
    );
}

/// #268's new `AppTree` operations are exercised directly against a bare
/// `AppTree` (no `WgpuApp`/`update_frame` involved) — the counting
/// allocator is process-wide, so it still sees everything `AppTree`
/// itself does; these tests just skip the unrelated scene/layout
/// machinery `build_app`'s warm-up pays for.
fn assert_no_op_allocates_nothing(label: &str, mut call: impl FnMut() -> bool) {
    let before = alloc_count();
    for _ in 0..1_000 {
        assert!(!call(), "{label}: must not report a 'change'");
    }
    let after = alloc_count();
    assert_eq!(
        before,
        after,
        "{label}: 1000 no-op calls allocated {} time(s); the no-op path must be allocation-free",
        after - before
    );
}

#[test]
fn reorder_children_to_the_same_order_allocates_nothing() {
    // #268 acceptance criterion 4: a `reorder_children` call that turns
    // out to be a no-op (the order already matches) must take the same
    // zero-allocation early-return path as `set_text`'s unchanged-value
    // case above.
    let mut tree = mkui_runtime::AppTree::new();
    let root = tree.root();
    tree.push_view(root, "").unwrap();
    tree.push_view(root, "").unwrap();
    let order = tree.get(root).unwrap().children.clone();

    assert_no_op_allocates_nothing("reorder_children", || tree.reorder_children(root, &order));
}

#[test]
fn set_class_with_unchanged_value_allocates_nothing() {
    // #268 acceptance criterion 4: same contract as `set_text` — writing
    // the class a node already has must not allocate.
    let mut tree = mkui_runtime::AppTree::new();
    let view = tree.push_view(tree.root(), "flex items-center").unwrap();

    assert_no_op_allocates_nothing("set_class", || {
        tree.set_class(view, "flex items-center").unwrap()
    });
}

#[test]
fn set_text_props_with_unchanged_values_allocates_nothing() {
    let mut tree = mkui_runtime::AppTree::new();
    let text = tree
        .push_text(tree.root(), "hp: 100", TextVariant::Body, "")
        .unwrap();

    assert_no_op_allocates_nothing("set_text_props", || {
        tree.set_text_props(text, "hp: 100", TextVariant::Body)
    });
}

#[test]
fn set_button_props_with_unchanged_values_allocates_nothing() {
    let mut tree = mkui_runtime::AppTree::new();
    let button = tree
        .push_button(
            tree.root(),
            "Pause",
            mkui_runtime::ButtonVariant::Primary,
            "",
            None,
            false,
        )
        .unwrap();

    assert_no_op_allocates_nothing("set_button_props", || {
        tree.set_button_props(button, "Pause", mkui_runtime::ButtonVariant::Primary, None)
    });
}

#[test]
fn set_custom_props_with_unchanged_value_allocates_nothing() {
    let mut tree = mkui_runtime::AppTree::new();
    let props = serde_json::json!({"count": 1});
    let custom = tree
        .push_custom(tree.root(), "badge", props.clone(), "")
        .unwrap();

    assert_no_op_allocates_nothing("set_custom_props", || tree.set_custom_props(custom, &props));
}

#[test]
fn diff_keyed_on_an_unchanged_keyed_list_allocates_nothing() {
    // #268 acceptance criterion 4, for the diff algorithm itself: a
    // caller re-diffing the same keyed list every frame (nothing
    // inserted/removed/moved/changed) must not pay for the general
    // path's `HashMap`/`HashSet`.
    let old = [
        ("a", mkui_runtime::NodeId::from_raw(0, 0)),
        ("b", mkui_runtime::NodeId::from_raw(1, 0)),
        ("c", mkui_runtime::NodeId::from_raw(2, 0)),
    ];
    let new = ["a", "b", "c"];

    let before = alloc_count();
    for _ in 0..1_000 {
        let ops = mkui_runtime::diff_keyed(&old, &new, |_, _| false);
        assert!(ops.is_empty(), "nothing changed; diff must produce no ops");
    }
    let after = alloc_count();

    assert_eq!(
        before,
        after,
        "1000 no-op diffs allocated {} time(s); the unchanged-list path must be allocation-free",
        after - before
    );
}

#[test]
fn update_frame_allocates_when_the_tree_actually_changes() {
    // Positive control: without this, a broken counter (or a dead-code
    // `update_frame`) would pass the zero-allocation test above for the
    // wrong reason.
    let (mut app, text_id) = build_app();

    let before = alloc_count();
    let changed = app
        .core()
        .unwrap()
        .borrow_mut()
        .tree_mut()
        .set_text(text_id, "hp: 90");
    assert!(changed);
    let rebuilt = app.update_frame();
    let after = alloc_count();

    assert!(rebuilt, "a real content change must trigger a rebuild");
    assert!(
        after > before,
        "a changed frame is expected to allocate (fresh Scene / hit-test / layout vecs)"
    );
}
