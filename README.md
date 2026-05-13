# miklabs/ui

*A modern, cross-platform UI toolkit for Rust, C, C++ & Python. Inspired by [shadcn/ui](https://ui.shadcn.com), [React Native](https://reactnative.dev), and [NativeWind](https://www.nativewind.dev).*

✨ Declarative • 🎨 Tailwind-style utilities • 🖼 Themeable • 🌍 Cross-platform • 🔧 Multi-language • ⚖️ MIT/Apache

---

## 🚀 Quick Example

### Rust

```rust
use showcase_common::create_showcase_ui;

fn main() -> std::io::Result<()> {
    // Create and run a cross-platform UI
    mkui::run!(create_showcase_ui, console)
}
```

Or build it step by step:

```rust
use mkui::prelude::*;

fn main() -> Result<(), MkuiError> {
    let app = Mkui::new()?
        .child(
            View::new()
                .class("flex-1 items-center justify-center")
                .child(Text::new("Hello World!").class("text-xl font-bold"))
                .child(
                    Button::new("Press me")
                        .variant(ButtonVariant::Primary)
                        .on_press(|| println!("Button pressed!"))
                )
        );
    
    app.run()
}
```

### C++

```cpp
#include "mkui.hpp"

int main() {
    try {
        auto app = mkui::createApp();
        
        app->addView("flex-1 items-center justify-center")
           .addText("Hello World!", "text-xl font-bold")
           .addButton("Press me", mkui::ButtonVariant::Primary)
           .runConsole();
           
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
    
    mkui_app_add_view(app, "flex-1 items-center justify-center");
    mkui_app_add_text(app, "Hello World!", "text-xl font-bold");
    mkui_app_add_button(app, "Press me", "", MKUI_BUTTON_PRIMARY);
    
    MkuiResult result = mkui_app_run_console(app);
    mkui_app_free(app);
    
    return result.code == MKUI_SUCCESS ? 0 : 1;
}
```

### Python

```python
import mkui_py

def main():
    try:
        # Create application using the high-level API
        app = mkui_py.create_app()
        
        # Build UI using method chaining
        (app.view("flex-1 items-center justify-center")
            .text("Hello World!", "text-xl font-bold")
            .button("Press me", mkui_py.BUTTON_PRIMARY)
            .run_console())
            
    except Exception as e:
        print(f"Error: {e}")
        return 1
    return 0

if __name__ == "__main__":
    exit(main())
```

---

## 🎨 Styling with Utility Classes

miklabs/ui uses a **Tailwind-like utility system** for layout, spacing, colors, and typography.

Examples:

* `flex-1`, `items-center`, `justify-between`
* `p-4`, `mt-2`, `gap-3`
* `rounded-lg`, `bg-surface`, `text-primary`

Variants work like shadcn’s **cva**:

```yaml
button:
  base: "inline-flex items-center justify-center rounded-lg transition"
  variants:
    intent:
      primary: "bg-primary text-primary-foreground hover:bg-primary/80"
      outline: "border border-input hover:bg-muted"
    size:
      sm: "h-8 px-3 text-sm"
      md: "h-10 px-4 text-base"
      lg: "h-12 px-6 text-lg"
  default:
    intent: "primary"
    size: "md"
```

---

## 🧭 Crate Layout

mkui is organized around a single contract crate that every backend consumes.
Backend-specific code never leaks into the contract.

```
crates/
  mkui-core       ← shared contract: component model, headless logic,
                    theme/layout/input/style/error contracts. Zero
                    backend dependencies — does not pull in wasm-bindgen,
                    crossterm, ratatui, wgpu, etc.
  mkui-web        ← web/WASM backend. Translates the shared component
                    tree into DOM elements via web-sys.
  mkui-console    ← terminal backend. Translates the shared component
                    tree into crossterm output.
  mkui-native     ← native (WGPU) backend. Walks the shared component
                    tree into draw records ready for a GPU scene.
  mkui            ← bridge: re-exports the backend chosen by Cargo
                    features and presents a single `Mkui` entry point.
  mkui-rsx        ← RSX/JSX-like macro (in progress).
  mkui-c          ← C/C++ FFI bindings over the `mkui` bridge.
  mkui-py         ← Python bindings via PyO3.
```

### What lives in `mkui-core`

- `components` — the renderable tree: `Component`, `View`, `Text`, `Button`.
- `headless` — pure-logic components (state, events, a11y traits) shared
  by every backend.
- `theme` — `Theme`, `ThemeMode`, `ColorTheme` (no platform colors).
- `layout` — `Layout`, `FlexDirection`, `Justify`, `Align`, `Edges`.
- `input` — `InputEvent`, `Key`, `PointerButton` (backend-neutral events).
- `style`, `event`, `state`, `error` — supporting contracts.

### What does **not** live in `mkui-core`

- DOM construction, `web-sys` / `wasm-bindgen` types → `mkui-web`.
- Terminal styles, crossterm/ratatui types → `mkui-console`.
- WGPU pipelines, scene transforms → `mkui-native`.

### Adding a new backend

A new backend is any crate that:

1. Depends on `mkui-core` (and only `mkui-core` from the contract side).
2. Consumes `mkui_core::components::Component` trees via `Any` downcasting.
3. Maps `theme::Theme` and `layout::Layout` values to its native styling.
4. Normalizes its native events into `mkui_core::input::InputEvent`.

If a contract change is needed (e.g. a new component type), it goes in
`mkui-core` so every backend keeps consuming the same model.

---

## 🌍 Target Platforms & Languages

### Platforms
* **Desktop**: Windows, macOS, Linux ✅
* **Console**: Terminal UIs with crossterm ✅
* **Web**: via WebAssembly ✅
* **Mobile**: iOS, iPadOS, Android 🚧 (planned)
* **Embedded**: WGPU on Vulkan/GL ES devices 🚧 (planned)

### Language Support
* **Rust**: Native support with full API ✅
* **C**: Complete FFI bindings with manual memory management ✅
* **C++**: Modern C++17 wrapper with RAII and exceptions ✅
* **Python**: PyO3 bindings with method chaining and exception handling ✅
* **JavaScript/TypeScript**: WASM bindings 🚧 (planned)

---

## 📊 Comparison

### Qt

```cpp
QApplication app(argc, argv);
QWidget window;
QPushButton *button = new QPushButton("Click me", &window);
QObject::connect(button, &QPushButton::clicked, []() { qDebug() << "Clicked!"; });
window.show();
return app.exec();
```

### miklabs/ui

```cpp
#include "mkui.hpp"

int main() {
    try {
        auto app = mkui::createApp();
        app->addButton("Click me", mkui::ButtonVariant::Primary, "px-4 py-2 rounded-lg")
           .runConsole();
    } catch (const mkui::MkuiException& e) {
        std::cout << "Clicked!\n";
    }
}
```

✅ Fewer lines, declarative, and styled with utilities.

---

## 🎮 Try the Showcases

Experience miklabs/ui across different platforms and languages:

### Rust Showcases

**Console Showcase** - Terminal UI with crossterm
```bash
cargo run --bin console-showcase
# Navigate: ↑↓←→  |  Interact: Space/Enter  |  Quit: q/Esc
```

**Web Showcase** - WebAssembly in browser
```bash
cd examples/web-showcase
wasm-pack build --target web
# Serve the generated files with any static server
```

**Headless Showcase** - Pure logic without rendering
```bash
cargo run --bin headless-showcase
```

### C/C++ Examples

**C Example** - Manual memory management
```bash
cd examples/c-example
make run
```

**C++ Example** - Modern RAII with exceptions
```bash
cd examples/cpp-example  
make run
```

### Python Example

**Python Example** - PyO3 bindings with exception handling
```bash
cd examples/python-example

# Build the Python bindings first
cd ../../crates/mkui-py
uv venv && source .venv/bin/activate
maturin develop --release

# Run the Python example
cd ../../examples/python-example
python main.py
```

### Key Features Demonstrated

- ✅ **Unified API**: Same UI code works across console, web, and native
- ✅ **Error Handling**: Proper error propagation with platform-specific types
- ✅ **Memory Safety**: Automatic cleanup in Rust/C++, manual in C
- ✅ **Styling**: Tailwind-like utility classes work everywhere
- ✅ **Components**: Views, Text, Buttons with multiple variants

---

## 🔮 Roadmap

### ✅ Phase 1: Foundation (COMPLETED)
* ✅ **Multi-language Support** - Rust, C, C++, and Python APIs with unified interface
* ✅ **Cross-platform Core** - Abstract error handling and component system  
* ✅ **Builder API** - Fluent builder pattern with method chaining
* ✅ **Bridge Architecture** - Unified `mkui` crate with feature-based platform switching
* ✅ **Headless Components** - Platform-agnostic logic (state, variants, events)
  * ✅ Button component with 6 variants and state management
  * ✅ Text component with styling and content management  
  * ✅ View component for layout and containers
* ✅ **Console Renderer** - Terminal UI with crossterm (no ratatui dependency)
* ✅ **Web Renderer** - DOM-based with WebAssembly support
* ✅ **C/C++ Bindings** - Complete FFI layer with modern C++17 wrapper
* ✅ **Python Bindings** - PyO3-based bindings with method chaining and exception handling
* ✅ **Showcases** - Working examples across all supported languages and platforms
* ✅ **Memory Management** - RAII for C++/Rust, manual cleanup for C
* ✅ **Error Handling** - Unified abstract errors with platform-specific conversion
* ✅ **Macro System** - `mkui::run!` macro for platform-agnostic application execution

### 🚧 Phase 2: Advanced Features (IN PROGRESS)
* [ ] **RSX Macro** - JSX-like syntax for declarative UI construction
* [ ] **Layout Engine** - Flexbox/CSS Grid-like layout with Taffy integration
* [ ] **Advanced Components** - Input fields, Select, Dialog, Popover, Menu
* [ ] **Accessibility** - Screen reader support, keyboard navigation, ARIA attributes  
* [ ] **Theme System** - Live theme switching, custom color palettes
* [ ] **Hot Reload** - Development-time live reloading for rapid iteration
* [ ] **Testing Framework** - Component testing utilities and assertions

### 🔮 Phase 3: Platform Expansion (PLANNED)  
* [ ] **Native Desktop** - WGPU backend for Windows, macOS, Linux
* [ ] **Mobile Support** - iOS and Android with platform-specific integrations
* [ ] **Language Bindings** - JavaScript/TypeScript (WASM), Go (CGO)
* [ ] **IDE Integration** - Language servers, syntax highlighting, code completion
* [ ] **Component Marketplace** - Shared component ecosystem and package manager
* [ ] **Production Tools** - Profiling, debugging, performance analysis

### 🎯 Current Focus (v0.2.0)
1. **RSX Syntax** - Investigate proc macro for JSX-like syntax
2. **Layout Engine** - Integrate Taffy for cross-platform layout
3. **Component Library** - Expand beyond basic View/Text/Button  
4. **Documentation** - Comprehensive guides and API references
5. **Testing** - Unit tests, integration tests, example validation

---

## 💬 Community

* Website: [miklabs.com/ui](https://miklabs.com/ui) (coming soon)
* GitHub Discussions: [github.com/miklabs/ui/discussions](https://github.com/miklabs/ui/discussions)
* Discord: *coming soon*

---

## ⚖️ License

MIT or Apache 2.0 (permissive, no GPL headaches).
