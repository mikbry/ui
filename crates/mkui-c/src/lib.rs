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
use std::panic::AssertUnwindSafe;
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

/// Status codes returned through [`MkuiResult::code`].
///
/// Variant names are prefixed with `Mkui` in Rust so cbindgen's
/// ScreamingSnakeCase rename emits them as the v0.4.x-stable
/// `MKUI_SUCCESS` / `MKUI_INVALID_PARAMETER` / … C-header forms.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MkuiErrorCode {
    MkuiSuccess = 0,
    MkuiInitializationFailed = 1,
    MkuiInvalidParameter = 2,
    MkuiRuntimeError = 3,
    MkuiOutOfMemory = 4,
}

#[repr(C)]
pub struct MkuiResult {
    pub code: MkuiErrorCode,
    pub message: *const c_char,
}

impl MkuiResult {
    fn success() -> Self {
        Self {
            code: MkuiErrorCode::MkuiSuccess,
            message: ptr::null(),
        }
    }

    fn error(code: MkuiErrorCode, message: &str) -> Self {
        // `CString::new` only fails on an interior NUL; fall back to an empty
        // C string rather than the prior nested `CString::new(...).unwrap()`,
        // which carried its own (unreachable, but real) panic path. Ordinary
        // error construction now has no panic path from the `CString`
        // fallback.
        let c_message = CString::new(message).unwrap_or_default();
        Self {
            code,
            message: c_message.into_raw(),
        }
    }
}

/// Allocation-free `MkuiResult` returned from the panic-recovery path of an
/// `extern "C"` wrapper. Unlike [`MkuiResult::error`] it never calls
/// `CString::new`/`into_raw`, so it cannot itself allocate or fail while we
/// are already unwinding out of a caught panic.
fn panic_result() -> MkuiResult {
    MkuiResult {
        code: MkuiErrorCode::MkuiRuntimeError,
        message: ptr::null(),
    }
}

/// Run an exported FFI body under [`std::panic::catch_unwind`] so a Rust
/// panic can never unwind across the `extern "C"` boundary (which is
/// undefined behaviour). On a caught panic we emit best-effort static-message
/// logging — itself contained in a nested `catch_unwind` so a logging failure
/// cannot escape — and return the type-appropriate, allocation-free
/// `fallback` value.
///
/// This protects against Rust panics using the unwind runtime inside mkui's
/// own Rust body. It does **not** catch process aborts, foreign-language
/// exceptions, or a panic/exception crossing an `extern "C"` callback
/// boundary; those remain outside `catch_unwind`'s guarantee.
fn ffi_guard<T>(fallback: T, body: impl FnOnce() -> T) -> T {
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        // `#[cfg(test)]`-only injection hook — absent from release/public
        // builds, so it adds no exported symbol and no runtime cost there.
        #[cfg(test)]
        test_panic::maybe_inject();
        body()
    }));
    match result {
        Ok(value) => value,
        Err(_) => {
            log_ffi_panic();
            fallback
        }
    }
}

/// Best-effort logging from the panic-recovery path. Static message only —
/// the panic payload is never formatted into an allocation here — and the
/// whole call is wrapped in a nested `catch_unwind` so a failure in the
/// logging path itself cannot unwind across the C ABI.
fn log_ffi_panic() {
    let _ = std::panic::catch_unwind(|| {
        eprintln!("mkui-c: caught a Rust panic at the FFI boundary; returning fallback value");
    });
}

/// `#[cfg(test)]`-only panic-injection hook. Lets the panic-safety tests
/// trigger a synthetic Rust panic *inside* a wrapper (after entry, before
/// normal return) without a Cargo feature or any release-build footprint.
#[cfg(test)]
mod test_panic {
    use std::cell::Cell;

    thread_local! {
        static ARMED: Cell<bool> = const { Cell::new(false) };
    }

    /// Arm the next `maybe_inject` call on this thread to panic once.
    pub(super) fn arm() {
        ARMED.with(|c| c.set(true));
    }

    /// Panic once if armed, disarming first so the recovery path cannot
    /// re-enter the injection on this thread.
    pub(super) fn maybe_inject() {
        if ARMED.with(|c| c.replace(false)) {
            panic!("injected synthetic FFI panic (test only)");
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
    ffi_guard(ptr::null_mut(), || {
        let app = MkuiApp {
            tree: RefCell::new(AppTree::new()),
            callbacks: RefCell::new(Vec::new()),
        };
        Box::into_raw(Box::new(app))
    })
}

/// Free a mkui application.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_free(app: *mut MkuiApp) {
    ffi_guard((), || {
        if app.is_null() {
            return;
        }
        // SAFETY: `app` was produced by `Box::into_raw` in `mkui_app_new` and
        // the host is responsible for calling `mkui_app_free` exactly once. We
        // reconstruct the Box and drop it.
        unsafe {
            let _ = Box::from_raw(app);
        }
    })
}

/// Return the root `NodeId` so the host can attach children to the runtime
/// tree without exposing the synthetic root constant.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_root(app: *const MkuiApp) -> MkuiNodeId {
    ffi_guard(invalid_node(), || {
        if app.is_null() {
            return invalid_node();
        }
        // SAFETY: caller guarantees `app` was produced by `mkui_app_new` and
        // has not been freed. We only borrow immutably here.
        let app_ref = unsafe { &*app };
        app_ref.tree.borrow().root().into()
    })
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
        .map_err(|_| MkuiResult::error(MkuiErrorCode::MkuiInvalidParameter, "Invalid class name"))
}

unsafe fn read_str(ptr: *const c_char, name: &str) -> Result<String, MkuiResult> {
    if ptr.is_null() {
        return Err(MkuiResult::error(
            MkuiErrorCode::MkuiInvalidParameter,
            &format!("{name} pointer is null"),
        ));
    }
    // SAFETY: caller guarantees `ptr` is a valid pointer to a NUL-terminated
    // C string that outlives this call.
    let cstr = unsafe { CStr::from_ptr(ptr) };
    cstr.to_str().map(|s| s.to_string()).map_err(|_| {
        MkuiResult::error(
            MkuiErrorCode::MkuiInvalidParameter,
            &format!("Invalid {name}"),
        )
    })
}

/// Validate that `parent` references a live node in the tree. Returns
/// `false` if the slot is empty or the generation counter is stale.
///
/// Every child constructor MUST call this before dispatching into the
/// `_unchecked` push helpers — the runtime's `alloc_and_attach` asserts
/// on invalid parents, and a panic across `extern "C"` is undefined
/// behaviour. The generation counter is the entire reason `NodeId` is a
/// pair instead of a bare `u32`; this is where the guard pays off.
fn parent_is_live(tree: &AppTree, parent: MkuiNodeId) -> bool {
    tree.get(parent.into()).is_some()
}

/// Append a `View` under `parent`. Returns the new node's id, or
/// `MkuiNodeId{index=UINT_MAX, generation=UINT_MAX}` on error.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_view_child(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    class_name: *const c_char,
) -> MkuiNodeId {
    ffi_guard(invalid_node(), || {
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
        // P1 guard (Codex round 8): refuse stale or fabricated parent handles
        // before reaching the runtime's `assert!`-on-invalid-parent path. A
        // panic across `extern "C"` is undefined behaviour.
        let mut tree = app_ref.tree.borrow_mut();
        if !parent_is_live(&tree, parent) {
            return invalid_node();
        }
        let id = tree.push_view_unchecked(parent.into(), style);
        id.into()
    })
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
    ffi_guard(invalid_node(), || {
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
        // P1 guard (Codex round 8): see `mkui_app_view_child`.
        let mut tree = app_ref.tree.borrow_mut();
        if !parent_is_live(&tree, parent) {
            return invalid_node();
        }
        let id = tree.push_text_unchecked(parent.into(), content, variant, style);
        id.into()
    })
}

/// Append an enabled `Button` under `parent`. Pass `MkuiActionId{UINT_MAX,
/// UINT_MAX}` for no callback, or an id from [`mkui_app_register_callback`].
/// This original six-argument entry point preserves its C ABI and semantics.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_button_child(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    label: *const c_char,
    variant: c_int,
    class_name: *const c_char,
    on_press: MkuiActionId,
) -> MkuiNodeId {
    // SAFETY: identical pointer requirements to the disabled-aware entry point.
    unsafe {
        mkui_app_button_child_with_disabled(app, parent, label, variant, class_name, on_press, 0)
    }
}

/// Append a `Button` with an explicit disabled state. Zero means enabled;
/// any nonzero `disabled` value means disabled. Disabled buttons dim and do
/// not activate through UI input, even when `on_press` is registered. Direct
/// calls to `mkui_app_fire_action` still invoke registered actions.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_button_child_with_disabled(
    app: *mut MkuiApp,
    parent: MkuiNodeId,
    label: *const c_char,
    variant: c_int,
    class_name: *const c_char,
    on_press: MkuiActionId,
    disabled: c_int,
) -> MkuiNodeId {
    ffi_guard(invalid_node(), || {
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
        // P1 guard (Codex round 8): see `mkui_app_view_child`.
        let mut tree = app_ref.tree.borrow_mut();
        if !parent_is_live(&tree, parent) {
            return invalid_node();
        }
        let id = tree.push_button_with_disabled_unchecked(
            parent.into(),
            label,
            variant,
            style,
            action,
            disabled != 0,
        );
        id.into()
    })
}

/// Remove `node` and every descendant from the tree, unregistering any
/// action a removed `Button` held (#83). Returns `MkuiErrorCode::
/// MkuiSuccess` on success, or `MkuiErrorCode::MkuiInvalidParameter` if
/// `node` is stale/missing or names the root (the root has no parent to
/// detach from and the tree always carries exactly one).
#[no_mangle]
pub unsafe extern "C" fn mkui_app_remove_node(app: *mut MkuiApp, node: MkuiNodeId) -> MkuiResult {
    ffi_guard(panic_result(), || {
        if app.is_null() {
            return MkuiResult::error(MkuiErrorCode::MkuiInvalidParameter, "App pointer is null");
        }
        // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
        // A shared reference is enough — every field is a `RefCell`, and a
        // bare `&mut MkuiApp` here would alias a `&MkuiApp` an outer,
        // still-on-the-stack call (e.g. `mkui_app_fire_action` invoking a
        // callback that itself calls `mkui_app_remove_node` on the same
        // `app` pointer — removing a row from its own delete-button
        // handler) already holds.
        let app_ref = unsafe { &*app };
        let removed = {
            let mut tree = app_ref.tree.borrow_mut();
            tree.remove_node(node.into()).is_some()
        };
        if !removed {
            return MkuiResult::error(
                MkuiErrorCode::MkuiInvalidParameter,
                "node id is stale, missing, or the root",
            );
        }
        // The runtime already unregistered any action a removed Button
        // held (bumping its slot generation) — drop this binding's own
        // foreign-callback entry for it too, so `user_data` doesn't sit
        // referenced indefinitely and a later `mkui_app_fire_action` on
        // the id can't resolve a once-removed slot's entry if it is ever
        // reached before that call's own liveness check (belt + braces;
        // see `mkui_app_fire_action`).
        sweep_stale_callbacks(app_ref);
        MkuiResult::success()
    })
}

/// Drop every `callbacks` table entry whose `ActionId` no longer resolves
/// in the runtime's [`mkui_runtime::ActionRegistry`] (#83). Called after a
/// successful `mkui_app_remove_node` — the removal may have unregistered
/// one or more `Button.on_press` ids (the subtree root's own, plus any
/// descendant's), and this binding has no cheaper way to learn which
/// indices those were than to ask the registry directly via
/// [`mkui_runtime::ActionRegistry::contains`].
fn sweep_stale_callbacks(app_ref: &MkuiApp) {
    let tree = app_ref.tree.borrow();
    let actions = tree.actions();
    let mut callbacks = app_ref.callbacks.borrow_mut();
    for (idx, slot) in callbacks.iter_mut().enumerate() {
        if let Some(cb) = slot {
            if !actions.contains(ActionId::from_raw(idx as u32, cb.generation)) {
                *slot = None;
            }
        }
    }
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
    ffi_guard(invalid_action(), || {
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
    })
}

/// Fire a registered callback. Useful for test harnesses; the actual UI
/// fires callbacks via the renderer (console / web / wgpu) when a button
/// is pressed.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_fire_action(app: *mut MkuiApp, id: MkuiActionId) -> MkuiResult {
    ffi_guard(panic_result(), || {
        if app.is_null() {
            return MkuiResult::error(MkuiErrorCode::MkuiInvalidParameter, "App pointer is null");
        }
        // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
        let app_ref = unsafe { &*app };
        // Consult the runtime registry first, and drop this borrow before
        // invoking the callback below (#83): `sweep_stale_callbacks`
        // clears this binding's own table on removal, but only from
        // `mkui_app_remove_node` — asking the registry directly here is
        // what actually rejects a stale id if this ever runs without that
        // sweep having happened (e.g. a future caller that frees a node
        // through a path that skips it). Scoping the borrow also means a
        // callback that re-enters the tree (another `mkui_app_*` call)
        // does not hit an already-borrowed `RefCell` panic.
        let is_live = app_ref.tree.borrow().actions().contains(id.into());
        if !is_live {
            return MkuiResult::error(
                MkuiErrorCode::MkuiInvalidParameter,
                "Action id is stale or removed",
            );
        }
        // Scoped so the `callbacks` borrow is dropped before `func` runs
        // below — `func` is the host's callback, which may re-enter mkui
        // (e.g. `mkui_app_remove_node` on its own button, the delete-row
        // pattern). `func`/`user_data` are both `Copy` (a fn pointer and a
        // raw pointer), so copying them out of the borrow costs nothing.
        let (func, user_data) = {
            let callbacks = app_ref.callbacks.borrow();
            let Some(slot) = callbacks.get(id.index as usize) else {
                return MkuiResult::error(MkuiErrorCode::MkuiInvalidParameter, "Unknown action id");
            };
            let Some(cb) = slot else {
                return MkuiResult::error(
                    MkuiErrorCode::MkuiInvalidParameter,
                    "Action id is empty",
                );
            };
            if cb.generation != id.generation {
                return MkuiResult::error(
                    MkuiErrorCode::MkuiInvalidParameter,
                    "Action id is stale",
                );
            }
            (cb.func, cb.user_data)
        };
        func(user_data);
        MkuiResult::success()
    })
}

// -------------------------------------------------------------------------
// Renderer entry — console
// -------------------------------------------------------------------------

/// Run the application via the real console backend. Hands the runtime
/// `AppTree` to `mkui_console::Mkui::from_core` and invokes its interactive
/// loop. The Codex round-8 regression (Sprint 4 round-7 shipped a stub
/// `println!` summary instead of the real backend) is corrected here.
///
/// Requires the `console` feature (default). Without it, returns
/// `MkuiErrorCode::MkuiRuntimeError`.
///
/// Single function (not cfg-gated declaration) so cbindgen emits one
/// prototype regardless of build features — duplicate prototypes are
/// invalid C and the header would fail to compile downstream.
#[no_mangle]
pub unsafe extern "C" fn mkui_app_run_console(app: *mut MkuiApp) -> MkuiResult {
    ffi_guard(panic_result(), || {
        if app.is_null() {
            return MkuiResult::error(MkuiErrorCode::MkuiInvalidParameter, "App pointer is null");
        }
        #[cfg(feature = "console")]
        {
            // SAFETY: caller guarantees `app` was produced by `mkui_app_new`.
            let app_ref = unsafe { &mut *app };
            // Move the tree out of the MkuiApp; the binding cannot be reused
            // after a successful `run_console`, mirroring the v0.4.x
            // semantics where `app.run()` consumed the Rust `Mkui`. The C
            // host calls `mkui_app_free` after run returns.
            let tree = std::mem::replace(&mut *app_ref.tree.borrow_mut(), AppTree::new());
            let core = mkui_core::components::Mkui::with_tree(tree);
            let console = match mkui_console::Mkui::from_core(core) {
                Ok(m) => m,
                Err(e) => {
                    return MkuiResult::error(
                        MkuiErrorCode::MkuiInitializationFailed,
                        &e.to_string(),
                    );
                }
            };
            match console.run() {
                Ok(()) => MkuiResult::success(),
                Err(e) => MkuiResult::error(MkuiErrorCode::MkuiRuntimeError, &e.to_string()),
            }
        }
        #[cfg(not(feature = "console"))]
        {
            let _ = app;
            MkuiResult::error(
                MkuiErrorCode::MkuiRuntimeError,
                "mkui-c was built without the `console` feature",
            )
        }
    })
}

// -------------------------------------------------------------------------
// Snapshot — parity-test entry point
// -------------------------------------------------------------------------

/// Emit a canonical JSON snapshot of the current tree. Used by the
/// cross-binding parity tests; the host frees the returned buffer via
/// [`mkui_free_error_message`] (same allocator).
#[no_mangle]
pub unsafe extern "C" fn mkui_app_snapshot_json(app: *const MkuiApp) -> *mut c_char {
    ffi_guard(ptr::null_mut(), || {
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
    })
}

/// Free an error message or JSON buffer returned by mkui functions.
#[no_mangle]
pub unsafe extern "C" fn mkui_free_error_message(message: *mut c_char) {
    ffi_guard((), || {
        if message.is_null() {
            return;
        }
        // SAFETY: caller guarantees `message` was returned by an mkui function
        // (via `CString::into_raw`). Reconstructing the CString drops it.
        unsafe {
            let _ = CString::from_raw(message);
        }
    })
}

/// Get version information.
#[no_mangle]
pub extern "C" fn mkui_version() -> *const c_char {
    ffi_guard(ptr::null(), || {
        static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
        VERSION.as_ptr() as *const c_char
    })
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
            assert!(matches!(result.code, MkuiErrorCode::MkuiSuccess));
            assert_eq!(counter.get(), 1);

            mkui_app_free(app);
        }
    }

    #[test]
    fn unknown_class_string_still_creates_a_valid_node() {
        // #269: an unrecognized utility class no longer fails the parse —
        // it's dropped (and logged) rather than rejecting the whole node.
        // SAFETY: single-threaded test.
        unsafe {
            let app = mkui_app_new();
            let root = mkui_app_root(app);
            let bogus = CString::new("not-a-utility").unwrap();
            let id = mkui_app_view_child(app, root, bogus.as_ptr());
            assert_ne!(
                id,
                invalid_node(),
                "an unknown utility must not block node creation"
            );
            mkui_app_free(app);
        }
    }

    #[test]
    fn a_delete_row_callback_can_remove_its_own_button() {
        // The headline use case for removal: a row's own delete button
        // calls `mkui_app_remove_node` on itself from inside the callback
        // `mkui_app_fire_action` is still in the middle of invoking.
        // `fire_action` must drop its `callbacks` borrow before calling
        // the host function, or this reentrant `remove_node` (which
        // sweeps the same `callbacks` table) hits an already-borrowed
        // `RefCell` and panics — caught by `ffi_guard`, but surfacing as
        // a spurious `MkuiRuntimeError` for a removal that actually
        // happened underneath it.
        // `button`/`removal_code` are `Cell`s (not plain fields written
        // through `&mut ctx`) so every access — including the one from
        // `on_delete`, running through the raw pointer handed to
        // `mkui_app_register_callback` — goes through shared-reference
        // interior mutability. A later ordinary write through `&mut ctx`
        // would invalidate that earlier-constructed `*const DeleteCtx`,
        // same pattern as `handle_callback_round_trip`'s `Cell<u32>`.
        struct DeleteCtx {
            app: *mut MkuiApp,
            button: std::cell::Cell<MkuiNodeId>,
            removal_code: std::cell::Cell<Option<MkuiErrorCode>>,
        }

        extern "C" fn on_delete(data: *mut c_void) {
            // SAFETY: `data` is `&DeleteCtx` on the test's stack, alive for
            // the whole call (the test waits for `mkui_app_fire_action` to
            // return before it does anything else).
            let ctx = unsafe { &*(data as *const DeleteCtx) };
            // SAFETY: `ctx.app` is the same live `MkuiApp` the outer
            // `mkui_app_fire_action` call is already operating on.
            let result = unsafe { mkui_app_remove_node(ctx.app, ctx.button.get()) };
            ctx.removal_code.set(Some(result.code));
        }

        // SAFETY: single-threaded test; every pointer comes from a
        // `Box::into_raw` we own or a `CString` that outlives the call.
        unsafe {
            let app = mkui_app_new();
            let root = mkui_app_root(app);
            let class = CString::new("").unwrap();
            let label = CString::new("delete").unwrap();

            let ctx = DeleteCtx {
                app,
                button: std::cell::Cell::new(invalid_node()),
                removal_code: std::cell::Cell::new(None),
            };
            let action = mkui_app_register_callback(
                app,
                Some(on_delete),
                &ctx as *const DeleteCtx as *mut c_void,
            );
            let button = mkui_app_button_child(
                app,
                root,
                label.as_ptr(),
                MKUI_BUTTON_PRIMARY,
                class.as_ptr(),
                action,
            );
            ctx.button.set(button);

            let fire = mkui_app_fire_action(app, action);
            assert!(
                matches!(fire.code, MkuiErrorCode::MkuiSuccess),
                "the outer fire must still report success, not a RefCell panic \
                 caught as MkuiRuntimeError: {:?}",
                fire.code
            );
            assert!(
                matches!(ctx.removal_code.get(), Some(MkuiErrorCode::MkuiSuccess)),
                "the reentrant remove_node (inside the callback) must itself \
                 succeed: {:?}",
                ctx.removal_code.get()
            );
            assert!(
                matches!(
                    mkui_app_remove_node(app, button).code,
                    MkuiErrorCode::MkuiInvalidParameter
                ),
                "the button must actually be gone after the reentrant removal"
            );

            mkui_app_free(app);
        }
    }

    #[test]
    fn remove_node_detaches_the_node_and_rejects_the_stale_id_afterward() {
        // SAFETY: single-threaded test; every pointer comes from a
        // `Box::into_raw` we own.
        unsafe {
            let app = mkui_app_new();
            let root = mkui_app_root(app);
            let class = CString::new("").unwrap();
            let view = mkui_app_view_child(app, root, class.as_ptr());
            assert_ne!(view, invalid_node());

            let result = mkui_app_remove_node(app, view);
            assert!(matches!(result.code, MkuiErrorCode::MkuiSuccess));

            // Removing the same (now-stale) id again must fail, not panic.
            let second = mkui_app_remove_node(app, view);
            assert!(matches!(second.code, MkuiErrorCode::MkuiInvalidParameter));

            mkui_app_free(app);
        }
    }

    #[test]
    fn removing_a_buttons_node_stops_its_action_from_firing() {
        // Firing an id whose button was removed must be rejected, not
        // re-invoke the (possibly use-after-free) callback — the whole
        // point of the runtime registry's generation bump on remove.
        unsafe {
            extern "C" fn cb(data: *mut c_void) {
                let cell = data as *const std::cell::Cell<u32>;
                let c = unsafe { &*cell };
                c.set(c.get() + 1);
            }

            let counter = std::cell::Cell::new(0u32);
            let app = mkui_app_new();
            let root = mkui_app_root(app);
            let action =
                mkui_app_register_callback(app, Some(cb), (&counter) as *const _ as *mut c_void);
            let class = CString::new("").unwrap();
            let label = CString::new("ok").unwrap();
            let button = mkui_app_button_child(
                app,
                root,
                label.as_ptr(),
                MKUI_BUTTON_PRIMARY,
                class.as_ptr(),
                action,
            );
            assert_ne!(button, invalid_node());

            let removal = mkui_app_remove_node(app, button);
            assert!(matches!(removal.code, MkuiErrorCode::MkuiSuccess));

            let fire = mkui_app_fire_action(app, action);
            assert!(
                matches!(fire.code, MkuiErrorCode::MkuiInvalidParameter),
                "firing a removed button's action must be rejected"
            );
            assert_eq!(
                counter.get(),
                0,
                "the callback must not run after its node was removed"
            );

            mkui_app_free(app);
        }
    }

    #[test]
    fn remove_node_on_stale_or_root_id_returns_invalid_parameter() {
        // SAFETY: single-threaded test.
        unsafe {
            let app = mkui_app_new();
            let stale = MkuiNodeId {
                index: u32::MAX - 1,
                generation: 0,
            };
            let result = mkui_app_remove_node(app, stale);
            assert!(matches!(result.code, MkuiErrorCode::MkuiInvalidParameter));

            let root = mkui_app_root(app);
            let root_removal = mkui_app_remove_node(app, root);
            assert!(matches!(
                root_removal.code,
                MkuiErrorCode::MkuiInvalidParameter
            ));

            mkui_app_free(app);
        }
    }

    #[test]
    fn stale_or_fabricated_parent_returns_invalid_not_panic() {
        // Regression test for Codex round-8 P1: a fabricated parent
        // `NodeId` reaching `push_*_unchecked` would `assert!` and abort
        // across the FFI boundary. Every child constructor now validates
        // the parent before mutating; this test exercises all three.
        // SAFETY: single-threaded test.
        unsafe {
            let app = mkui_app_new();
            let stale = MkuiNodeId {
                index: u32::MAX - 1,
                generation: 0,
            };
            let class = CString::new("").unwrap();
            let content = CString::new("hi").unwrap();
            let label = CString::new("ok").unwrap();

            let v = mkui_app_view_child(app, stale, class.as_ptr());
            assert_eq!(v, invalid_node(), "stale parent must surface as invalid");

            let t =
                mkui_app_text_child(app, stale, content.as_ptr(), MKUI_TEXT_BODY, class.as_ptr());
            assert_eq!(t, invalid_node());

            let b = mkui_app_button_child(
                app,
                stale,
                label.as_ptr(),
                MKUI_BUTTON_PRIMARY,
                class.as_ptr(),
                invalid_action(),
            );
            assert_eq!(b, invalid_node());

            // Wrong-generation handle (real index, bumped generation) must
            // also be rejected — the entire point of generation counters.
            let root = mkui_app_root(app);
            let wrong_gen = MkuiNodeId {
                index: root.index,
                generation: root.generation.wrapping_add(1),
            };
            let v2 = mkui_app_view_child(app, wrong_gen, class.as_ptr());
            assert_eq!(v2, invalid_node());

            mkui_app_free(app);
        }
    }

    // -------------------------------------------------------------------------
    // Panic safety (#103): a Rust panic must never unwind across the
    // `extern "C"` boundary. The `test_panic` injection hook fires a synthetic
    // panic inside each wrapper (after entry, before normal return); we assert
    // the type-appropriate, allocation-free fallback comes back and that no
    // unwind reaches this test caller. The hook is `#[cfg(test)]`-only — it
    // adds no exported symbol and no code to release/public builds, and there
    // is no Cargo feature gating it (see the `test_panic` module).
    //
    // The `*const MkuiApp` / `*mut MkuiApp` arguments below are never
    // dereferenced: injection panics ahead of the wrapper body, so passing a
    // dangling/dummy handle is sound here.

    /// A non-null but never-dereferenced app handle for the panic-fallback
    /// tests. Sound only because the injection hook panics before the wrapper
    /// body touches `app`.
    fn dummy_app() -> *mut MkuiApp {
        std::ptr::dangling_mut()
    }

    #[test]
    fn panic_app_new_returns_null() {
        test_panic::arm();
        let app = mkui_app_new();
        assert!(app.is_null(), "panic fallback must be a null pointer");
    }

    #[test]
    fn panic_app_free_swallows_and_returns() {
        // SAFETY: injection panics before the body, so the dummy handle is
        // never reconstructed/freed.
        unsafe {
            test_panic::arm();
            mkui_app_free(dummy_app()); // returns normally after swallowing
        }
    }

    #[test]
    fn panic_app_root_returns_invalid_node() {
        // SAFETY: handle never dereferenced (see module note).
        unsafe {
            test_panic::arm();
            assert_eq!(mkui_app_root(dummy_app()), invalid_node());
        }
    }

    #[test]
    fn panic_view_child_returns_invalid_node() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let id = mkui_app_view_child(dummy_app(), invalid_node(), ptr::null());
            assert_eq!(id, invalid_node());
        }
    }

    #[test]
    fn panic_text_child_returns_invalid_node() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let id = mkui_app_text_child(
                dummy_app(),
                invalid_node(),
                ptr::null(),
                MKUI_TEXT_BODY,
                ptr::null(),
            );
            assert_eq!(id, invalid_node());
        }
    }

    #[test]
    fn panic_button_child_returns_invalid_node() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let id = mkui_app_button_child(
                dummy_app(),
                invalid_node(),
                ptr::null(),
                MKUI_BUTTON_PRIMARY,
                ptr::null(),
                invalid_action(),
            );
            assert_eq!(id, invalid_node());
        }
    }

    #[test]
    fn panic_register_callback_returns_invalid_action() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let id = mkui_app_register_callback(dummy_app(), None, ptr::null_mut());
            assert_eq!(id, invalid_action());
        }
    }

    #[test]
    fn panic_remove_node_returns_runtime_error_null_message() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let result = mkui_app_remove_node(dummy_app(), invalid_node());
            assert!(matches!(result.code, MkuiErrorCode::MkuiRuntimeError));
            assert!(result.message.is_null(), "fallback must not allocate");
        }
    }

    #[test]
    fn panic_fire_action_returns_runtime_error_null_message() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let result = mkui_app_fire_action(dummy_app(), invalid_action());
            assert!(matches!(result.code, MkuiErrorCode::MkuiRuntimeError));
            assert!(result.message.is_null(), "fallback must not allocate");
        }
    }

    #[test]
    fn panic_run_console_returns_runtime_error_null_message() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            let result = mkui_app_run_console(dummy_app());
            assert!(matches!(result.code, MkuiErrorCode::MkuiRuntimeError));
            assert!(result.message.is_null(), "fallback must not allocate");
        }
    }

    #[test]
    fn panic_snapshot_json_returns_null() {
        // SAFETY: handle never dereferenced.
        unsafe {
            test_panic::arm();
            assert!(mkui_app_snapshot_json(dummy_app()).is_null());
        }
    }

    #[test]
    fn panic_free_error_message_swallows_and_returns() {
        // SAFETY: injection panics before the body, so the dummy pointer is
        // never reconstructed/freed.
        unsafe {
            test_panic::arm();
            mkui_free_error_message(std::ptr::dangling_mut());
        }
    }

    #[test]
    fn panic_version_returns_null() {
        test_panic::arm();
        assert!(mkui_version().is_null());
    }

    #[test]
    fn injection_disarms_after_one_panic() {
        // The hook fires exactly once per `arm()`, so the next call runs
        // normally — proving the recovery path can't re-enter the injection.
        test_panic::arm();
        assert!(mkui_app_new().is_null());
        let app = mkui_app_new();
        assert!(!app.is_null(), "second call must not panic");
        // SAFETY: `app` came from `mkui_app_new`.
        unsafe { mkui_app_free(app) };
    }
}
