# mkui-web component recipes

`mkui-web` renders `Card` as `<div class="card ...">` and `Button` as a
native `<button class="btn ...">`. Applications own the theme and may supply
all CSS themselves. Rendering does not install CSS or change theme variables.

For default Card chrome and disabled Button styling, import the public
[`styles.css`](styles.css) before your application styles, or embed the exact
same stylesheet from Rust. When using reset/base layers such as Tailwind
Preflight, declare the layer order first as shown below.

```rust
fn install_component_recipe() -> Result<(), wasm_bindgen::JsValue> {
    let document = web_sys::window().unwrap().document().unwrap();
    let style = document.create_element("style")?;
    style.set_text_content(Some(mkui_web::DEFAULT_COMPONENT_CSS));
    document.head().unwrap().append_child(&style)?;
    Ok(())
}
```

The recipe needs no Tailwind build or showcase assets. It defines Card's fill,
foreground, 1px border, radius, and shadow, plus disabled Button opacity and
pointer behavior. Other layout utilities and Button variant styles remain
application-owned. No global reset, palette, padding, or typography is installed.

Card reads `--card`, `--card-foreground`, `--border`, and `--radius` as complete
CSS values, with neutral fallbacks. Set them on the app root or a card; unstyled
children inherit the foreground. The `mkui-defaults` cascade layer and
`:where()` selectors let application classes override defaults, even when those
unlayered classes appear earlier in the stylesheet. For example:

```css
.my-app { --card: #202024; --card-foreground: #fafafa; --radius: 12px; }
.featured-card { border-color: gold; }
```

Use `Card::new().class("featured-card")` to retain both classes. A consumer using
its own cascade layers should declare `mkui-defaults` after reset/base layers
and before its override layers.

With Tailwind 4, establish this order **before any imports or injected styles
first declare the layers** (adjust the recipe path for your application):

```css
@layer theme, base, mkui-defaults, components, utilities;
@import "tailwindcss";
@import "../../crates/mkui-web/styles.css";
@import "./app.css";
```

This places the recipe above [Tailwind Preflight](https://tailwindcss.com/docs/preflight)
in `base`, which resets borders and button opacity, while keeping application
`components`, Tailwind `utilities`, and unlayered rules able to override it.
Importing the recipe before Tailwind without that declaration lets Preflight
erase the defaults; importing it after Tailwind without the declaration puts
defaults above utilities. The same ordering applies when embedding
`DEFAULT_COMPONENT_CSS` from Rust. Tailwind-free consumers need no additional
layer declaration unless they use their own reset or override layers.

Disabled buttons keep native `disabled` semantics and `pointer-events: auto`:
they dim and block pointer hits on their painted surface without activating.
Do not add `pointer-events: none` to disabled buttons when overlapping controls
must remain blocked. Scope custom hover effects to `:enabled:hover`.

The standalone [browser smoke fixture](../../examples/web-recipe-smoke/README.md)
checks actual computed styles, consumer overrides, inheritance, and overlapping
disabled/active button dispatch through the public web entry point. Its separate
[Tailwind cascade check](../../examples/web-recipe-smoke/README.md#tailwind-cascade-regression)
compiles the public import recipe and showcase CSS, checks browser computed
styles and overrides, and rejects the old ordering as a negative control.
