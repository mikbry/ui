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

### Method Chaining

```cpp
app->addView("flex-1")
   .addView("container mx-auto px-4")
   .addText("Welcome!", "text-2xl font-bold")
   .addButton("Get Started", mkui::ButtonVariant::Primary)
   .addButton("Learn More", mkui::ButtonVariant::Secondary);
```

### Type-Safe Enums

```cpp
// Strongly typed enum class prevents invalid values
app->addButton("Submit", mkui::ButtonVariant::Primary)
   .addButton("Cancel", mkui::ButtonVariant::Secondary)
   .addButton("Delete", mkui::ButtonVariant::Destructive);
```

## API Reference

### Classes

#### `mkui::App`

The main application class with RAII semantics.

**Methods:**
- `addView(className = "")` - Add container/layout view
- `addText(content, className = "")` - Add text content  
- `addButton(text, variant = Primary, className = "")` - Add button
- `runConsole()` - Start console application
- `static version()` - Get library version

**Static Functions:**
- `mkui::createApp()` - Create new application (returns `std::unique_ptr<App>`)

#### `mkui::MkuiException`

Exception class for error handling.

**Methods:**
- `what()` - Get error message
- `code()` - Get error code

### Enums

#### `mkui::ButtonVariant`

- `Primary` - Primary action button
- `Secondary` - Secondary action button
- `Destructive` - Destructive action (delete, etc.)
- `Outline` - Outlined button
- `Ghost` - Minimal ghost button
- `Link` - Link-style button

## Example Usage Patterns

### Simple Application

```cpp
#include "mkui.hpp"

int main() {
    try {
        auto app = mkui::createApp();
        
        app->addText("Hello, World!", "text-xl")
           .addButton("OK", mkui::ButtonVariant::Primary);
           
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

app->addView("min-h-screen flex flex-col")
   // Header
   .addView("bg-header border-b")
   .addView("container mx-auto px-4 py-2")
   .addText("My Application", "text-2xl font-bold")
   
   // Main content
   .addView("flex-1 container mx-auto px-4 py-8")
   .addView("max-w-4xl mx-auto space-y-8")
   
   // Content sections
   .addText("Welcome to mkui!", "text-4xl font-bold text-center")
   .addText("Cross-platform UI library", "text-xl text-gray-600 text-center")
   
   // Actions
   .addView("flex gap-4 justify-center")
   .addButton("Get Started", mkui::ButtonVariant::Primary)
   .addButton("Documentation", mkui::ButtonVariant::Outline)
   
   // Footer
   .addView("border-t bg-gray-50 py-4")
   .addText("© 2024 mkui", "text-center text-gray-500");
```

## CSS Styling

The C++ API supports the same Tailwind-style classes as other mkui implementations:

### Layout Classes
```cpp
.addView("flex flex-col items-center justify-center")
.addView("grid grid-cols-2 gap-4")
.addView("container mx-auto max-w-4xl")
```

### Typography Classes
```cpp
.addText("Title", "text-4xl font-bold")
.addText("Subtitle", "text-xl font-medium text-gray-600")
.addText("Body", "text-base leading-relaxed")
```

### Spacing Classes
```cpp
.addView("p-4 m-2")           // padding and margin
.addView("px-8 py-4")         // horizontal/vertical padding
.addView("space-y-6")         // vertical spacing between children
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