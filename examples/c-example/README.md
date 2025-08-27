# mkui C Example

This example demonstrates how to use mkui from C code to create a cross-platform console application.

## Features

- Pure C implementation using the mkui C API
- Cross-platform console UI
- Error handling with detailed messages
- Memory management with proper cleanup
- Multiple UI components (views, text, buttons)

## Building

### Prerequisites

- Rust toolchain (to build the mkui library)
- C compiler (gcc, clang, etc.)
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

# Then build the C example
cd examples/c-example
gcc -std=c11 -Wall -Wextra -O2 \
    -I../../crates/mkui-c/target/include \
    main.c \
    -L../../target/release \
    -lmkui_c -ldl -lpthread -lm \
    -o mkui_c_example
```

## Code Structure

The example creates a complete UI application with:

1. **Application initialization**: `mkui_app_new()`
2. **UI component hierarchy**:
   - Main container view
   - Header with title and subtitle
   - Hero section with large text
   - Button showcase with different variants
3. **Error handling**: Comprehensive error checking
4. **Resource cleanup**: Proper memory management

## Key Concepts

### Error Handling

```c
void check_result(MkuiResult result, const char* operation) {
    if (result.code != 0) {
        fprintf(stderr, "Error in %s: %s\n", operation, result.message);
        if (result.message) {
            mkui_free_error_message((char*)result.message);
        }
        exit(1);
    }
}
```

### Component Creation

```c
// Add a view (container/layout)
result = mkui_app_add_view(app, "flex-1 container mx-auto");
check_result(result, "adding main container");

// Add text content
result = mkui_app_add_text(app, "Hello, World!", "text-xl font-bold");
check_result(result, "adding title text");

// Add interactive button
result = mkui_app_add_button(app, "Click me!", "", MKUI_BUTTON_PRIMARY);
check_result(result, "adding primary button");
```

### Resource Management

```c
// Create application
MkuiApp* app = mkui_app_new();
if (!app) {
    fprintf(stderr, "Failed to create mkui application\n");
    return 1;
}

// ... use application ...

// Always clean up
mkui_app_free(app);
```

## Button Variants

The example demonstrates all button variants:

- `MKUI_BUTTON_PRIMARY` - Primary action button
- `MKUI_BUTTON_SECONDARY` - Secondary action button  
- `MKUI_BUTTON_DESTRUCTIVE` - Destructive action (delete, etc.)
- `MKUI_BUTTON_OUTLINE` - Outlined button
- `MKUI_BUTTON_GHOST` - Minimal ghost button
- `MKUI_BUTTON_LINK` - Link-style button

## CSS Classes

The example uses Tailwind-style CSS classes for styling:

- **Layout**: `flex-1`, `container`, `mx-auto`
- **Spacing**: `px-4`, `py-8`, `mb-12`, `gap-4`
- **Typography**: `text-xl`, `text-4xl`, `font-bold`, `font-semibold`
- **Colors**: `text-foreground`, `text-muted-foreground`
- **Borders**: `border-b`, `rounded-lg`, `border`

## Running

```bash
./mkui_c_example
```

The application will display a terminal-based UI with navigation using arrow keys, Enter to interact, and 'q' to quit.

## Troubleshooting

### Build Issues

1. **Missing header**: Make sure to build the Rust library first
2. **Linking errors**: Check library paths and ensure all dependencies are available
3. **Runtime errors**: Verify the library was built with the correct features

### Platform-Specific Notes

- **Linux**: May need to install `build-essential` package
- **macOS**: Requires Xcode command line tools
- **Windows**: MSVC or MinGW-w64 required (coming soon)