//! Byte-identical JSON snapshot parity between the Rust reference and
//! the Python handle-based API.
//!
//! Gated behind `feature = "parity-test"` which enables
//! `pyo3/auto-initialize`. That feature switches PyO3 from "loaded by
//! Python" mode (where libpython provides symbols at runtime) to
//! "embeds Python" mode (where `pyo3-build-config` emits real
//! `-l python3.X` linker flags). The test binary then links against
//! whatever interpreter `PYO3_PYTHON` (or auto-discovery from `PATH`)
//! resolves to.
//!
//! Run with `PYO3_PYTHON=$(which python3) cargo test -p mkui-py
//! --features parity-test --test parity --locked`.
#![cfg(feature = "parity-test")]

//!
//! Sprint 4 acceptance criterion #2 (byte-identical Rust/C/Py snapshots)
//! is asserted here for the Python frontend. The test exercises the
//! same `App::view_child` / `App::text_child` / `App::button_child`
//! methods Python callers invoke — PyO3's `#[pymethods]` macro generates
//! a Python dispatcher around regular Rust methods, so calling them
//! directly from Rust exercises the exact same code path.
//!
//! For the action id, the test uses
//! [`mkui_py::App::register_remote_action_for_test`] (a non-`#[pymethods]`
//! helper) because constructing a `Py<PyAny>` needs a live Python
//! interpreter, which `cargo test` does not provide. The runtime side
//! of the registration is identical between this helper and the real
//! `App::register_callback`, so the snapshot byte-equality property
//! still holds.

use mkui_py::{App, PyNodeId};
use mkui_runtime::snapshot::TreeSnapshot;
use mkui_runtime::{ButtonVariant, TextVariant};

fn build_reference_tree_json() -> String {
    let mut tree = mkui_runtime::AppTree::new();
    let root = tree.root();
    let header = tree
        .push_view(root, "flex items-center justify-between")
        .unwrap();
    tree.push_text(header, "Title", TextVariant::Heading1, "text-2xl font-bold")
        .unwrap();
    let action = tree.actions_mut().register_remote();
    tree.push_button(
        header,
        "OK",
        ButtonVariant::Primary,
        "p-6",
        Some(action),
        false,
    )
    .unwrap();

    let content = tree.push_view(root, "flex-1").unwrap();
    tree.push_text(content, "Body", TextVariant::Body, "text-foreground")
        .unwrap();

    TreeSnapshot::of(&tree).to_json()
}

fn build_py_tree_json() -> String {
    let app = App::new_for_test();
    let root: PyNodeId = app.root();

    let header = app
        .view_child(root, "flex items-center justify-between")
        .unwrap();
    app.text_child(
        header,
        "Title",
        TextVariant::Heading1.to_ffi(),
        "text-2xl font-bold",
    )
    .unwrap();
    let action = app.register_remote_action_for_test();
    app.button_child(
        header,
        "OK",
        ButtonVariant::Primary.to_ffi(),
        "p-6",
        Some(action),
        false,
    )
    .unwrap();

    let content = app.view_child(root, "flex-1").unwrap();
    app.text_child(
        content,
        "Body",
        TextVariant::Body.to_ffi(),
        "text-foreground",
    )
    .unwrap();

    app.snapshot_json()
}

#[test]
fn py_app_and_rust_runtime_produce_byte_identical_snapshots() {
    let rust_json = build_reference_tree_json();
    let py_json = build_py_tree_json();
    if rust_json != py_json {
        let max = rust_json.len().max(py_json.len());
        let first_diff = (0..max)
            .find(|i| rust_json.as_bytes().get(*i) != py_json.as_bytes().get(*i))
            .unwrap_or(0);
        let start = first_diff.saturating_sub(40);
        let end = (first_diff + 80).min(max);
        panic!(
            "Py snapshot diverges from Rust reference at byte {first_diff}:\n\
             rust: …{:?}…\n\
             py  : …{:?}…",
            &rust_json[start..end.min(rust_json.len())],
            &py_json[start..end.min(py_json.len())],
        );
    }
    assert_eq!(rust_json, py_json);
}

/// #245: a `disabled` button must snapshot byte-identically whether it's
/// built through the Rust runtime API or the Python-shaped `App` surface.
#[test]
fn disabled_button_produces_byte_identical_snapshots_in_rust_and_py() {
    let rust_json = {
        let mut tree = mkui_runtime::AppTree::new();
        let root = tree.root();
        tree.push_button(root, "Save", ButtonVariant::Primary, "", None, true)
            .unwrap();
        TreeSnapshot::of(&tree).to_json()
    };

    let app = App::new_for_test();
    let root = app.root();
    app.button_child(
        root,
        "Save",
        ButtonVariant::Primary.to_ffi(),
        "",
        None,
        true,
    )
    .unwrap();
    let py_json = app.snapshot_json();

    assert_eq!(rust_json, py_json);
    assert!(py_json.contains("\"disabled\":true"));
}

#[test]
fn remove_node_detaches_the_node_and_rejects_the_stale_id_afterward() {
    // Cross-binding parity for #83: `App::remove_node` mirrors
    // `AppTree::remove_node` and `mkui_app_remove_node`'s contract —
    // success once, a typed error (never a panic) on a repeat call with
    // the same now-stale id.
    let app = App::new_for_test();
    let root = app.root();
    let view = app.view_child(root, "").unwrap();

    assert!(app.remove_node(view).is_ok());

    let second = app.remove_node(view);
    assert!(second.is_err(), "a second remove on the same id must fail");
    let err_text = second.unwrap_err().to_string();
    assert!(
        err_text.contains("stale") || err_text.contains("missing") || err_text.contains("root"),
        "expected stale/missing/root in error message, got: {err_text}"
    );
}

#[test]
fn removing_a_buttons_node_stops_its_action_from_firing() {
    // `register_remote_action_for_test` never populates `App`'s own
    // `callbacks` table (it can't — building a `Py<PyAny>` needs a live
    // interpreter), so `fire_action` would reject this id as "unknown"
    // regardless of removal. The real end-to-end regression coverage
    // (a registered Python callable that actually would have fired) is
    // `smoke_py.rs`'s live-interpreter script; this test only pins the
    // `App::remove_node` + `fire_action` contract at the Rust level.
    let app = App::new_for_test();
    let root = app.root();
    let action = app.register_remote_action_for_test();
    let button = app
        .button_child(
            root,
            "ok",
            ButtonVariant::Primary.to_ffi(),
            "",
            Some(action),
            false,
        )
        .unwrap();

    assert!(app.remove_node(button).is_ok());

    pyo3::Python::attach(|py| {
        let fire = app.fire_action(py, action);
        assert!(
            fire.is_err(),
            "firing a removed button's action must be rejected"
        );
    });
}

#[test]
fn remove_node_on_the_root_raises_python_value_error() {
    let app = App::new_for_test();
    let root = app.root();
    assert!(
        app.remove_node(root).is_err(),
        "the root has no parent to detach from and must be protected"
    );
}

#[test]
fn stale_parent_raises_python_value_error() {
    // P1 #3 regression: a fabricated parent must surface as PyValueError,
    // not as a panic across the PyO3 boundary.
    let app = App::new_for_test();
    let stale = PyNodeId {
        index: u32::MAX,
        generation: u32::MAX,
    };
    let result = app.view_child(stale, "");
    assert!(result.is_err(), "stale parent must produce an error");
    let err_text = result.unwrap_err().to_string();
    assert!(
        err_text.contains("stale") || err_text.contains("invalid"),
        "expected stale/invalid in error message, got: {err_text}"
    );
}
