# mkui C/C++ Bindings

This directory contains C and C++ bindings for the mkui UI library, enabling developers to create cross-platform applications using C or C++.

## Structure

```
bindings/
├── cpp/
│   └── mkui.hpp          # Modern C++ wrapper (C++17)
└── README.md             # This file
```

## Features

- **Cross-platform**: Works on Linux, macOS, and Windows
- **Memory safe**: RAII in C++, proper cleanup functions in C
- **Modern C++**: C++17 features like smart pointers and method chaining
- **Exception safety**: C++ wrapper converts errors to exceptions
- **Zero-cost abstractions**: C++ wrapper is header-only with minimal overhead

## Quick Start

### C++ (Recommended)

```cpp
#include "mkui.hpp"

int main() {
    try {
        auto app = mkui::createApp();
        auto root = app->root();
        auto container = app->viewChild(root, "container");
        app->textChild(container, "Hello, World!",
                       mkui::TextVariant::Heading1, "text-xl font-bold");
        app->buttonChild(container, "Click me!", mkui::ButtonVariant::Primary);
        app->runConsole();
    } catch (const mkui::MkuiException& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    return 0;
}
```

### C

```c
#include "mkui_c.h"

int main() {
    MkuiApp* app = mkui_app_new();
    if (!app) return 1;

    MkuiNodeId root = mkui_app_root(app);
    MkuiNodeId container = mkui_app_view_child(app, root, "container");
    mkui_app_text_child(app, container, "Hello, World!",
                        MKUI_TEXT_HEADING_1, "text-xl font-bold");
    mkui_app_button_child(app, container, "Click me!", MKUI_BUTTON_PRIMARY, "",
                          (MkuiActionId){UINT32_MAX, UINT32_MAX});

    MkuiResult result = mkui_app_run_console(app);
    mkui_app_free(app);
    return result.code == MKUI_SUCCESS ? 0 : 1;
}
```

## Building

### Prerequisites

- Rust toolchain (for building the mkui library)
- C/C++ compiler (gcc, clang, or MSVC)
- Make (optional, for using provided Makefiles)

### Steps

1. **Build the Rust library**:
   ```bash
   cd /path/to/mkui
   cargo build --release -p mkui-c
   ```

2. **Build C example**:
   ```bash
   cd examples/c-example
   make
   ```

3. **Build C++ example**:
   ```bash
   cd examples/cpp-example
   make
   ```

## API Reference

### C++ API (mkui.hpp)

#### Classes

- **`mkui::App`**: Main application class with RAII semantics
- **`mkui::MkuiException`**: Exception class for error handling

#### Methods

- **`App::root()`**: Return the runtime tree's root `NodeId`.
- **`App::viewChild(parent, className)`**: Append a view under `parent`.
- **`App::textChild(parent, content, variant, className)`**: Append text.
- **`App::buttonChild(parent, label, variant, className, onPress)`**: Append button. Pass `mkui::kNoAction` for no callback.
- **`App::registerCallback(callable)`**: Register a `std::function<void()>` and get an `MkuiActionId`.
- **`App::runConsole()`**: Start the real interactive console backend (Sprint 4 restored — pre-Sprint-4 round-7 shipped a stub).
- **`App::snapshotJson()`**: Canonical JSON snapshot of the tree (parity test fixture).

#### Enums

- **`mkui::ButtonVariant`**: Primary, Secondary, Destructive, Outline, Ghost, Link.
- **`mkui::TextVariant`**: Body, Heading1, Heading2, Heading3, Caption, Label, Code.

### C API (mkui_c.h)

#### Types

- **`MkuiApp*`**: Opaque application handle.
- **`MkuiNodeId`**: `(index, generation)` pair identifying a node — generation guards against use-after-free.
- **`MkuiActionId`**: Same shape, separate arena, identifies an action callback.
- **`MkuiResult`**: Operation result with `MkuiErrorCode` + message string.

#### Functions

- **`mkui_app_new()`** / **`mkui_app_free(app)`**: Lifecycle.
- **`mkui_app_root(app)`**: Return root `NodeId`.
- **`mkui_app_view_child(app, parent, class_name)`**: Append view under `parent`.
- **`mkui_app_text_child(app, parent, content, variant, class_name)`**: Append text.
- **`mkui_app_button_child(app, parent, label, variant, class_name, on_press)`**: Append button.
- **`mkui_app_register_callback(app, fn, user_data)`**: Register C callback, returns `MkuiActionId`.
- **`mkui_app_run_console(app)`**: Run the real interactive console backend.
- **`mkui_app_snapshot_json(app)`**: Allocates a JSON snapshot string (host frees with `mkui_free_error_message`).

## Styling

Both C and C++ APIs support CSS-like class names for styling:

```cpp
auto root = app->root();
auto row = app->viewChild(root, "flex items-center justify-between");
app->textChild(row, "Title", mkui::TextVariant::Heading1,
               "text-4xl font-bold text-center");
app->buttonChild(row, "Submit", mkui::ButtonVariant::Primary, "px-4 py-2");
```

Common classes:
- **Layout**: `flex`, `grid`, `container`, `mx-auto`
- **Spacing**: `p-4`, `px-2`, `py-1`, `m-4`, `gap-2`
- **Typography**: `text-xl`, `font-bold`, `text-center`
- **Colors**: `text-primary`, `bg-secondary`, `border-gray`

## Error Handling

### C++
```cpp
try {
    app->runConsole();
} catch (const mkui::MkuiException& e) {
    std::cout << "Error: " << e.what() << " (code: " << e.code() << ")" << std::endl;
}
```

### C
```c
MkuiResult result = mkui_app_run_console(app);
if (result.code != MKUI_SUCCESS) {
    printf("Error: %s\n", result.message);
    mkui_free_error_message((char*)result.message);
}
```

## Platform Support

| Platform | Status | Notes |
|----------|---------|-------|
| Linux    | ✅ Supported | Tested on Ubuntu, Debian, Fedora |
| macOS    | ✅ Supported | Requires Xcode command line tools |
| Windows  | 🚧 Planned | MSVC and MinGW support coming soon |

## Examples

See the `examples/` directory for complete working examples:
- [`c-example/`](../examples/c-example/): Simple C application
- [`cpp-example/`](../examples/cpp-example/): Modern C++ application

## Contributing

Contributions are welcome! Please see the main project [CONTRIBUTING.md](../CONTRIBUTING.md) for guidelines.

## License

This project is licensed under MIT OR Apache-2.0. See [LICENSE](../LICENSE) for details.