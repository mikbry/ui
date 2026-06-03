//! Python bindings for mkui — handle-based, runtime-backed.
//!
//! Sprint 4 rewrite: replaces the v0.4.x flat `add_view` / `add_text` /
//! `add_button` shape with a nested handle-based API that builds into the
//! same [`mkui_runtime::AppTree`] every other binding consumes:
//!
//! ```python
//! import mkui_py
//!
//! app = mkui_py.App()
//! root = app.root()
//! header = app.view_child(root, "border-b")
//! app.text_child(header, "Title", mkui_py.TEXT_HEADING_1, "text-xl")
//!
//! def on_click():
//!     print("clicked")
//!
//! action = app.register_callback(on_click)
//! app.button_child(header, "OK", mkui_py.BUTTON_PRIMARY, "", action)
//!
//! app.run_console()
//! ```
//!
//! Bumped to PyO3 0.28.3 (was 0.22). The new `Bound`-based API is required;
//! the older `&PyAny` patterns no longer compile. This bump unblocks Python
//! 3.14 (audit Phase 5 Task 24).

use mkui_runtime::{ActionId, AppTree, ButtonVariant, NodeId, StyleClass, TextVariant};
use pyo3::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

/// `repr(transparent)` wrapper exposing the runtime `NodeId` as a Python
/// class. The two `u32` fields are accessible from Python as `.index` /
/// `.generation` so test code can compare ids.
#[pyclass(frozen, eq, hash, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PyNodeId {
    #[pyo3(get)]
    pub index: u32,
    #[pyo3(get)]
    pub generation: u32,
}

impl From<NodeId> for PyNodeId {
    fn from(id: NodeId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation(),
        }
    }
}

impl From<PyNodeId> for NodeId {
    fn from(id: PyNodeId) -> Self {
        NodeId::from_raw(id.index, id.generation)
    }
}

#[pymethods]
impl PyNodeId {
    fn __repr__(&self) -> String {
        format!(
            "NodeId(index={}, generation={})",
            self.index, self.generation
        )
    }
}

/// Action id mirror.
#[pyclass(frozen, eq, hash, from_py_object)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PyActionId {
    #[pyo3(get)]
    pub index: u32,
    #[pyo3(get)]
    pub generation: u32,
}

impl From<ActionId> for PyActionId {
    fn from(id: ActionId) -> Self {
        Self {
            index: id.index(),
            generation: id.generation(),
        }
    }
}

impl From<PyActionId> for ActionId {
    fn from(id: PyActionId) -> Self {
        ActionId::from_raw(id.index, id.generation)
    }
}

#[pymethods]
impl PyActionId {
    fn __repr__(&self) -> String {
        format!(
            "ActionId(index={}, generation={})",
            self.index, self.generation
        )
    }
}

/// Python wrapper for an `AppTree`.
///
/// `unsendable` because the runtime is single-threaded by design (Codex
/// anti-pattern guard — `ActionRegistry` is not `Send + Sync`). Python
/// callers must stay on one thread; PyO3 enforces this at runtime.
#[pyclass(unsendable)]
pub struct App {
    tree: Rc<RefCell<AppTree>>,
    /// Parallel callback table; same shape as the C binding's. Keyed by
    /// `ActionId.index()`, generation tracked alongside.
    callbacks: Rc<RefCell<Vec<Option<PyCallback>>>>,
}

struct PyCallback {
    func: Py<PyAny>,
    generation: u32,
}

#[pymethods]
impl App {
    // allow: `#[new]` is the PyO3 constructor; a parallel `impl Default` would
    // either duplicate the body or fight the `#[pyclass]` toolchain that does
    // not expect it to coexist with `#[new]`. Cleanup tracked in
    // https://github.com/mikbry/ui/issues/53 (Codex round-11 P2 #4).
    #[allow(clippy::new_without_default)]
    #[new]
    pub fn new() -> Self {
        Self {
            tree: Rc::new(RefCell::new(AppTree::new())),
            callbacks: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Return the synthetic root id. Children must attach under this or a
    /// descendant of this.
    pub fn root(&self) -> PyNodeId {
        self.tree.borrow().root().into()
    }

    /// Append a `View` under `parent`. `class_name` is optional.
    #[pyo3(signature = (parent, class_name=""))]
    pub fn view_child(&self, parent: PyNodeId, class_name: &str) -> PyResult<PyNodeId> {
        let style = StyleClass::from_str(class_name);
        style
            .parse()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let mut tree = self.tree.borrow_mut();
        // P1 guard (Codex round 8): refuse stale parent handles before
        // reaching the runtime's `assert!`-on-invalid-parent path. PyO3
        // catches panics but turns them into PanicException, which is a
        // much worse diagnostic than a typed `PyValueError`.
        if tree.get(parent.into()).is_none() {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "stale or invalid parent NodeId",
            ));
        }
        let id = tree.push_view_unchecked(parent.into(), style);
        Ok(id.into())
    }

    /// Append a `Text` under `parent`.
    #[pyo3(signature = (parent, content, variant, class_name=""))]
    pub fn text_child(
        &self,
        parent: PyNodeId,
        content: &str,
        variant: i32,
        class_name: &str,
    ) -> PyResult<PyNodeId> {
        let variant = TextVariant::from_ffi(variant)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let style = StyleClass::from_str(class_name);
        style
            .parse()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let mut tree = self.tree.borrow_mut();
        // P1 guard — see `view_child`.
        if tree.get(parent.into()).is_none() {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "stale or invalid parent NodeId",
            ));
        }
        let id = tree.push_text_unchecked(parent.into(), content, variant, style);
        Ok(id.into())
    }

    /// Append a `Button` under `parent`.
    #[pyo3(signature = (parent, label, variant, class_name="", on_press=None))]
    pub fn button_child(
        &self,
        parent: PyNodeId,
        label: &str,
        variant: i32,
        class_name: &str,
        on_press: Option<PyActionId>,
    ) -> PyResult<PyNodeId> {
        let variant = ButtonVariant::from_ffi(variant)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let style = StyleClass::from_str(class_name);
        style
            .parse()
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;
        let action = on_press.map(|a| a.into());
        let mut tree = self.tree.borrow_mut();
        // P1 guard — see `view_child`.
        if tree.get(parent.into()).is_none() {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "stale or invalid parent NodeId",
            ));
        }
        let id = tree.push_button_unchecked(parent.into(), label, variant, style, action);
        Ok(id.into())
    }

    /// Register a Python callable as an action callback. Returns an
    /// `ActionId` the host passes to `button_child`. The callable is
    /// kept alive on the Rust side via `Py<PyAny>`.
    pub fn register_callback(&self, func: Py<PyAny>) -> PyActionId {
        let id = self.tree.borrow_mut().actions_mut().register_remote();
        let idx = id.index() as usize;
        let mut callbacks = self.callbacks.borrow_mut();
        if callbacks.len() <= idx {
            callbacks.resize_with(idx + 1, || None);
        }
        callbacks[idx] = Some(PyCallback {
            func,
            generation: id.generation(),
        });
        id.into()
    }

    /// Fire an action by id. Used in tests; production renderers fire on
    /// real user interaction.
    pub fn fire_action(&self, py: Python<'_>, id: PyActionId) -> PyResult<()> {
        let callbacks = self.callbacks.borrow();
        let Some(slot) = callbacks.get(id.index as usize) else {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "unknown action id",
            ));
        };
        let Some(cb) = slot else {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "empty action id",
            ));
        };
        if cb.generation != id.generation {
            return Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(
                "stale action id",
            ));
        }
        cb.func.call0(py)?;
        Ok(())
    }

    /// Number of live nodes in the tree (includes the synthetic root).
    pub fn node_count(&self) -> usize {
        self.tree.borrow().len()
    }

    /// Canonical JSON snapshot. Identical to `mkui_app_snapshot_json` on the
    /// C side — the parity gate compares the two.
    pub fn snapshot_json(&self) -> String {
        let tree = self.tree.borrow();
        let snapshot = mkui_runtime::snapshot::TreeSnapshot::of(&tree);
        snapshot.to_json()
    }

    /// Run the application via the real console backend. Hands the
    /// runtime `AppTree` to `mkui_console::Mkui::from_core` and invokes
    /// its interactive loop. The Codex round-8 regression (Sprint 4
    /// round-7 shipped a stub `println!` summary instead of the real
    /// backend) is corrected here.
    ///
    /// Requires the `console` feature (default). Consumes the tree —
    /// after a successful return, the `App` is empty and should be
    /// dropped.
    #[cfg(feature = "console")]
    pub fn run_console(&self) -> PyResult<()> {
        let tree = std::mem::replace(&mut *self.tree.borrow_mut(), AppTree::new());
        let core = mkui_core::components::Mkui::with_tree(tree);
        let console = mkui_console::Mkui::from_core(core).map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "console backend init failed: {e}"
            ))
        })?;
        console.run().map_err(|e| {
            PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!("console run failed: {e}"))
        })
    }

    /// Fallback when the `console` feature is disabled.
    #[cfg(not(feature = "console"))]
    pub fn run_console(&self) -> PyResult<()> {
        Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
            "mkui-py was built without the `console` feature",
        ))
    }
}

// Rust-only helpers — callable from integration tests in `tests/` that
// run without a live Python interpreter. PyO3's `#[pymethods]` block
// generates Python dispatchers; methods in a *plain* `impl App` block are
// invisible to Python.
impl App {
    /// Construct an `App` without going through Python's `__new__`.
    /// `App::new()` itself is already a plain Rust function (PyO3 marks
    /// it `#[new]` but the body is callable from Rust); this alias exists
    /// for clarity in test code.
    pub fn new_for_test() -> Self {
        Self::new()
    }

    /// Allocate an action id with no Python callback — used by the
    /// byte-identical parity test in `tests/parity.rs`, which can't
    /// construct a `Py<PyAny>` without a live interpreter.
    ///
    /// Behaves identically to [`Self::register_callback`] from the
    /// runtime's perspective: it bumps the `ActionRegistry`'s
    /// `register_remote` slot. The callback table on the Python side
    /// stays empty for this id (firing it from Python would be a no-op).
    pub fn register_remote_action_for_test(&self) -> PyActionId {
        self.tree
            .borrow_mut()
            .actions_mut()
            .register_remote()
            .into()
    }
}

/// Convenience factory matching the v0.4.x API surface.
#[pyfunction]
fn create_app() -> App {
    App::new()
}

/// Get the mkui version.
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Run the showcase (Rust-side) — convenience for parity with
/// `mkui::run!`. Sprint 4 keeps this minimal; the real cross-language
/// showcase is tracked in Sprint 6+.
#[pyfunction]
fn run_showcase() -> PyResult<()> {
    match showcase_common::create_showcase_ui() {
        Ok(app) => match app.run() {
            Ok(_) => Ok(()),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "Showcase run failed: {e}"
            ))),
        },
        Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
            "Failed to create showcase: {e}"
        ))),
    }
}

/// Python module definition.
///
/// `pub` so the embedded-interpreter smoke test (`tests/smoke_py.rs`) can
/// register it via `pyo3::append_to_inittab!` and `import mkui_py` from a
/// live interpreter. The `maturin develop` cdylib path uses the same
/// `#[pymodule]`-generated entry point.
#[pymodule]
pub fn mkui_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<App>()?;
    m.add_class::<PyNodeId>()?;
    m.add_class::<PyActionId>()?;
    m.add_function(wrap_pyfunction!(create_app, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(run_showcase, m)?)?;

    // Button variant constants
    m.add("BUTTON_PRIMARY", ButtonVariant::Primary.to_ffi())?;
    m.add("BUTTON_SECONDARY", ButtonVariant::Secondary.to_ffi())?;
    m.add("BUTTON_DESTRUCTIVE", ButtonVariant::Destructive.to_ffi())?;
    m.add("BUTTON_OUTLINE", ButtonVariant::Outline.to_ffi())?;
    m.add("BUTTON_GHOST", ButtonVariant::Ghost.to_ffi())?;
    m.add("BUTTON_LINK", ButtonVariant::Link.to_ffi())?;

    // Text variant constants
    m.add("TEXT_BODY", TextVariant::Body.to_ffi())?;
    m.add("TEXT_HEADING_1", TextVariant::Heading1.to_ffi())?;
    m.add("TEXT_HEADING_2", TextVariant::Heading2.to_ffi())?;
    m.add("TEXT_HEADING_3", TextVariant::Heading3.to_ffi())?;
    m.add("TEXT_CAPTION", TextVariant::Caption.to_ffi())?;
    m.add("TEXT_LABEL", TextVariant::Label.to_ffi())?;
    m.add("TEXT_CODE", TextVariant::Code.to_ffi())?;

    Ok(())
}
