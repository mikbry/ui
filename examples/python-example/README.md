# mkui Python Example

This example demonstrates how to use mkui from Python to create cross-platform console applications.

## Prerequisites

- Python 3.8 or higher
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

- **Method chaining API**: Build UIs using fluent interface patterns
- **Exception handling**: Proper error propagation with Python exceptions  
- **Button variants**: All 6 button styles (Primary, Secondary, Destructive, Outline, Ghost, Link)
- **Multiple API styles**: Both method chaining and step-by-step approaches
- **Showcase integration**: Access to common showcase components

## API Examples

### Method Chaining (Pythonic)
```python
app = mkui_py.create_app()
(app.view("flex-1")
    .text("Hello World!", "text-xl font-bold")
    .button("Click me", mkui_py.BUTTON_PRIMARY)
    .run_console())
```

### Step-by-Step
```python
app = mkui_py.App()
app.add_view("container mx-auto p-4")
app.add_text("Step-by-step Example", "text-2xl font-bold")
app.add_button("Click me!", mkui_py.BUTTON_PRIMARY)
app.run_console()
```

### Showcase
```python
mkui_py.run_showcase()  # Runs the common showcase
```