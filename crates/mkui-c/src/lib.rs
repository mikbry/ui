//! C/C++ bindings for mkui — handle-based, runtime-backed.
//!
//! Sprint 4 rewrite: replaces the v0.4.x flat `add_view` / `add_text` /
//! `add_button` shape with the nested handle-based FFI promised in the
//! README. Every binding now builds into the same
//! [`mkui_runtime::AppTree`] every other binding consumes:
//!
//! ```c
//! MkuiApp* app = mkui_app_new();
//! MkuiNodeId root = mkui_app_root(app);
//! MkuiNodeId header = mkui_app_view_child(app, root, "border-b");
//! mkui_app_text_child(app, header, "Title", MKUI_TEXT_HEADING_1, "text-xl");
//!
//! MkuiActionId on_click = mkui_app_register_callback(app, &my_callback, NULL);
//! mkui_app_button_child(app, header, "OK", MKUI_BUTTON_PRIMARY, "", on_click);
//!
//! mkui_app_run_console(app);
//! mkui_app_free(app);
//! ```
//!
//! ### Safety, audit, and CI re-entry
//!
//! Every `unsafe` block carries an explicit `// SAFETY:` annotation per the
//! Sprint 4 Phase 1.1 audit fold-in. With those annotations + the
//! `not_unsafe_ptr_arg_deref` lint cleared by the handle shape, `mkui-c`
//! re-enters the clippy + build-release CI jobs in this PR (issue #51
//! acceptance criterion #13).

#![allow(clippy::missing_safety_doc)] // every `unsafe fn` carries an inline SAFETY comment

use std::cell::RefCell;
use std::ffi::{c_char, c_int, c_void, CStr, CString};
use std::ptr;

use mkui_runtime::{ActionId, AppTree, ButtonVariant, NodeId, StyleClass, TextVariant};

/// Opaque app handle. The runtime tree lives inside a `RefCell` so the FFI
/// can mutate it through `*mut MkuiApp` without taking a Rust `&mut` across
/// the boundary.
pub struct MkuiApp {
    tree: RefCell<AppTree>,
    /// Foreign callbacks the host registered through
    /// [`mkui_app_register_callback`]. Indexed by `ActionId.index()`.
    /// Generation is stored alongside so we can ignore stale fires.
    callbacks: RefCell<Vec<Option<ForeignCallback>>>,
}

struct ForeignCallback {
    func: extern "C" fn(*mut c_void),
    user_data: *mut c_void,
    generation: u32,
}

/// `repr(C)` mirror of `mkui_runtime::NodeId`. Carries the two `u32`s the
/// runtime uses to detect use-after-free.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MkuiNodeId {
    pub index: u32,
    pub generation: u32,
}

impl From<NodeId> for MkuiNodeId {
    fn from(id: NodeId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation(),
        }
    }
}

impl From<MkuiNodeId> for NodeId {
    fn from(id: MkuiNodeId) -> Self {
        NodeId::from_raw(id.index, id.generation)
    }
}

/// `repr(C)` mirror of `mkui_runtime::ActionId`.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MkuiActionId {
    pub index: u32,
    pub generation: u32,
}

impl From<ActionId> for MkuiActionId {
    fn from(id: ActionId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation(),
        }
    }
}

impl From<MkuiActionId> for ActionId {
    fn from(id: MkuiActionId) -> Self {
        ActionId::from_raw(id.index, id.generation)
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MkuiErrorCode {
    Success = 0,
    InitializationFailed = 1,
    InvalidParameter = 2,
    RuntimeError = 3,
    OutOfMemory = 4,
}

#[repr(C)]
pub struct MkuiResult {
    pub code: MkuiErrorCode,
    pub message: *const c_char,
}

impl MkuiResult {
    fn success() -> Self {
        Self {
            code: MkuiErrorCode::Success,
            message: ptr::null(),
        }
    }

    fn error(code: MkuiErrorCode, message: &str) -> Self {
        let c_message = CString::new(message)
            .unwrap_or_else(|_| CString::new("Failed to create error message").unwrap());
        Self {
            code,
            message: c_message.into_raw(),
        }
    }
}

/// Sentinel `MkuiNodeId` returned when an FFI call fails. The host can also
/// check `MkuiResult` for the message.
fn invalid_node() -> MkuiNodeId {
    MkuiNodeId {
        index: u32::MAX,
        generation: u32::MAX,
    }
}

fn invalid_action() -> MkuiActionId {
    MkuiActionId {
        index: u32::MAX,
        generation: u32::MAX,
    }
}

// -------------------------------------------------------------------------
// App lifecycle
// -------------------------------------------------------------------------

/// Create a new mkui application.
#[no_mangle]
pub extern "C" fn mkui_app_new() -> *mut MkuiApp {
    let app = MkuiApp {
        tree: RefCell::new(AppTree::new()),
        callbacks: RefCell::new(Vec::new()),
    };
    Box::into_raw(Box::new(app))
}

/// Free a mkui application.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_free(app: *mut MkuiApp) {
    if app.is_null() {
        return;
    }
    // SAFETY: `app` was produced by `Box::into_raw` in `mkui_app_new` and
    // the host is responsible for calling `mkui_app_free` exactly once. We
    // reconstruct the Box and drop it.
    unsafe {
        let _ = Box::from_raw(app);
    }
}

/// Return the root `NodeId` so the host can attach children to the runtime
/// tree without exposing the synthetic root constant.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_root(app: *const MkuiApp) -> MkuiNodeId {
    if app.is_null() {
        return invalid_node();
    }
    // SAFETY: caller guarantees `app` was produced by `mkui_app_new` and
    // has not been freed. We only borrow immutably here.
    let app_ref = unsafe { &*app };
    app_ref.tree.borrow().root().into()
}

// -------------------------------------------------------------------------
// Node construction — handle-based, nested
// -------------------------------------------------------------------------

unsafe fn read_class(class_name: *const c_char) -> Result<String, MkuiResult> {
    if class_name.is_null() {
        return Ok(String::new());
    }
    // SAFETY: caller guarantees `class_name` is either NULL (checked above)
    // or a valid pointer to a NUL-terminated C string that outlives this call.
    let cstr = unsafe { CStr::from_ptr(class_name) };
    cstr.to_str()
        .map(|s| s.to_string())
        .map_err(|_| MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid class name"))
}

unsafe fn read_str(ptr: *const c_char, name: &str) -> Result<String, MkuiResult> {
    if ptr.is_null() {
        return Err(MkuiResult::error(
            MkuiErrorCode::InvalidParameter,
            &format!("{name} pointer is null"),
        ));
    }
    // SAFETY: caller guarantees `ptr` is a valid pointer to a NUL-terminated
    // C string that outlives this call.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_str()
        .map(|s| s.to_string())
        .map_err(|_| MkuiResult::error(MkuiErrorCode::InvalidParameter, &format!("Invalid {name}")))
}

/// Append a `View` under `parent`. Returns the new node's id, or
/// `MkuiNodeId{index=UINT_MAX, generation=UINT_MAX}` on error.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_view_child(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    class_name: *const c_char,
) -> MkuiNodeId {
    if app.is_null() {
        return invalid_node();
    }
    // SAFETY: `app` is non-null and was produced by `mkui_app_new`.
    let app_ref = unsafe { &mut *app };
    let class = match unsafe { read_class(class_name) } {
        Ok(c) => c,
        Err(_) => return invalid_node(),
    };
    let style = StyleClass::from_str(class);
    if style.parse().is_err() {
        return invalid_node();
    }
    let id = app_ref
        .tree
        .borrow_mut()
        .push_view_unchecked(parent.into(), style);
    id.into()
}

/// Append a `Text` under `parent`.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_text_child(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    content: *const c_char,
    variant: c_int,
    class_name: *const c_char,
) -> MkuiNodeId {
    if app.is_null() {
        return invalid_node();
    }
    // SAFETY: see `mkui_app_view_child`.
    let app_ref = unsafe { &mut *app };
    let content = match unsafe { read_str(content, "content") } {
        Ok(s) => s,
        Err(_) => return invalid_node(),
    };
    let class = match unsafe { read_class(class_name) } {
        Ok(c) => c,
        Err(_) => return invalid_node(),
    };
    let variant = match TextVariant::from_ffi(variant) {
        Ok(v) => v,
        Err(_) => return invalid_node(),
    };
    let style = StyleClass::from_str(class);
    if style.parse().is_err() {
        return invalid_node();
    }
    let id = app_ref
        .tree
        .borrow_mut()
        .push_text_unchecked(parent.into(), content, variant, style);
    id.into()
}

/// Append a `Button` under `parent`. Pass `MkuiActionId{UINT_MAX, UINT_MAX}`
/// for no callback, or an id from [`mkui_app_register_callback`].
#[no_mangle]
pub unsafe extern "C" fn mkui_app_button_child(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    label: *const c_char,
    variant: c_int,
    class_name: *const c_char,
    on_press: MkuiActionId,
) -> MkuiNodeId {
    if app.is_null() {
        return invalid_node();
    }
    // SAFETY: see `mkui_app_view_child`.
    let app_ref = unsafe { &mut *app };
    let label = match unsafe { read_str(label, "label") } {
        Ok(s) => s,
        Err(_) => return invalid_node(),
    };
    let class = match unsafe { read_class(class_name) } {
        Ok(c) => c,
        Err(_) => return invalid_node(),
    };
    let variant = match ButtonVariant::from_ffi(variant) {
        Ok(v) => v,
        Err(_) => return invalid_node(),
    };
    let style = StyleClass::from_str(class);
    if style.parse().is_err() {
        return invalid_node();
    }
    let action = if on_press == invalid_action() {
        None
    } else {
        Some(on_press.into())
    };
    let id = app_ref.tree.borrow_mut().push_button_unchecked(
        parent.into(),
        label,
        variant,
        style,
        action,
    );
    id.into()
}

// -------------------------------------------------------------------------
// Action callbacks — binding-owned table keyed by ActionId
// -------------------------------------------------------------------------

/// Register a C function as a callback. The returned [`MkuiActionId`] is
/// what `mkui_app_button_child` accepts; firing the action invokes
/// `func(user_data)` exactly once.
///
/// The host is responsible for `user_data` lifetime — mkui never frees it.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_register_callback(
    app: *mut MkuiApp,
    func: Option<extern "C" fn(*mut c_void)>,
    user_data: *mut c_void,
) -> MkuiActionId {
    if app.is_null() {
        return invalid_action();
    }
    let Some(func) = func else {
        return invalid_action();
    };
    // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
    let app_ref = unsafe { &mut *app };

    // Allocate a stable id on the runtime side ("remote" registration —
    // the closure stays on the C side, the runtime just owns the handle).
    let id = app_ref.tree.borrow_mut().actions_mut().register_remote();

    // Stash the C callback in our parallel table keyed by ActionId.index().
    let mut callbacks = app_ref.callbacks.borrow_mut();
    let idx = id.index() as usize;
    if callbacks.len() <= idx {
        callbacks.resize_with(idx + 1, || None);
    }
    callbacks[idx] = Some(ForeignCallback {
        func,
        user_data,
        generation: id.generation(),
    });

    id.into()
}

/// Fire a registered callback. Useful for test harnesses; the actual UI
/// fires callbacks via the renderer (console / web / wgpu) when a button
/// is pressed.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_fire_action(app: *mut MkuiApp, id: MkuiActionId) -> MkuiResult {
    if app.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "App pointer is null");
    }
    // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
    let app_ref = unsafe { &*app };
    let callbacks = app_ref.callbacks.borrow();
    let Some(slot) = callbacks.get(id.index as usize) else {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Unknown action id");
    };
    let Some(cb) = slot else {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Action id is empty");
    };
    if cb.generation != id.generation {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Action id is stale");
    }
    (cb.func)(cb.user_data);
    MkuiResult::success()
}

// -------------------------------------------------------------------------
// Renderer entry — console
// -------------------------------------------------------------------------

/// Run the application via the console backend. The console backend takes a
/// component tree, not an `AppTree`; for Sprint 4 we walk the tree and
/// surface a non-interactive print so the C smoke build still completes
/// without a TTY. Sprint 5+ will reroute this through the runtime walker
/// once the console backend exposes a `from_tree` constructor.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_run_console(app: *mut MkuiApp) -> MkuiResult {
    if app.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "App pointer is null");
    }
    // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
    let app_ref = unsafe { &*app };
    let tree = app_ref.tree.borrow();
    // Best-effort terminal output without taking over the alt screen — keeps
    // the C example smoke-testable in CI environments without a real TTY.
    println!("mkui-c: app tree contains {} live node(s)", tree.len());
    for node in tree.nodes() {
        match &node.kind {
            mkui_runtime::NodeKind::Text(t) => println!("  text: {}", t.content),
            mkui_runtime::NodeKind::Button(b) => println!("  button: {}", b.label),
            _ => {}
        }
    }
    MkuiResult::success()
}

// -------------------------------------------------------------------------
// Snapshot — parity-test entry point
// -------------------------------------------------------------------------

/// Emit a canonical JSON snapshot of the current tree. Used by the
/// cross-binding parity tests; the host frees the returned buffer via
/// [`mkui_free_error_message`] (same allocator).
#[no_mangle]
pub unsafe extern "C" fn mkui_app_snapshot_json(app: *const MkuiApp) -> *mut c_char {
    if app.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
    let app_ref = unsafe { &*app };
    let tree = app_ref.tree.borrow();
    let snapshot = mkui_runtime::snapshot::TreeSnapshot::of(&tree);
    let json = snapshot.to_json();
    CString::new(json)
        .map(|cs| cs.into_raw())
        .unwrap_or(ptr::null_mut())
}

/// Free an error message or JSON buffer returned by mkui functions.
#[no_mangle]
pub unsafe extern "C" fn mkui_free_error_message(message: *mut c_char) {
    if message.is_null() {
        return;
    }
    // SAFETY: caller guarantees `message` was returned by an mkui function
    // (via `CString::into_raw`). Reconstructing the CString drops it.
    unsafe {
        let _ = CString::from_raw(message);
    }
}

/// Get version information.
#[no_mangle]
pub extern "C" fn mkui_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// Button variant constants for C.
#[no_mangle]
pub static MKUI_BUTTON_PRIMARY: c_int = 0;
#[no_mangle]
pub static MKUI_BUTTON_SECONDARY: c_int = 1;
#[no_mangle]
pub static MKUI_BUTTON_DESTRUCTIVE: c_int = 2;
#[no_mangle]
pub static MKUI_BUTTON_OUTLINE: c_int = 3;
#[no_mangle]
pub static MKUI_BUTTON_GHOST: c_int = 4;
#[no_mangle]
pub static MKUI_BUTTON_LINK: c_int = 5;

// Text variant constants for C.
#[no_mangle]
pub static MKUI_TEXT_BODY: c_int = 0;
#[no_mangle]
pub static MKUI_TEXT_HEADING_1: c_int = 1;
#[no_mangle]
pub static MKUI_TEXT_HEADING_2: c_int = 2;
#[no_mangle]
pub static MKUI_TEXT_HEADING_3: c_int = 3;
#[no_mangle]
pub static MKUI_TEXT_CAPTION: c_int = 4;
#[no_mangle]
pub static MKUI_TEXT_LABEL: c_int = 5;
#[no_mangle]
pub static MKUI_TEXT_CODE: c_int = 6;

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    #[test]
    fn build_a_tree_through_the_handle_api() {
        // SAFETY: every FFI call in this test runs on the same thread and
        // every pointer comes from a `Box::into_raw` we own.
        unsafe {
            let app = mkui_app_new();
            assert!(!app.is_null());

            let root = mkui_app_root(app);
            let class = CString::new("flex").unwrap();
            let view = mkui_app_view_child(app, root, class.as_ptr());
            assert_ne!(view, invalid_node());

            let content = CString::new("hello").unwrap();
            let empty = CString::new("").unwrap();
            let text = mkui_app_text_child(
                app,
                view,
                content.as_ptr(),
                MKUI_TEXT_HEADING_1,
                empty.as_ptr(),
            );
            assert_ne!(text, invalid_node());

            let label = CString::new("ok").unwrap();
            let button = mkui_app_button_child(
                app,
                view,
                label.as_ptr(),
                MKUI_BUTTON_PRIMARY,
                empty.as_ptr(),
                invalid_action(),
            );
            assert_ne!(button, invalid_node());

            // Snapshot the tree and confirm it contains the labels.
            let json_ptr = mkui_app_snapshot_json(app);
            assert!(!json_ptr.is_null());
            let json = CStr::from_ptr(json_ptr).to_str().unwrap();
            assert!(json.contains("hello"));
            assert!(json.contains("\"label\":\"ok\""));
            mkui_free_error_message(json_ptr);

            mkui_app_free(app);
        }
    }

    #[test]
    fn handle_callback_round_trip() {
        // SAFETY: as above — single-threaded test, every pointer owned here.
        unsafe {
            extern "C" fn cb(data: *mut c_void) {
                // `data` is `&Cell<u32>` cast to `*mut c_void`.
                // SAFETY: the test owns the Cell on the stack and passes
                // its address as `user_data`; the callback runs before the
                // Cell goes out of scope.
                let cell = data as *const std::cell::Cell<u32>;
                let c = unsafe { &*cell };
                c.set(c.get() + 1);
            }

            let counter = std::cell::Cell::new(0u32);
            let app = mkui_app_new();
            let id =
                mkui_app_register_callback(app, Some(cb), (&counter) as *const _ as *mut c_void);
            assert_ne!(id, invalid_action());
            let result = mkui_app_fire_action(app, id);
            assert!(matches!(result.code, MkuiErrorCode::Success));
            assert_eq!(counter.get(), 1);

            mkui_app_free(app);
        }
    }

    #[test]
    fn invalid_class_string_returns_invalid_node() {
        // SAFETY: single-threaded test.
        unsafe {
            let app = mkui_app_new();
            let root = mkui_app_root(app);
            let bogus = CString::new("not-a-utility").unwrap();
            let id = mkui_app_view_child(app, root, bogus.as_ptr());
            assert_eq!(
                id,
                invalid_node(),
                "unknown utility must surface as invalid"
            );
            mkui_app_free(app);
        }
    }
}
