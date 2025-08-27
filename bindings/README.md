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
        
        app->addView("container")
           .addText("Hello, World!", "text-xl font-bold")
           .addButton("Click me!", mkui::ButtonVariant::Primary);
           
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
    
    mkui_app_add_view(app, "container");
    mkui_app_add_text(app, "Hello, World!", "text-xl font-bold");
    mkui_app_add_button(app, "Click me!", "", MKUI_BUTTON_PRIMARY);
    
    MkuiResult result = mkui_app_run_console(app);
    mkui_app_free(app);
    
    return result.code == 0 ? 0 : 1;
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

- **`App::addView(className)`**: Add a container/layout view
- **`App::addText(content, className)`**: Add text content
- **`App::addButton(text, variant, className)`**: Add interactive button
- **`App::runConsole()`**: Start the console application

#### Enums

- **`mkui::ButtonVariant`**: Primary, Secondary, Destructive, Outline, Ghost, Link

### C API (mkui_c.h)

#### Types

- **`MkuiApp*`**: Opaque application handle
- **`MkuiResult`**: Operation result with error code and message
- **`MkuiErrorCode`**: Error code enumeration

#### Functions

- **`mkui_app_new()`**: Create new application
- **`mkui_app_free(app)`**: Free application resources
- **`mkui_app_add_view(app, class_name)`**: Add view component
- **`mkui_app_add_text(app, content, class_name)`**: Add text component
- **`mkui_app_add_button(app, text, class_name, variant)`**: Add button component
- **`mkui_app_run_console(app)`**: Run console application

## Styling

Both C and C++ APIs support CSS-like class names for styling:

```cpp
app->addText("Title", "text-4xl font-bold text-center")
   .addView("flex items-center justify-between")
   .addButton("Submit", mkui::ButtonVariant::Primary, "px-4 py-2");
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