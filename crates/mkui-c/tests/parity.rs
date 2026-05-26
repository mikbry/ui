//! Byte-identical JSON snapshot parity between the Rust ergonomic
//! builder and the C handle-based FFI.
//!
//! This is the load-bearing test for Sprint 4 acceptance criterion #2:
//! "Rust builder and C handle APIs produce byte-identical JSON snapshots
//! for equivalent tree constructions."
//!
//! The Codex round-7 PR shipped only a `contains(...)` substring check
//! on the C side, which is **not** byte-equality. Round-8 P1 fix: build
//! the same non-trivial tree two ways, compare the canonical JSON in
//! full.
//!
//! ## Why the snapshots are byte-equal
//!
//! - Both frontends ultimately mutate the same `mkui_runtime::AppTree`
//!   via the same `push_*_unchecked` helpers, in the same order.
//! - `ActionId` allocation is order-dependent. Both frontends register
//!   their action *before* the button that references it, so the id is
//!   `(index=0, generation=0)` in both cases.
//! - `TreeSnapshot::of(...).to_json()` uses serde's derived `Serialize`
//!   impl: field order is declaration order (not BTreeMap-sorted), and
//!   `serde_json::Map` is `BTreeMap`-backed by default so nested JSON
//!   object keys are sorted deterministically.

use std::ffi::{CStr, CString};
use std::os::raw::c_void;
use std::ptr;

use mkui_c::*;
use mkui_runtime::snapshot::TreeSnapshot;
use mkui_runtime::{ButtonVariant, TextVariant};

/// Build the canonical reference tree directly through the runtime API.
/// This is what every binding should produce when the same logical
/// construction is mirrored through its public surface.
fn build_reference_tree_json() -> String {
    let mut tree = mkui_runtime::AppTree::new();
    let root = tree.root();
    let header = tree
        .push_view(root, "flex items-center justify-between")
        .unwrap();
    tree.push_text(header, "Title", TextVariant::Heading1, "text-2xl font-bold")
        .unwrap();
    // Register the action **before** the button so its id is (0, 0) — the
    // FFI side does the same thing, so the snapshotted ids match.
    let action = tree.actions_mut().register_remote();
    tree.push_button(header, "OK", ButtonVariant::Primary, "p-6", Some(action))
        .unwrap();

    let content = tree.push_view(root, "flex-1").unwrap();
    tree.push_text(content, "Body", TextVariant::Body, "text-foreground")
        .unwrap();

    TreeSnapshot::of(&tree).to_json()
}

extern "C" fn noop_callback(_user_data: *mut c_void) {}

/// Build the same tree via the public C FFI surface and return the JSON
/// snapshot.
unsafe fn build_c_tree_json() -> String {
    let app = mkui_app_new();
    assert!(!app.is_null(), "mkui_app_new must succeed");
    let root = mkui_app_root(app);

    let header_class = CString::new("flex items-center justify-between").unwrap();
    let header = mkui_app_view_child(app, root, header_class.as_ptr());

    let title_content = CString::new("Title").unwrap();
    let title_class = CString::new("text-2xl font-bold").unwrap();
    mkui_app_text_child(
        app,
        header,
        title_content.as_ptr(),
        MKUI_TEXT_HEADING_1,
        title_class.as_ptr(),
    );

    // Register the action before constructing the button — matches the
    // reference tree's ordering so the resulting `ActionId` is `(0, 0)`.
    let action = mkui_app_register_callback(app, Some(noop_callback), ptr::null_mut());

    let ok_label = CString::new("OK").unwrap();
    let ok_class = CString::new("p-6").unwrap();
    mkui_app_button_child(
        app,
        header,
        ok_label.as_ptr(),
        MKUI_BUTTON_PRIMARY,
        ok_class.as_ptr(),
        action,
    );

    let content_class = CString::new("flex-1").unwrap();
    let content = mkui_app_view_child(app, root, content_class.as_ptr());

    let body_content = CString::new("Body").unwrap();
    let body_class = CString::new("text-foreground").unwrap();
    mkui_app_text_child(
        app,
        content,
        body_content.as_ptr(),
        MKUI_TEXT_BODY,
        body_class.as_ptr(),
    );

    let json_ptr = mkui_app_snapshot_json(app);
    assert!(!json_ptr.is_null(), "snapshot must succeed");
    let json = CStr::from_ptr(json_ptr).to_str().unwrap().to_string();
    mkui_free_error_message(json_ptr);
    mkui_app_free(app);
    json
}

#[test]
fn c_ffi_and_rust_runtime_produce_byte_identical_snapshots() {
    let rust_json = build_reference_tree_json();
    // SAFETY: every FFI call below runs on this thread and every pointer
    // comes from a `Box::into_raw` we own or a `CString` that outlives the
    // call.
    let c_json = unsafe { build_c_tree_json() };

    // Byte-equality is the load-bearing assertion. `contains(...)` was the
    // Sprint 4 round-7 shortcut Codex round-8 P1 flagged; round-8 fix is
    // this full-string comparison.
    if rust_json != c_json {
        let max = rust_json.len().max(c_json.len());
        let first_diff = (0..max)
            .find(|i| rust_json.as_bytes().get(*i) != c_json.as_bytes().get(*i))
            .unwrap_or(0);
        let start = first_diff.saturating_sub(40);
        let end = (first_diff + 80).min(max);
        panic!(
            "C FFI snapshot diverges from Rust reference at byte {first_diff}:\n\
             rust: …{:?}…\n\
             c   : …{:?}…\n\
             (full rust len={}, c len={})",
            &rust_json[start..end.min(rust_json.len())],
            &c_json[start..end.min(c_json.len())],
            rust_json.len(),
            c_json.len(),
        );
    }
    assert_eq!(rust_json, c_json);
}

#[test]
fn snapshot_is_deterministic_across_repeated_construction() {
    // Sanity: building the same tree twice must produce the same JSON.
    // Catches non-determinism creeping in (HashMap iteration, timestamp
    // injection, etc.) before it shows up as a parity-test flake.
    let a = build_reference_tree_json();
    let b = build_reference_tree_json();
    assert_eq!(a, b);

    // SAFETY: each call constructs and frees its own MkuiApp.
    let c1 = unsafe { build_c_tree_json() };
    let c2 = unsafe { build_c_tree_json() };
    assert_eq!(c1, c2);
}
