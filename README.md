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
        auto root = app->root();
        auto container = app->viewChild(root, "flex-1 items-center justify-center");
        app->textChild(container, "Hello World!",
                       mkui::TextVariant::Heading1, "text-xl font-bold");
        app->buttonChild(container, "Press me", mkui::ButtonVariant::Primary);
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
    MkuiNodeId container = mkui_app_view_child(app, root,
                                               "flex-1 items-center justify-center");
    mkui_app_text_child(app, container, "Hello World!",
                        MKUI_TEXT_HEADING_1, "text-xl font-bold");
    mkui_app_button_child(app, container, "Press me", MKUI_BUTTON_PRIMARY, "",
                          (MkuiActionId){UINT32_MAX, UINT32_MAX});

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
        app = mkui_py.App()
        root = app.root()
        container = app.view_child(root, "flex-1 items-center justify-center")
        app.text_child(container, "Hello World!",
                       mkui_py.TEXT_HEADING_1, "text-xl font-bold")
        app.button_child(container, "Press me", mkui_py.BUTTON_PRIMARY)
        app.run_console()
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

| Crate         | Responsibility                                                                                                                  | Maturity                                                      |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------- |
| `mkui-core`   | Shared contract: component model, headless logic, theme / layout / input / style / error. Zero backend deps.                    | Stable                                                        |
| `mkui-text`   | Text-system trait + from-scratch bitmap prototype. No external text-stack deps (no cosmic-text / swash / freetype / fontdue).   | Experimental (bitmap prototype, trait stable)                 |
| `mkui-wgpu`   | WGPU scene primitives + declarative builders + `winit` `ApplicationHandler` shell. Backs the HUD-style 2D pipeline.             | Experimental (shipping; declarative `AppTree` bridge per ADR 0006) |
| `mkui-web`    | Web/WASM backend. Translates the shared component tree into DOM via `web-sys`.                                                  | Stable                                                        |
| `mkui-console`| Terminal backend. Translates the shared component tree into `crossterm` output.                                                 | Stable                                                        |
| `mkui-native` | Scene-walker contract for native backends — collects `mkui-core` component trees into draw records that `mkui-wgpu` can render. | Experimental                                                  |
| `mkui`        | Bridge crate. Re-exports the backend chosen by Cargo features and presents a single `Mkui` entry point.                         | Stable                                                        |
| `mkui-rsx`    | RSX/JSX-like macro.                                                                                                             | Placeholder                                                   |
| `mkui-runtime`| Portable application-tree substrate (`AppTree`, `NodeId`, `ActionId`, class parser, JSON snapshots). Every binding builds into this same arena. | Stable (Sprint 4) |
| `mkui-c`      | C/C++ FFI bindings — handle-based nested API over `mkui-runtime`'s `AppTree`. CI builds + clippy gates active.                  | Stable (Sprint 4)                                             |
| `mkui-py`     | Python bindings via PyO3 0.28.3. Handle-based nested API.                                                                       | CI parity job active; full-matrix re-entry tracked in [#53](https://github.com/mikbry/ui/issues/53) (Python 3.14 + PyO3 0.28) |

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
- WGPU pipelines, scene transforms, winit shell → `mkui-wgpu`.
- Text layout / rasterization → `mkui-text`.

### Adding a new backend

A new backend is any crate that:

1. Depends on `mkui-core` (and only `mkui-core` from the contract side).
2. Consumes `mkui_core::components::Component` trees via `Any` downcasting.
3. Maps `theme::Theme` and `layout::Layout` values to its native styling.
4. Normalizes its native events into `mkui_core::input::InputEvent`.

If a contract change is needed (e.g. a new component type), it goes in
`mkui-core` so every backend keeps consuming the same model.

---

## 📐 Architecture

The load-bearing architectural decisions behind the workspace shape above
are documented as ADRs (Architecture Decision Records) under
[`docs/architecture/`](docs/architecture/README.md):

- [ADR 0001 — `mkui-core` as the contract crate](docs/architecture/0001-mkui-core-as-contract-crate.md)
- [ADR 0002 — `mkui-text` owns the stack (no external Rust text crates)](docs/architecture/0002-mkui-text-own-the-stack.md)
- [ADR 0003 — `mkui-web` registry-based extension](docs/architecture/0003-mkui-web-registry-based-extension.md)
- [ADR 0004 — `mkui-wgpu` 2D HUD pipeline port](docs/architecture/0004-mkui-wgpu-2d-hud-pipeline-port.md)
- [ADR 0005 — `mkui-runtime` as the portable AppTree substrate](docs/architecture/0005-mkui-runtime-portable-substrate.md)
- [ADR 0006 — `mkui-wgpu` declarative bridge over `mkui-runtime::AppTree`](docs/architecture/0006-wgpu-declarative-bridge.md)

New contributors and reviewers should start with the
[ADR index](docs/architecture/README.md) for the format conventions and
one-sentence summaries.

### Threading model

**mkui is single-threaded by design.** The component tree (`AppTree`) and the
`ActionRegistry` must be built, mutated, and driven from a **single thread** —
they are intentionally `!Send + !Sync`. Actions are stored as
`Rc<RefCell<…>>`, so the tree and its callbacks never cross a thread boundary.
This is a deliberate invariant, not an oversight: adding `Send + Sync` bounds
prematurely would force every binding (Rust, C, Python) to thread those bounds
through closures that never actually cross threads.

Where the boundary sits:

- **The host may be multithreaded.** Your application can run any number of
  threads for I/O, compute, or networking.
- **mkui's tree must be driven from one thread.** All `AppTree` /
  `ActionRegistry` access — construction, mutation, event dispatch, and the
  `mkui-wgpu` render walk — has to happen on that one owning thread (typically
  the main / UI thread). Marshal data from worker threads back to the UI
  thread before touching the tree.
- **Errors stay local too.** `MkuiError` is `Send + Sync` on native targets so
  results can flow across spawned tasks, but the WASM `JsValue` variant is
  `!Send + !Sync` because errors there are local to the single-threaded WASM
  context.

A future host (Python, C, …) wanting to drive UI from a non-main thread should
treat this as a hard constraint today. The design rationale lives in
[ADR 0005](docs/architecture/0005-mkui-runtime-portable-substrate.md) (the
`ActionRegistry` single-threaded decision) and
[ADR 0006](docs/architecture/0006-wgpu-declarative-bridge.md) (the declarative
bridge over `AppTree`). Cross-thread support is explicitly out of scope until a
real multithreaded runtime exists.

---

## 🌍 Target Platforms & Languages

### Platforms
* **Desktop**: Windows, macOS, Linux ✅
* **Console**: Terminal UIs with crossterm ✅
* **Web**: via WebAssembly ✅
* **Native WGPU**: HUD-style 2D scene pipeline + `winit` `ApplicationHandler` shell shipped in `mkui-wgpu`. Bitmap-text fallback is the current default; richer text rendering is on the roadmap, tracked in project issues.
* **Mobile**: iOS, iPadOS, Android 🚧 (planned)

### Language Support
* **Rust**: Native support with full ergonomic API ✅
* **C**: FFI bindings — handle-based nested API on the `mkui-runtime` substrate; CI build + clippy gates active ✅
* **C++**: Modern C++17 wrapper (RAII + exceptions) over the C handle API ✅
* **Python**: PyO3 0.28.3 bindings — handle-based API on the same substrate; Python 3.14 + PyO3 0.28 compatibility tracked in [#53](https://github.com/mikbry/ui/issues/53)
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
        auto on_click = app->registerCallback([]() { std::cout << "Clicked!\n"; });
        app->buttonChild(app->root(), "Click me",
                         mkui::ButtonVariant::Primary, "px-4 py-2 rounded-lg",
                         on_click);
        app->runConsole();
    } catch (const mkui::MkuiException& e) {
        std::cerr << e.what() << "\n";
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

### Native Window Example

**Native Window** — minimal `mkui-wgpu` smoke: opens a `winit` window via the
`ApplicationHandler` shell and paints a clear color + a single quad through
the HUD `Scene` API. Any visual regression in the HUD pipeline shows up here.

```bash
cargo run --example native-window
```

### Python Example

> ⚠ **`mkui-py` requires Python 3.13** — Python 3.14 + PyO3 0.28
> compatibility is tracked in [#53](https://github.com/mikbry/ui/issues/53).
> Use Python 3.13 if you specifically need the Python bindings; otherwise
> build the workspace with `--exclude mkui-py`.

**Python Example** - PyO3 bindings with exception handling (Python 3.13)
```bash
cd examples/python-example

# Build the Python bindings first
cd ../../crates/mkui-py
uv venv --python 3.13 && source .venv/bin/activate
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

## 🧪 Local Verification

Before opening a PR, run the workspace checks. `mkui-py` needs a local
PyO3 / Python 3.13 toolchain — Python 3.14 + PyO3 0.28 compatibility is
tracked in [#53](https://github.com/mikbry/ui/issues/53) — so the everyday
loop excludes it. `mkui-c` is no longer excluded: the Sprint 4 handle-based
rewrite added `// SAFETY:` annotations on every `unsafe` block and `mkui-c`
re-entered the CI matrix (build + clippy + test) as of v0.5.0.

```bash
# Everyday loop (no PyO3 toolchain required)
cargo build   --workspace --exclude mkui-py
cargo test    --workspace --exclude mkui-py
cargo clippy  --workspace --exclude mkui-py --all-targets -- -D warnings
cargo fmt     --all -- --check

# Full workspace (requires Python 3.13 + maturin for mkui-py)
cargo build   --workspace
cargo test    --workspace
```

Backend-specific feature checks for the bridge crate:

```bash
cargo test -p mkui                       # default: no backend, verifies init-error path
cargo test -p mkui --features console    # console backend smoke
```

Native WGPU smoke (opens a winit window, paints a single quad via the HUD
`Scene` API):

```bash
cargo run --example native-window
```

The web backend is exercised by `examples/web-showcase` via `wasm-pack`;
the bridge-crate `cargo test` runs cover the contract + dispatch surface only.

---

## 🔮 Current Capabilities & Direction

### ✅ Current capabilities (v0.8.0)

- **Shared contract** — `mkui-core` component model (`View` / `Text` /
  `Button`), headless state/variant logic, theme / layout / input contracts.
- **Console backend** — `mkui-console`, crossterm-driven terminal UI.
- **Web backend** — `mkui-web`, DOM construction via `web-sys` / WebAssembly.
- **Native WGPU pipeline** — `mkui-wgpu` ships a HUD-style 2D scene API
  (quads, panels, hit regions, theme-aware variant resolvers) plus a
  `winit` `ApplicationHandler` shell so a native window is one call away.
- **Text system** — `mkui-text` defines a `TextSystem` trait with a
  from-scratch bitmap implementation (`BitmapTextSystem`). No external
  text-stack dependencies. The bitmap path is the current default and
  stays as the permanent debug-fallback / visual-regression oracle.
- **First shadcn-aligned atoms** — `Badge` (6 variants) and `Dot` (status
  variants + halo + animation modifiers) in `mkui-wgpu`.
- **CI** — `fmt`, `clippy -D warnings`, `test`, and release build fully
  gated.

### 🚧 Active direction

- **Component surface** — expand the shadcn-aligned atom set on top of
  `mkui-wgpu`.
- **Layout engine** — flexbox-style layout integration for the shared
  contract.
- **RSX macro** — `mkui-rsx` is a placeholder; JSX-like authoring is the
  target.
- **Native text rendering** — extending `mkui-text` beyond the bitmap
  prototype is on the roadmap. The specific approach is internal; the
  trait surface is the public contract.
- **FFI hardening** — `mkui-c` is gated in CI (build + clippy + test) with
  `// SAFETY:` annotations on every `unsafe` block since the Sprint 4
  handle-based rewrite; `mkui-py` requires a local Python 3.13 toolchain
  until Python 3.14 + PyO3 0.28 support lands
  ([#53](https://github.com/mikbry/ui/issues/53)).

### 🔮 Longer-horizon

- Mobile (iOS / Android) backends.
- JavaScript / TypeScript bindings.
- Accessibility, theming polish, hot reload.

mkui is an open UI framework that drives its own internal work first;
public roadmap detail tracks shipped capabilities rather than aspirational
plans. Sprint-by-sprint direction beyond what's in this list lives in
project issues, not the README.

---

## 📜 Releases

See [CHANGELOG.md](CHANGELOG.md) for release notes.

---

## 💬 Community

* Website: [miklabs.com/ui](https://miklabs.com/ui) (coming soon)
* GitHub Discussions: [github.com/miklabs/ui/discussions](https://github.com/miklabs/ui/discussions)
* Discord: *coming soon*

---

## ⚖️ License

MIT or Apache 2.0 (permissive, no GPL headaches).
