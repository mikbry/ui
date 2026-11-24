# Public web recipe browser smoke

This standalone consumer uses `mkui_web::Mkui`, shared `Card`/`Button` builders,
and `mkui_web::DEFAULT_COMPONENT_CSS`. It imports no showcase CSS or components
and requires no npm dependencies or Tailwind compilation.

From the repository root, using the Rust toolchain with the WASM target installed:

```sh
wasm-pack build examples/web-recipe-smoke --target web --dev --no-opt --out-dir pkg
python3 -m http.server 8000 --directory examples/web-recipe-smoke
```

Open `http://localhost:8000`. The page reports `PASS` (and sets
`document.documentElement.dataset.smoke` to `pass`) only after checking default
computed fill/foreground/border/radius/shadow, application CSS variable and class
overrides, child inheritance, disabled opacity, browser overlap hit ordering,
and click dispatch. Failure reports `FAIL` with the assertion and stack.
It uses `elementFromPoint` followed by native `.click()` to exercise browser hit
selection and native disabled semantics; this is not a trusted OS input test.

For an automated smoke, a headless Chromium invocation can use `--dump-dom
--virtual-time-budget=10000` against the served URL; require `data-smoke="pass"`
in its output. `pkg/` is generated and ignored. `cargo test -p mkui-web` is a
separate host-only contract suite and does not replace this browser check.

## Tailwind cascade regression

The separate CSS-only fixture compiles the public recipe with the showcase's
locked Tailwind 4 dependencies, then checks computed styles in headless Chrome.
It needs Node/npm and Chrome, but no WASM build or additional npm dependencies:

```sh
npm ci --prefix examples/web-showcase
node examples/web-recipe-smoke/check-cascade.mjs
```

Set `CHROME_BIN` to your Chromium executable if needed (defaults to Google
Chrome's standard macOS path or `google-chrome` on Linux). To reuse an existing
installation, set `TAILWIND_NODE_MODULES` to its absolute `node_modules` path.
The script copies CSS into a temporary directory and creates an absolute
`node_modules` symlink only there; it does not alter the dependency installation.

The checks cover the standalone Tailwind recipe, the actual showcase CSS entry
point (with normalized theme tokens), and the unchanged Tailwind-free recipe.
They assert Card's computed border/fill/foreground/radius/shadow, disabled
opacity and pointer behavior, theme variables, and consumer overrides from
unlayered rules, the `components` layer, and actual generated Tailwind utilities.
A negative control compiles the old recipe-before-Tailwind ordering and must
fail the Card border assertion, proving this regression is detected.

If a sandbox prevents Chrome subprocesses, run the command outside that sandbox.
Alternatively, `node examples/web-recipe-smoke/check-cascade.mjs --build-only`
prints and retains the compiled fixture directory. Open `tailwind.html`,
`showcase.html`, and `plain.html` there in Chrome; each must report `PASS` and
`data-smoke="pass"`. `regressed.html` must report
`FAIL: Error: Card border survives base reset`. These pages work with `file://`
and require no server or network. Remove the printed temporary directory after
inspection. Compilation alone is not a passing browser regression check.
