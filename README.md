# miklabs/ui

*A modern, cross-platform UI toolkit for Rust & C++. Inspired by [shadcn/ui](https://ui.shadcn.com), [React Native](https://reactnative.dev), and [NativeWind](https://www.nativewind.dev).*

✨ Declarative • 🎨 Tailwind-style utilities • 🖼 Themeable • 🌍 Cross-platform • ⚖️ MIT/Apache

---

## 🚀 Quick Example

### Rust

```rust
use mkui::prelude::*;

fn main() {
    ui! {
        <View class="flex-1 items-center justify-center bg-surface">
            <Text class="text-xl font-semibold text-primary">"Hello World"</Text>
            <Button class="mt-4 px-4 py-2 bg-primary rounded-lg" on:press=|| {
                println!("Pressed!");
            }>
                "Press me"
            </Button>
        </View>
    }
}
```

### C++

```cpp
#include <mkui/mkui.hpp>
using namespace mkui;

int main() {
    ui(R"RSX(
        <View class="flex-1 items-center justify-center bg-surface">
            <Text class="text-xl font-semibold text-primary">Hello World</Text>
            <Button class="mt-4 px-4 py-2 bg-primary rounded-lg" on:press="onPress">
                Press me
            </Button>
        </View>
    )RSX", props{
        {"onPress", [](){ std::cout << "Pressed!" << std::endl; }}
    });
}
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

## 🌍 Target Platforms

* **Desktop**: Windows, macOS, Linux
* **Mobile**: iOS, iPadOS, Android
* **Web**: via WebAssembly (WebGPU backend)
* **Embedded**: WGPU on Vulkan/GL ES devices

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

### miklabs/ui with mkui

```cpp
ui(R"RSX(
    <Button class="px-4 py-2 bg-primary rounded-lg" on:press="onPress">
        Click me
    </Button>
)RSX", props{
    {"onPress", [](){ std::cout << "Clicked!\n"; }}
});
```

✅ Fewer lines, declarative, and styled with utilities.

---

## 🔮 Roadmap

* [ ] Core renderer (WGPU backend)
* [ ] Layout engine (`View` with flexbox-like props)
* [ ] Core primitives (`Text`, `Button`, `Input`)
* [ ] Utility class system (spacing, colors, typography)
* [ ] Headless components (Dialog, Listbox, Menu, etc.)
* [ ] Theming + tokens
* [ ] Cross-platform builds (desktop → mobile → wasm → embedded)

---

## 💬 Community

* Website: [miklabs.com/ui](https://miklabs.com/ui) (coming soon)
* GitHub Discussions: [github.com/miklabs/ui/discussions](https://github.com/miklabs/ui/discussions)
* Discord: *coming soon*

---

## ⚖️ License

MIT or Apache 2.0 (permissive, no GPL headaches).
