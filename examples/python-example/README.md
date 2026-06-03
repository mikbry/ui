# mkui Python Example

This example demonstrates how to use mkui from Python to create
cross-platform console applications using the Sprint 4 handle-based
runtime API.

## Prerequisites

- Python 3.10 or higher (PyO3 0.28.3 requires modern Python)
- Rust toolchain (for building the bindings)
- maturin (install with `pip install maturin`)

## Build & Run

1. **Build the Python bindings**:
   ```bash
   cd ../../crates/mkui-py
   uv venv
   source .venv/bin/activate
   maturin develop --release
   ```

2. **Run the example**:
   ```bash
   cd ../../examples/python-example
   python main.py
   ```

## Features Demonstrated

- **Handle-based nested API**: every constructor takes an explicit parent
  `NodeId` and returns a child id. No fluent chaining — the tree is
  explicit and matches the C/Rust APIs byte-for-byte.
- **Action callbacks**: `app.register_callback(fn)` returns a stable
  `ActionId`, which `button_child(..., on_press=action)` consumes.
- **Variant constants**: `BUTTON_PRIMARY`, `TEXT_HEADING_1`, etc.

## API Example

```python
import mkui_py

def main():
    app = mkui_py.App()
    root = app.root()

    container = app.view_child(root, "flex-1")
    app.text_child(container, "Hello World!",
                   mkui_py.TEXT_HEADING_1, "text-xl font-bold")

    on_click = app.register_callback(lambda: print("Clicked"))
    app.button_child(container, "Click me",
                     mkui_py.BUTTON_PRIMARY, "", on_click)

    app.run_console()

if __name__ == "__main__":
    main()
```

The pre-Sprint-4 fluent `app.view(...).text(...).button(...)` shape was
removed when the runtime substrate landed (#51) — the new handle-based
shape matches every other binding (`mkui_app_view_child` in C,
`viewChild` in C++).

## CI status

`mkui-py` is in the CI build/test matrix. Its default test path does not
link `libpython` (PyO3's `extension-module` provides the symbols at load
time), so `cargo test --workspace` covers it with no Python toolchain. The
two interpreter-linked tests (snapshot parity + `import mkui_py` smoke) are
feature-gated behind `parity-test` and run against any Python 3.9–3.14:

```bash
PYO3_PYTHON=$(which python3) cargo test -p mkui-py \
  --no-default-features --features "parity-test,console" --locked
```

Local development with `maturin develop` works on Python 3.9–3.14
Linux/macOS hosts.
