//! Byte-identical JSON snapshot parity between the Rust reference and
//! the Python handle-based API.
//!
//! **Gated behind `feature = "parity-test"` AND `not(target_os = "macos")`**
//! because the test binary statically references PyO3 symbols
//! (`_Py_FalseStruct`, etc.) that on macOS only resolve at runtime when
//! loaded by Python. CI Linux images resolve them at link time. On
//! macOS, run the parity check through `maturin develop` + pytest.
//!
//! On Linux CI (Sprint 5 mkui-py re-entry target), enable with
//! `cargo test -p mkui-py --features parity-test`.
#![cfg(all(feature = "parity-test", not(target_os = "macos")))]

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
    tree.push_button(header, "OK", ButtonVariant::Primary, "p-6", Some(action))
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
