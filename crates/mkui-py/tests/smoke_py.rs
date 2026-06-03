//! Live Python-interpreter smoke test for the mkui bindings.
//!
//! Where `tests/parity.rs` calls the `App` methods directly from Rust
//! (exercising the method *bodies* but not PyO3's generated Python
//! dispatchers), this test drives the binding the way a real user does:
//! it registers the extension module into an embedded interpreter's
//! inittab, then runs a Python script that `import mkui_py`, builds a
//! tree, registers a callback, and fires it. This is the end-to-end
//! "the bindings actually work on this interpreter" check the issue #5
//! acceptance criteria ask for.
//!
//! Gated behind `feature = "parity-test"` (enables
//! `pyo3/auto-initialize`, which links a real `libpython` so the test
//! binary can embed an interpreter). The `extension-module` feature is
//! mutually exclusive with that link mode, so callers MUST disable
//! default features:
//!
//! ```text
//! PYO3_PYTHON=$(which python3) cargo test -p mkui-py \
//!   --no-default-features --features "parity-test,console" \
//!   --test smoke_py --locked
//! ```
#![cfg(feature = "parity-test")]

use pyo3::prelude::*;
use std::ffi::CString;

// `#[pymodule] fn mkui_py` expands to a sibling `mod mkui_py` holding the
// `__PYO3_NAME` / `__pyo3_init` glue that `append_to_inittab!` needs. From
// this integration-test crate that module lives at `mkui_py::mkui_py`;
// import it so the macro's `mkui_py::__PYO3_NAME` path resolves.
use mkui_py::mkui_py;

#[test]
fn python_interpreter_builds_and_fires_through_bindings() {
    // Must run before the interpreter is first attached: makes
    // `import mkui_py` resolve to the in-process extension module rather
    // than a wheel on `sys.path`.
    pyo3::append_to_inittab!(mkui_py);

    Python::attach(|py| {
        // Mirrors the README Python example: nested handle-based build,
        // a registered callback, and a fired action — all through the
        // interpreter's attribute/method dispatch onto the PyO3 layer.
        let script = CString::new(
            r#"
import mkui_py

app = mkui_py.App()
root = app.root()

header = app.view_child(root, "border-b")
app.text_child(header, "Title", mkui_py.TEXT_HEADING_1, "text-xl")

fired = []
def on_click():
    fired.append(True)

action = app.register_callback(on_click)
app.button_child(header, "OK", mkui_py.BUTTON_PRIMARY, "", action)

# root + header + text + button == 4 live nodes.
assert app.node_count() == 4, f"node_count={app.node_count()}"

# The registered Python callable fires through the Rust action table.
app.fire_action(action)
assert fired == [True], f"callback did not fire: {fired}"

# Module-level conveniences are wired up.
assert mkui_py.version(), "version() returned empty"

# A snapshot round-trips as JSON for the cross-binding parity gate.
import json
snapshot = json.loads(app.snapshot_json())
assert snapshot, "snapshot_json() produced empty JSON"
"#,
        )
        .expect("script contains no interior NUL");

        py.run(script.as_c_str(), None, None)
            .expect("python smoke script failed");
    });
}
