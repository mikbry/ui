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
