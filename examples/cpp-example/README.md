# mkui C++ Example

This example demonstrates how to use mkui from C++ code with modern C++17 features to create a cross-platform console application.

## Features

- Modern C++17 implementation with RAII
- Exception-based error handling
- Method chaining for fluent API
- Smart pointers for automatic memory management
- Type-safe enum classes
- Cross-platform console UI

## Building

### Prerequisites

- Rust toolchain (to build the mkui library)
- C++17 compatible compiler (g++, clang++, MSVC)
- Make (optional)

### Using Make

```bash
# Build and run
make run

# Just build
make

# Clean
make clean
```

### Manual Build

```bash
# First, build the Rust library
cd ../..
cargo build --release -p mkui-c

# Then build the C++ example
cd examples/cpp-example
g++ -std=c++17 -Wall -Wextra -O2 \
    -I../../crates/mkui-c/target/include \
    -I../../bindings/cpp \
    main.cpp \
    -L../../target/release \
    -lmkui_c -ldl -lpthread -lm \
    -o mkui_cpp_example
```

## Code Structure

The example demonstrates modern C++ best practices:

1. **RAII Resource Management**: Automatic cleanup with destructors
2. **Exception Handling**: Type-safe error management
3. **Method Chaining**: Fluent API for building UI hierarchies
4. **Smart Pointers**: `std::unique_ptr` for memory safety
5. **Strong Types**: Enum classes for type safety

## Key Concepts

### RAII and Smart Pointers

```cpp
// Automatic memory management with smart pointers
auto app = mkui::createApp();  // std::unique_ptr<mkui::App>

// No need for manual cleanup - destructor handles it
// app goes out of scope automatically
```

### Exception Handling

```cpp
try {
    app->runConsole();
} catch (const mkui::MkuiException& e) {
    std::cerr << "mkui Error: " << e.what() 
              << " (code: " << e.code() << ")" << std::endl;
    return 1;
} catch (const std::exception& e) {
    std::cerr << "Error: " << e.what() << std::endl;
    return 1;
}
```

### Handle-based nested construction (Sprint 4)

```cpp
auto root = app->root();
auto container = app->viewChild(root, "container mx-auto px-4");
app->textChild(container, "Welcome!", mkui::TextVariant::Heading1,
               "text-2xl font-bold");
app->buttonChild(container, "Get Started", mkui::ButtonVariant::Primary);
app->buttonChild(container, "Learn More", mkui::ButtonVariant::Secondary);
```

Each child constructor takes an explicit parent `NodeId` and returns the
new child's id. The pre-Sprint-4 fluent `app->addView(...).addText(...)`
chaining is gone — see `main.cpp` for the canonical handle-based shape.

### Type-Safe Enums

```cpp
auto row = app->viewChild(app->root(), "flex gap-2");
app->buttonChild(row, "Submit", mkui::ButtonVariant::Primary);
app->buttonChild(row, "Cancel", mkui::ButtonVariant::Secondary);
app->buttonChild(row, "Delete", mkui::ButtonVariant::Destructive);
```

## API Reference

### Classes

#### `mkui::App`

RAII wrapper around `MkuiApp*`.

**Methods:**
- `root()` — root `NodeId`
- `viewChild(parent, className)` — append view
- `textChild(parent, content, variant, className)` — append text
- `buttonChild(parent, label, variant, className, onPress)` — append button (pass `mkui::kNoAction` for no callback)
- `registerCallback(std::function<void()>)` — register callable, returns `MkuiActionId`
- `runConsole()` — run real interactive console backend
- `snapshotJson()` — canonical JSON snapshot of the tree
- `static version()` — library version

#### `mkui::MkuiException` (`what()`, `code()`) — exception type.

### Enums

#### `mkui::ButtonVariant` — Primary / Secondary / Destructive / Outline / Ghost / Link.
#### `mkui::TextVariant` — Body / Heading1 / Heading2 / Heading3 / Caption / Label / Code.

## Example Usage Patterns

### Simple Application

```cpp
#include "mkui.hpp"

int main() {
    try {
        auto app = mkui::createApp();
        auto root = app->root();
        app->textChild(root, "Hello, World!",
                       mkui::TextVariant::Heading1, "text-xl");
        app->buttonChild(root, "OK", mkui::ButtonVariant::Primary);
        app->runConsole();
    } catch (const mkui::MkuiException& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }
    return 0;
}
```

### Complex Layout

```cpp
auto app = mkui::createApp();
auto root = app->root();

auto page = app->viewChild(root, "min-h-screen flex flex-col");

auto header = app->viewChild(page, "border-b");
app->textChild(header, "My Application", mkui::TextVariant::Heading1,
               "text-2xl font-bold");

auto content = app->viewChild(page, "flex-1 container mx-auto px-4 py-8");
app->textChild(content, "Welcome to mkui!", mkui::TextVariant::Heading1,
               "text-4xl font-bold text-center");
app->textChild(content, "Cross-platform UI library",
               mkui::TextVariant::Caption,
               "text-xl text-muted-foreground text-center");

auto actions = app->viewChild(content, "flex gap-4 justify-center");
app->buttonChild(actions, "Get Started", mkui::ButtonVariant::Primary);
app->buttonChild(actions, "Documentation", mkui::ButtonVariant::Outline);
```

## CSS Styling

The C++ API supports the same Tailwind-style classes as other mkui implementations:

### Layout Classes
```cpp
app->viewChild(parent, "flex flex-col items-center justify-center");
app->viewChild(parent, "container mx-auto max-w-4xl");
```

### Typography Classes
```cpp
app->textChild(parent, "Title", mkui::TextVariant::Heading1,
               "text-4xl font-bold");
app->textChild(parent, "Subtitle", mkui::TextVariant::Caption,
               "text-xl text-muted-foreground");
app->textChild(parent, "Body", mkui::TextVariant::Body,
               "text-base leading-none");
```

### Spacing Classes
```cpp
app->viewChild(parent, "p-6 mb-4");      // padding + bottom margin
app->viewChild(parent, "px-4 py-6");     // horizontal + vertical padding
app->viewChild(parent, "space-y-8");     // vertical gap between children
```

## Running

```bash
./mkui_cpp_example
```

Use arrow keys to navigate, Enter to interact with buttons, and 'q' to quit.

## Memory Management

The C++ wrapper handles all memory management automatically:

- **Automatic Cleanup**: RAII ensures resources are freed
- **Exception Safety**: Resources cleaned up even if exceptions are thrown
- **Move Semantics**: Efficient transfer of ownership
- **Smart Pointers**: Automatic lifetime management

## Troubleshooting

### Compiler Issues

1. **C++17 Required**: Use `-std=c++17` or newer
2. **Missing Headers**: Ensure both C and C++ headers are in include path
3. **Linking**: Make sure to link against the mkui_c library

### Runtime Issues

1. **Library Not Found**: Check `LD_LIBRARY_PATH` or equivalent
2. **Terminal Issues**: Some terminals may not support all features
3. **Build Configuration**: Ensure library and example are built with compatible settings