# Architecture Audit Report — `mkui` workspace

- **Project:** ui (`miklabs/ui`, `mkui`)
- **Path:** `/Users/mik/dev/mikbry/ui`
- **Date:** 2026-05-20
- **Workspace version:** `0.3.0`
- **Audit scope:** all 9 library crates + 4 example crates; checklist is the full 10-category Rust review.
- **Verification:** `cargo test --workspace --exclude mkui-py --exclude mkui-c` (79 passed, 2 ignored, 0 failed) and `cargo clippy --workspace --exclude mkui-py --all-targets` (11 errors in `mkui-c`, ~31 warnings).

---

## Executive Summary

**Overall Health: 6.4 / 10 — Fair, with strong architectural fundamentals undermined by tooling debt.**

After Sprint 1 the workspace has a genuinely good *shape*: `mkui-core` is a clean, dependency-light contract crate; every backend (`mkui-web`, `mkui-console`, `mkui-wgpu`, `mkui-native`) follows the same five-module template (`app` / `renderer` / `components` / `high_level` / `prelude`); the new `WebRendererRegistry` (PR #14) and the console tree walker (PR #13) have replaced the previous closed-set downcasting with `TypeId`-keyed dispatch that downstream crates can extend. Documentation at the crate level is well above average — every `lib.rs` carries a non-trivial design note explaining boundary discipline. Smoke tests cover the right contract-level invariants (registry dispatch, handler retention through `Rc`, layout-flattening fidelity).

The audit's main concerns are not architectural; they are *operational*. Clippy fails the workspace today: `mkui-c` has 11 hard errors (`clippy::not_unsafe_ptr_arg_deref`) that mean the FFI surface is **unsound by Rust's own standards**, and `mkui-core` ships shadowing of `std` traits (`from_str`, inherent `to_string`) that will break consumers as soon as they `use std::str::FromStr`. There is no CI configuration in the repo (`.github/` is absent), so none of these regressions are caught before merge. `mkui-py` is broken on Python 3.14 (issue #5), `mkui-rsx` is empty, and `mkui-native` is a 142-line scene-walker that does not yet line up with the WGPU scaffolding already living in `mkui-wgpu` (the boundary tracked in issue #9). Error handling is hand-rolled and barely uses the `thiserror` dependency every backend pulls in.

Sprint 2 should treat the clippy/CI gap as the headline item — fixing the eight pre-existing `mkui-core` clippy issues, removing the `mkui-c` unsoundness, and wiring a GitHub Actions workflow are mechanical changes that immediately raise the floor across categories 2, 3, 7, and 9.

| # | Category | Score | Verdict |
|---|----------|-------|---------|
| 1 | Workspace & Crate Layout | 8 / 10 | Strong contract/backend split; minor: `mkui-rsx` empty, `mkui-native` vs `mkui-wgpu` overlap |
| 2 | Error Handling | 5 / 10 | One type per crate but `MkuiError` is hand-rolled, no `thiserror`, no `From<JsValue>` / `From<io::Error>` |
| 3 | Ownership, Borrowing & Lifetimes | 6 / 10 | Sound on hot paths but `mkui-c` is unsound by clippy + zero `// SAFETY:` notes |
| 4 | Async & Concurrency | n/a | No async surface; concurrency primitives unused. Excluded from average. |
| 5 | Test Coverage & Quality | 7 / 10 | 79 tests, good contract coverage; gaps: no `mkui-c`/`mkui-py` tests, web DOM path untested |
| 6 | Dependencies & Supply Chain | 6 / 10 | Lockfile committed, `cargo audit`/`deny` absent, several duplicated transitive deps |
| 7 | API Design & Public Surface | 6 / 10 | Builder pattern present but inconsistent; `#[non_exhaustive]` missing on growing enums |
| 8 | Performance & Resource Use | 7 / 10 | Reasonable; some allocation hotspots (clone-heavy WebButton variant chain) |
| 9 | Tooling & CI | 2 / 10 | **No CI at all**, no `rust-version`, clippy currently fails on `main` |
| 10 | Documentation | 8 / 10 | Crate-level docs are excellent; README contradicts current state in places |

**Weighted average (excluding N/A):** 6.4 / 10.

---

## 1. Workspace & Crate Layout

### Score: 8 / 10

### Findings

- **Strong contract crate.** `crates/mkui-core/Cargo.toml:9-17` keeps `mkui-core` to `thiserror`, `derive_more`, and optional `serde` — zero backend deps, as the workspace claims. The `lib.rs:1-31` doc-comment explicitly inverts the dependency direction.
- **Backends mirror each other.** `mkui-web/src/lib.rs`, `mkui-console/src/lib.rs`, and `mkui-wgpu/src/lib.rs` each export the same five public modules (`app` / `renderer` / `components` / `high_level` / `prelude`). This is the rare workspace where new backends really do have a template. (`mkui-wgpu/src/lib.rs:9-46`.)
- **`mkui-native` is a 142-line placeholder.** `crates/mkui-native/src/lib.rs:1-89` defines a `NativeScene`/`SceneNode` walker that *does not* depend on `mkui-wgpu`, even though `mkui-wgpu` already has a far more developed scene model in `mkui-wgpu/src/types.rs` and `mkui-wgpu/src/renderer.rs:1-14`. Issue #9 tracks this — the verdict for Sprint 2 is to either (a) make `mkui-native` depend on `mkui-wgpu` and re-export the scene types, or (b) delete `mkui-native` and rename `mkui-wgpu` to `mkui-native`. The current pairing is two backends pretending to be one.
- **`mkui-rsx` is empty.** `crates/mkui-rsx/src/lib.rs:1` is a single comment. The crate ships in the workspace and pulls in `syn` / `quote` / `proc-macro2` (`mkui-rsx/Cargo.toml:11-14`) but compiles to nothing. It should either be removed from the workspace until work starts, or get a "not yet implemented" `compile_error!` so consumers can't accidentally depend on it.
- **Workspace `Cargo.toml:3-17`** declares every member explicitly; no glob members; resolver is `2`. Good.
- **No circular deps.** Verified by reading each `Cargo.toml`: bindings → `mkui` → `mkui-{web,console}` → `mkui-core` is a strict DAG. `mkui-wgpu` is currently isolated (only `mkui-core`), `mkui-native` likewise.
- **Bridge feature scoping is correct.** `crates/mkui/Cargo.toml:19-23` makes both `web` and `console` opt-in and the bridge `lib.rs:81-115` handles the no-backend case with an explicit error path covered by `tests/no_backend.rs`.
- **`pub` discipline.** Modules and types are correctly `pub`/`pub(crate)`-scoped; nothing private leaks (verified by reading `mkui-web/src/lib.rs:50-62`, `mkui-console/src/lib.rs:35-44`, `mkui-wgpu/src/lib.rs:52-94`).

### Recommendations

1. Decide and execute the `mkui-native` vs `mkui-wgpu` merger described in issue #9 — either turn `mkui-native` into a thin re-export wrapper over `mkui-wgpu`, or fold `mkui-wgpu` into `mkui-native`.
2. Either start `mkui-rsx` or remove it from `[workspace.members]` until the macro work begins.
3. Add a workspace-level `lints` table (Cargo 1.74+) so per-crate clippy/style settings are declared once.

---

## 2. Error Handling

### Score: 5 / 10

### Findings

- **`MkuiError` is hand-rolled despite the `thiserror` dependency.** `crates/mkui-core/src/error.rs:1-65` defines `MkuiError { message: String, kind: MkuiErrorKind }` with hand-written `Display` and `Error` impls. The three crates that depend on `thiserror` (`mkui-core/Cargo.toml:10`, `mkui-web/Cargo.toml:35`, `mkui-console/Cargo.toml:11`) never invoke it. Migrating to `#[derive(thiserror::Error)]` would shrink the file by ~40 lines and make adding variants safer.
- **Lossy error conversions in the bridge.** `crates/mkui/src/lib.rs:96-107` and `lib.rs:142-150` convert backend errors to strings via `format!("Console initialization failed: {}", e)` and `format!("{:?}", e)`. The `JsValue` for the web backend loses structured data; the console `io::Error` loses its `ErrorKind`. Once `MkuiError` is `thiserror`-based, add `#[from] io::Error` and a `JsValue` variant so the conversion is preserved.
- **Error message context is thin.** `MkuiErrorKind` (`error.rs:11-20`) carries no file paths or operation names. For an initialization error from `mkui-web::WebRenderer::new` (`mkui-web/src/renderer.rs:10-17`), the user sees `Initialization: Element with id 'app' not found` — useful, but most other paths just emit short strings.
- **No `unwrap()` on user-reachable paths in core.** Verified by `grep`. The `unwrap`s in `mkui-c/build.rs` (lines 5, 6, 11) are build-time, fine. `mkui-web/src/utils.rs:4,10` uses `expect("no global window exists")` — acceptable for a browser-only crate, but should be documented at the function level.
- **`unwrap` in `Closure` wiring.** `mkui-web/src/render.rs:236-237` does `JsCast::dyn_ref::<HtmlElement>(&element).unwrap()` to attach `set_onclick`. Reachable from any user `Button::on_press`. Replace with `ok_or_else(|| JsValue::from_str("Button element is not an HtmlElement"))?`.
- **No distinction between retryable and non-retryable.** `MkuiErrorKind::Io` is reused for both "could not write to stdout" (retryable) and "the terminal is closed" (not). Add an `is_fatal()` method or split kinds further.
- **The C FFI builds error strings via `CString::unwrap_or_else(unwrap())`.** `mkui-c/src/lib.rs:51-53` — if both `CString::new` calls fail the process panics. Use a static fallback message.

### Recommendations

1. Convert `MkuiError` to `thiserror::Error`-derived form with `#[from]` impls for `std::io::Error` and (behind `cfg(target_arch = "wasm32")`) `JsValue`.
2. Add `MkuiError::with_context(self, ctx: impl Into<String>) -> Self` so backends can annotate errors as they bubble up.
3. Replace the `.unwrap()` in `mkui-web/src/render.rs:236` with a proper error path.

---

## 3. Ownership, Borrowing & Lifetimes

### Score: 6 / 10

### Findings

- **`mkui-c` FFI surface is unsound by Rust's standards.** 11 hard clippy errors of `clippy::not_unsafe_ptr_arg_deref` across `mkui-c/src/lib.rs:77, 92, 99, 121, 129, 136, 159, 167, 184, 205, 222`. Every public `extern "C"` function dereferences raw `*mut MkuiApp` / `*const c_char` pointers without being marked `unsafe fn`. This compiles only because clippy's deny is currently being suppressed by the test workflow (it isn't — `cargo clippy` fails). The functions must be declared `pub unsafe extern "C" fn` with `// SAFETY:` comments describing the pointer-validity contract callers must uphold. This is the **single highest-priority remediation in the audit**.
- **Zero `// SAFETY:` comments in the whole workspace.** `grep` for `SAFETY|// SAFETY` returns nothing. Every `unsafe { ... }` block in `mkui-c/src/lib.rs:76, 92, 98, 121, 129, 135, 159, 167, 183, 204, 221` is undocumented.
- **`unsafe_code = "forbid"` not declared anywhere.** `mkui-core`, `mkui-console`, `mkui-web` (Rust-only side), `mkui-wgpu`, `mkui-native`, `mkui-rsx`, and `mkui` could all forbid `unsafe` at the crate level, since they contain none. Add `#![forbid(unsafe_code)]` to each crate's `lib.rs`.
- **`Rc<dyn Fn()>` in `Button::on_press`.** `crates/mkui-core/src/components.rs:178, 201-207, 221-223` stores the handler as `Rc<dyn Fn()>`. This is appropriate — it lets the same handler be retained by both the original tree and the backend's flattened plan (see `mkui-console/src/components.rs:30, 86`), and the audit confirms via `tests/component_smoke.rs:111-129` that the `Rc` is shared rather than cloned per call.
- **`WebButton::variant`/`size`/`on_click` rebuild the inner headless button via `ButtonBuilder::new()...build()`.** `mkui-web/src/components.rs:52-80`. Each chained call clones the text and the variant/size, then constructs a fresh `HeadlessButton`. For three chained builder calls this is three full state rebuilds. Either store fields on `WebButton` directly, or add `with_variant(&mut self, …)` mutation to `HeadlessButton`.
- **`StyleClass::add` takes `mut self` by value (`crates/mkui-core/src/style.rs:12-15`).** This is the builder pattern, fine, but `add(&mut self, …) -> &mut Self` would let callers loop. Documented as an inconvenience, not a soundness issue.
- **No spurious clones on hot paths.** The 15 `.clone()` occurrences (`grep` count) are concentrated in `ConsoleButton` (cloning `Rc<dyn Fn()>` — cheap) and `WebButton` builder calls; the renderer's tessellation/render loops in `mkui-wgpu/src/tessellation.rs` are by-reference.

### Recommendations

1. **Critical**: Re-declare every `pub extern "C" fn` in `mkui-c/src/lib.rs` as `pub unsafe extern "C" fn`, and add `// SAFETY:` blocks above each `unsafe { … }` describing what the caller must guarantee about pointer validity, lifetime, and ownership.
2. Add `#![forbid(unsafe_code)]` to `mkui-core/src/lib.rs`, `mkui-web/src/lib.rs`, `mkui-console/src/lib.rs`, `mkui-wgpu/src/lib.rs`, `mkui-native/src/lib.rs`, `mkui-rsx/src/lib.rs`, and `mkui/src/lib.rs`.
3. Eliminate the rebuild-on-every-setter pattern in `mkui-web/src/components.rs:52-80` by mutating the existing `HeadlessButton`.

---

## 4. Async & Concurrency

### Score: n/a (excluded from average)

### Findings

- **No async surface anywhere.** `grep` for `async |tokio|async_std|\.await` across `crates/` returns zero matches. The console renderer's main loop (`mkui-console/src/high_level.rs:67-98`) is fully synchronous on `crossterm::event::read()`; the web backend dispatches through DOM events; the WGPU/native paths are not yet doing frame loops.
- **No `Mutex` / `RwLock` / `Arc` in any crate.** The only `Arc` mention is a comment in `mkui/src/lib.rs:7`. State sharing uses `Rc<RefCell<WebApp>>` (`mkui-web/src/high_level.rs:18`) — appropriate for the single-threaded WASM target.
- **Implication for Sprint 2:** when `mkui-wgpu`'s render loop gains a winit window, the workspace will need a documented async/runtime stance. The audit's recommendation is to leave the backend single-threaded by default (no `tokio` in `mkui-core` or any backend) and only introduce async at the very edges if a specific backend demands it.

### Recommendations

1. Document the no-async stance in `mkui-core/src/lib.rs` so future contributors don't reach for `tokio` reflexively.

---

## 5. Test Coverage & Quality

### Score: 7 / 10

### Findings

- **79 tests pass, 2 ignored.** Excluding `mkui-py` (broken — issue #5) and `mkui-c` (clippy failure prevents test compilation), `cargo test --workspace` is clean.
- **`mkui-core` contract tests are good.** `crates/mkui-core/tests/component_smoke.rs:46-185` covers tree traversal, variant round-trips, handler retention through `Rc`, headless button/toggle state machines. Combined with inline `#[cfg(test)] mod tests` in every module (`components.rs:228-282`, `theme.rs:117-135`, `layout.rs:125-171`, `input.rs:104-147`), this is the strongest tested crate.
- **`mkui-web` registry contract is properly tested.** `crates/mkui-web/tests/web_smoke.rs:17-79` and `tests/custom_component_extension.rs:37-109` together verify that (a) the built-ins are registered, (b) unknown components are not silently accepted, (c) a downstream type plugs in via `register::<T>()`, (d) the fallback hook is opt-in. This is exactly the contract PR #14 promised.
- **`mkui-console` walker tests are realistic.** `crates/mkui-console/src/components.rs:99-185` covers `Rc` handler retention, recursion into nested views, `TextVariant`-driven styling (not class-string sniffing).
- **`mkui-wgpu` has only `PanelLayout` smoke tests.** `crates/mkui-wgpu/src/types.rs:446-494` covers a couple of geometric invariants and `crates/mkui-wgpu/src/components.rs:453-454` checks button quad fills. The 660-line `builder.rs` and 535-line `tessellation.rs` have no tests. Given the recent upstream from stonesketch this is the largest coverage gap.
- **`mkui-c` and `mkui-py` are completely untested.** Both crates depend on `mkui` and re-implement the same builder pattern; both have hand-rolled error mapping; neither has a single `#[test]`. Smoke tests would catch issue #5 (Python 3.14 break) immediately.
- **The web DOM rendering path is *not* tested.** `tests/custom_component_extension.rs:30-35` and `render.rs:255-263` explicitly bail with `unreachable!()` because the host target has no `Document`. This is the right call, but it leaves PR #14's actual DOM emission paths covered only by manual `examples/web-showcase` runs. A `wasm-bindgen-test` target would close this gap.
- **No property tests.** `Edges::all`, `Edges::symmetric`, `Layout` builder chaining are good candidates for `proptest`.
- **No `#[ignore]`d tests.** Verified.
- **CI does not run any of these tests.** No `.github/workflows/`. See category 9.

### Recommendations

1. Add `wasm-bindgen-test` coverage for the actual `render_web` paths in `mkui-web` so the DOM emission shape is checked, not just the registry plumbing.
2. Add at least construct-and-destruct tests for `mkui-c` (using `unsafe` from the test harness once the FFI is corrected) and `mkui-py`.
3. Add 4-5 tests to `mkui-wgpu/src/builder.rs` covering `UiBuilder::heading`, `subheading`, `NumberRow`, `ListRow` — the upstream from stonesketch is otherwise unverified at integration level.

---

## 6. Dependencies & Supply Chain

### Score: 6 / 10

### Findings

- **`Cargo.lock` is committed.** `Cargo.lock` at the workspace root is tracked, with version pins for every transitive dep. Good.
- **No `cargo deny` / `cargo audit` configuration.** No `deny.toml` at the root, no audit step in any (non-existent) CI. For a workspace shipping FFI bindings to C and Python this is a meaningful gap.
- **Duplicated transitive deps.**
  - `bitflags` v1 + v2 (`Cargo.lock:23, 29`)
  - `hashbrown` two versions (`Cargo.lock:232, 238`)
  - `indexmap` two versions (`Cargo.lock:273, 283`)
  - `linux-raw-sys` two versions (`Cargo.lock:321, 327`)
  - `rustix` two versions (`Cargo.lock:688, 701`)
  - `syn` v1 + v2 (`Cargo.lock:894, 905`)
  - `wasi` two versions (`Cargo.lock:1023, 1029`)
  - All `windows-sys` / `windows-targets` and every `windows_*_msvc` / `windows_*_gnu` shim has v0.52 + v0.59 variants.
  These mostly come from `crossterm` (older `bitflags`/`rustix`) + `safer-ffi` / `pyo3` (newer). Not blocking, but `cargo tree -d` from CI would surface drift.
- **`default-features` rarely disabled.** `mkui-web/Cargo.toml:11-33` explicitly enables only the `web-sys` features it needs — good. But workspace-level dependency declarations in `Cargo.toml:27-55` mostly leave default features on (`thiserror`, `derive_more`, `tracing`, `tracing-subscriber`). `derive_more` is *declared* in the workspace dependencies (`Cargo.toml:29`) but is never actually used (`Grep` for `derive_more` in `crates/`: zero matches).
- **Empty `mkui-rsx` pulls in `syn` / `quote` / `proc-macro2`.** `mkui-rsx/Cargo.toml:11-14` — these compile every time the workspace builds, for zero functionality.
- **Unmaintained crate check.** `derive_more` 0.99 is on the cusp of being superseded by 1.x; `cbindgen` 0.26 (one major behind 0.27); `pyo3` 0.22 (one major behind 0.23). None are abandoned; all should bump in Sprint 2 or 3.
- **`tracing` / `tracing-subscriber` are declared but unused.** `Cargo.toml:45-46`. Either start emitting traces from the renderer entry points or drop the dep.

### Recommendations

1. Add a `deny.toml` and wire `cargo deny check` + `cargo audit` into CI.
2. Drop `derive_more`, `tracing`, `tracing-subscriber`, `serde`, `serde_json` from `[workspace.dependencies]` if not used (verify each first).
3. Bump `pyo3` to 0.23 — likely fixes the Python 3.14 issue (#5).
4. Move `syn` / `quote` / `proc-macro2` out of `mkui-rsx`'s `[dependencies]` until macro work begins.

---

## 7. API Design & Public Surface

### Score: 6 / 10

### Findings

- **Two clippy issues that are also API-design bugs.**
  - `crates/mkui-core/src/theme.rs:54-71` defines `ColorTheme::from_str(&str) -> Option<Self>` as an inherent method, conflicting with `std::str::FromStr::from_str`. The moment a consumer writes `use std::str::FromStr;` the call becomes ambiguous. Either implement the `FromStr` trait (returning `Result<Self, …>`) or rename to `parse_color_class`.
  - `crates/mkui-core/src/style.rs:17-19` defines an inherent `to_string(&self) -> String`. This silently shadows `ToString`, breaking any code that uses `s.to_string()` after a `Display` impl is added. Replace with `impl Display for StyleClass`.
- **Builder pattern is good but inconsistent.** `View::new`, `Text::new`, `Button::new` use by-value `mut self` builders (`mkui-core/src/components.rs:97-129, 139-169, 181-224`) — fluent. `Mkui::new` in the bridge returns `Result<Self, MkuiError>` and *also* uses the same `child(self, …)` builder — consistent. But `Theme::new(mode, color)` takes positional args (`theme.rs:103-105`), `HudTheme` has no builder, and `Layout::row()` / `Layout::column()` mix builder + factory. Not wrong, just uneven.
- **`#[non_exhaustive]` is missing on enums that will grow.** Verified by `grep`: zero matches. The following enums should be `#[non_exhaustive]` because they obviously have not stabilized:
  - `MkuiErrorKind` (`mkui-core/src/error.rs:10-20`) — only 4 variants today.
  - `ButtonVariant`, `ButtonSize`, `TextVariant`, `TextSize`, `TextWeight`, `TextAlign` (`mkui-core/src/headless/{button,text}.rs`).
  - `ColorTheme` (`mkui-core/src/theme.rs:19-33`) — already shadcn-aligned but adding a theme today would be a breaking change.
  - `Key`, `PointerButton`, `InputEvent` (`mkui-core/src/input.rs:12-29, 87-102`) — the doc-comment on `Key` explicitly says backends map "their raw key codes into these variants", which guarantees this enum will grow.
- **`Button::new` returns `Self` but `Text::new` returns `TextBuilder` — that's actually `clippy::new_ret_no_self` flagged at `mkui-core/src/headless/text.rs:258-260`.** And the *high-level* `Text::new` at `mkui-core/src/components.rs:140-146` returns `Self`, while the *headless* `Text::new` at `mkui-core/src/headless/text.rs:258-260` returns a builder. Two functions with the same name, same arg shape, different return types. Pick one.
- **Builders that need `Default` impls.** Clippy flags `mkui-core/src/headless/text.rs:192`, `headless/button.rs:202`, `headless/toggle.rs:135` — each `ButtonBuilder::new` / `TextBuilder::new` / `ToggleBuilder::new` has no public `Default::default()`. Easy add.
- **`pub` items lacking rustdoc with `# Examples`.** Most components have a one-line doc but no `# Examples` block. E.g. `crates/mkui-core/src/components.rs:64-89` (`Mkui`) has zero rustdoc on `new`, `child`, `children`. The crate-level doc is excellent; the per-item docs are sparse.
- **No `pub` types in private signatures.** Verified by reading each `lib.rs` re-export list.
- **Semver discipline.** Version is `0.x`, so breaking changes are allowed by default — but the workspace just shipped four PRs in Sprint 1 that touched `mkui-console` and `mkui-web` public APIs, and version stayed at 0.3.0 throughout. That is consistent with `0.x` semver but should be documented in a `CHANGELOG.md`.

### Recommendations

1. Replace `ColorTheme::from_str` with `impl FromStr for ColorTheme` (uses `Result<Self, Self::Err>`).
2. Replace `StyleClass::to_string` with `impl Display for StyleClass`.
3. Apply `#[non_exhaustive]` to the seven enums listed above.
4. Rename the headless `Text::new` to `Text::builder` or `Text::with_content` to remove the ambiguity with `mkui_core::components::Text::new`.
5. Add `impl Default` to `ButtonBuilder`, `TextBuilder`, `ToggleBuilder`.
6. Start a `CHANGELOG.md` at the workspace root.

---

## 8. Performance & Resource Use

### Score: 7 / 10

### Findings

- **No `Vec::with_capacity` for known sizes.** `Mkui::new` (`mkui-core/src/components.rs:69-73`), `View::new` (`components.rs:97-103`), `WebRendererRegistry::new` (`mkui-web/src/render.rs:82-87`) all start with empty `Vec`/`HashMap`. The console flattener `walk_component` (`mkui-console/src/components.rs:54-91`) pushes into `Vec`s without sizing hints. Low impact at current tree sizes, but `WebRendererRegistry::with_defaults` (`render.rs:91-97`) knows it will hold three entries — `HashMap::with_capacity(8)` is a one-liner.
- **`WebButton::variant`/`size`/`on_click` rebuild the headless button.** Already flagged in category 3 (`mkui-web/src/components.rs:52-80`). Each setter clones text and re-runs `ButtonBuilder::build`. For a typical button construction the user makes three setter calls — three full state rebuilds.
- **`apply_theme` enumerates all 13 color themes on every theme switch.** `mkui-web/src/app.rs:69-72` calls `html_class_list.remove_1(theme.to_class())` for every theme in `ColorTheme::all()`. The list of currently-applied classes could be cached on `WebApp`.
- **`ColorTheme::all()` allocates a new `Vec` every call.** `mkui-core/src/theme.rs:73-89` constructs a 13-element `Vec` on each invocation; used in `WebApp::apply_theme` (called on mount and on every theme change) and `ThemeSelector::render_web`. Make it `&'static [ColorTheme]`.
- **`paint_blank` allocates a per-row string.** `mkui-console/src/renderer.rs:88-98` calls `" ".repeat(clear_width)` once per row inside the resize loop. Build the string once outside the loop. Probably negligible at 24×80 but the redraw path on resize hits it.
- **`PRINTABLE_ASCII` lookup is good.** `mkui-core/src/input.rs:70-83` uses a static `&str` slice — exemplary, the audit calls it out as a positive.
- **`chip_group` takes 9 arguments.** `mkui-wgpu/src/components.rs:298-308`. Clippy flags `too_many_arguments`. Group `(scene, layout, row_rect)` into a `RenderContext` struct.
- **`field_reassign_with_default` in `mkui-wgpu` tests.** `mkui-wgpu/src/types.rs:465-466, 482-483` — cosmetic, but fix the suggestion in passing.
- **No benchmarks.** No `benches/` directory in any crate, no `criterion` dep. For a UI toolkit aiming at multiple platforms the rendering paths should have at least one criterion suite — `walk_component` in the console backend is an obvious starting point.
- **No `cargo build --release` smoke in CI** (because no CI).

### Recommendations

1. Make `ColorTheme::all() -> &'static [ColorTheme]` (zero-allocation).
2. Cache the active color-theme class on `WebApp` so `apply_theme` doesn't enumerate all 13 entries.
3. Fix `WebButton` rebuild churn (also a category 3 item).
4. Add a `benches/` directory with a baseline criterion benchmark for `walk_component` and `WebRendererRegistry::render`.

---

## 9. Tooling & CI

### Score: 2 / 10

### Findings

- **There is no CI at all.** `.github/` does not exist in the repository (verified by `ls -la /Users/mik/dev/mikbry/ui/`). None of `cargo fmt --check`, `cargo clippy`, `cargo test`, or `cargo build --release` is enforced before merge.
- **Clippy currently fails on `main`.** Running `cargo clippy --workspace --exclude mkui-py --all-targets` produces:
  - **11 errors** in `mkui-c/src/lib.rs` (lines 77, 92, 99, 121, 129, 136, 159, 167, 184, 205, 222) — `clippy::not_unsafe_ptr_arg_deref` is deny-by-default.
  - **~31 warnings**, of which the pre-existing eight in `mkui-core` are noted in the audit prompt:
    - `mkui-core/src/headless/toggle.rs:135` — `ToggleBuilder::new` needs `Default`.
    - `mkui-core/src/headless/button.rs:202` — `ButtonBuilder::new` needs `Default`.
    - `mkui-core/src/headless/text.rs:192` — `TextBuilder::new` needs `Default`.
    - `mkui-core/src/headless/text.rs:258` — `Text::new` returns `TextBuilder`, not `Self`.
    - `mkui-core/src/style.rs:12` — `add` conflicts with `std::ops::Add::add`.
    - `mkui-core/src/style.rs:17` — inherent `to_string` shadows `ToString`.
    - `mkui-core/src/theme.rs:54` — `from_str` conflicts with `std::str::FromStr::from_str`.
    - (Plus the `chip_group` 9-arg warning and the `field_reassign_with_default` pair in `mkui-wgpu`, the doc-overindent warnings in `mkui-wgpu/src/lib.rs:21-45`, and `unused_variable` in `mkui-c/build.rs:5`.)
- **No `rust-version` (MSRV) anywhere.** `grep` over every `Cargo.toml` returns zero matches. The workspace pulls in `pyo3 0.22` (needs Rust 1.63+), `wasm-bindgen 0.2.95+` (needs Rust 1.57+), `crossterm 0.28` (needs Rust 1.65+) — declaring `rust-version = "1.74"` or similar would prevent silent regressions.
- **No `#[allow(...)]` justifications**, but there's only one `#[allow]` in the workspace (`mkui-wgpu/src/theme.rs:475`, `dead_code`) and it lacks a comment.
- **`cargo fmt --check` would surface drift.** Reading `mkui-core/src/headless/text.rs` shows occasional 4-space indentation in trait impls (`text.rs:80-87`) where the rest of the workspace uses standard `rustfmt` defaults — a `cargo fmt` pass would normalize.
- **Local verification documented in README is good.** `README.md:312-328` lists the exact commands the audit ran. Good. The next step is to put them in a workflow.

### Recommendations

1. **Critical — add a `.github/workflows/ci.yml` that runs:**
   ```yaml
   - cargo fmt --all -- --check
   - cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings
   - cargo test --workspace --exclude mkui-py
   - cargo build --workspace --exclude mkui-py --release
   ```
   plus a separate `python` job that builds `mkui-py` once issue #5 is resolved.
2. **Critical — fix the 11 `mkui-c` clippy errors** (declare functions `unsafe extern "C" fn`, add `// SAFETY:`).
3. **Fix the 8 pre-existing `mkui-core` clippy warnings** noted in the sprint context — most are 1-2 line edits.
4. Declare `rust-version = "1.74"` in `[workspace.package]` so MSRV regressions get caught.
5. Add a `lints.workspace = true` table with `unsafe_code = "forbid"` for the Rust-only crates and `unsafe_code = "allow"` overrides for `mkui-c`.

---

## 10. Documentation

### Score: 8 / 10

### Findings

- **Crate-level `//!` docs are excellent.** Every backend's `lib.rs` opens with a detailed design note explaining its place in the workspace (`mkui-core/src/lib.rs:1-31`, `mkui-web/src/lib.rs:1-48`, `mkui-console/src/lib.rs:1-33`, `mkui-wgpu/src/lib.rs:1-50`, `mkui/src/lib.rs:1-72`). The bridge crate's ASCII diagram in `mkui/src/lib.rs:10-31` is the clearest single visual artifact in the workspace.
- **Module-level docs cover boundary contracts.** `mkui-web/src/render.rs:1-43` is the strongest — it explains the `WebRendererRegistry` extension point with a working code example, the unsupported-component policy, and how the fallback hook differs from the panic path. Mirror this template across `mkui-console/src/components.rs:1-15` and `mkui-wgpu/src/builder.rs:1-27` (already done — both are good).
- **README is the audit's biggest documentation liability.**
  - `README.md:343-360` lists "Phase 1: Foundation (COMPLETED)" with claims like "✅ **Console Renderer** - Terminal UI with crossterm (no ratatui dependency)" — accurate, but...
  - `README.md:379-385` says the **Current Focus is v0.2.0** — the workspace is at v0.3.0 (verified by `Cargo.toml:20`).
  - `README.md:37` ("Cargo.toml" doesn't exist) shows `Mkui::new()?` returning `Result<Mkui, MkuiError>`. This matches the bridge crate API. ✓
  - `README.md:144-194` describes `mkui-native` as the "native (WGPU) backend" — but `mkui-wgpu` exists as a sibling crate (added in PR #12). The README does not mention `mkui-wgpu`.
  - `README.md:271-300` describes a Python build flow that currently does not work (issue #5). No disclaimer.
  - Issue #7 already tracks "README positioning". Fix as part of Sprint 2.
- **No ADRs / design docs directory.** The `docs/` directory did not exist before this audit. Major architectural decisions (e.g. PR #14's registry pattern, the `mkui-core`-as-contract stance, the no-async stance) live only in PR descriptions. Add an `docs/architecture/` with one ADR per major decision (3-4 docs would cover everything).
- **`pub` rustdoc is sparse.** Already covered in category 7. The crate-level docs are A-grade; the per-item docs are C-grade.
- **No orphan docs detected.** No `.md` files in the workspace other than `README.md` and the audit instructions.

### Recommendations

1. **Rewrite README "Current Focus" and "Crate Layout" sections** so they reflect v0.3.0 reality (`mkui-wgpu` exists, `mkui-native` is a placeholder, Python is broken on 3.14).
2. Create `docs/architecture/` with ADRs for: (a) `mkui-core` as the contract crate, (b) registry-based backend dispatch, (c) bridge crate feature selection, (d) the native/WGPU boundary (post-issue-#9 decision).
3. Add `# Examples` blocks to every public constructor in `mkui-core` (`Mkui`, `View`, `Text`, `Button`, `Theme`, `Layout`).

---

# Remediation Roadmap

The roadmap is sequenced so each phase unblocks the next. Phase 1 raises the floor (eliminate the clippy failure that gates everything else); Phase 2 closes architectural debts noted in Sprint 1; Phase 3 hardens the toolkit for external consumers.

Effort scale: **S** ≤ ½ day, **M** ≈ 1-2 days, **L** ≈ 3-5 days.

## Phase 1: Stop the Bleeding (Week 1) — Priority: Critical

Block any new feature merge until this phase lands. Without it, every Sprint 2 PR will trip clippy on every category-9 finding.

| # | Task | Files | Effort | Category |
|---|------|-------|--------|----------|
| 1.1 | **Make every `mkui-c` extern fn `unsafe extern "C" fn` and add `// SAFETY:` comments.** Resolves 11 clippy errors and the soundness gap. | `crates/mkui-c/src/lib.rs:63-225` | M | 3, 9 |
| 1.2 | **Add a `.github/workflows/ci.yml`** running `cargo fmt --check`, `cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings`, `cargo test --workspace --exclude mkui-py`. | `.github/workflows/ci.yml` (new) | S | 9 |
| 1.3 | **Fix the 8 pre-existing `mkui-core` clippy warnings:** add `Default` impls to the three builders, replace `to_string`/`from_str` with proper trait impls, rename or remove the conflicting `StyleClass::add`. | `crates/mkui-core/src/{style.rs, theme.rs, headless/{button,text,toggle}.rs}` | M | 7, 9 |
| 1.4 | **Declare `rust-version = "1.74"`** in `[workspace.package]`. | `Cargo.toml:19-24` | S | 9 |
| 1.5 | **Update README** to reflect v0.3.0 (mention `mkui-wgpu`, note `mkui-py` is broken on 3.14, drop the "Current Focus: v0.2.0" line). Closes issue #7. | `README.md:343-385, 144-194, 271-300` | M | 10 |

**Exit criteria for Phase 1:** `cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings` exits 0; CI is green on a fresh PR.

## Phase 2: Sprint 1 Cleanup (Weeks 2-3) — Priority: High

Close architectural questions left open by Sprint 1.

| # | Task | Files | Effort | Category |
|---|------|-------|--------|----------|
| 2.1 | **Decide and execute the `mkui-native` ↔ `mkui-wgpu` boundary** (issue #9). Either fold `mkui-wgpu` into `mkui-native`, or make `mkui-native` re-export the `mkui-wgpu` scene types. | `crates/mkui-native/{Cargo.toml, src/lib.rs}`, `crates/mkui-wgpu/Cargo.toml` | L | 1 |
| 2.2 | **Fix `mkui-py` for Python 3.14** (issue #5). Bump `pyo3` to 0.23 and re-run `maturin develop`. Add a regression test. | `crates/mkui-py/{Cargo.toml, src/lib.rs}`, `Cargo.toml:54-55` | M | 6 |
| 2.3 | **Migrate `MkuiError` to `thiserror::Error`** with `#[from]` impls for `std::io::Error` and (cfg-gated) `JsValue`. | `crates/mkui-core/src/error.rs`, bridge translations in `crates/mkui/src/lib.rs:91-157` | M | 2 |
| 2.4 | **Add `#![forbid(unsafe_code)]`** to the seven Rust-only crates. | every `lib.rs` except `mkui-c`, `mkui-py` | S | 3 |
| 2.5 | **Apply `#[non_exhaustive]`** to `MkuiErrorKind`, `ButtonVariant`, `ButtonSize`, `TextVariant`, `TextSize`, `TextWeight`, `TextAlign`, `ColorTheme`, `Key`, `PointerButton`, `InputEvent`. | `crates/mkui-core/src/{error.rs, headless/*.rs, theme.rs, input.rs}` | S | 7 |
| 2.6 | **Add `wasm-bindgen-test` coverage** for the actual DOM-emitting paths in `WebRendererRegistry::render` and `ThemeSelector::render_web`. | `crates/mkui-web/tests/` (new wasm test target) | M | 5 |
| 2.7 | **Add construct-and-destruct smoke tests** for `mkui-c` (once Phase 1.1 lands) and `mkui-py` (once 2.2 lands). | `crates/mkui-c/tests/`, `crates/mkui-py/tests/` (new) | M | 5 |
| 2.8 | **Build out the WGPU renderer** (issue #2) — start with a hosted `mkui_wgpu::WgpuRenderer` that can paint a `Scene` through a `wgpu::Device`. | `crates/mkui-wgpu/src/renderer.rs`, new `wgpu` dep behind a feature flag | L | 1 |

**Exit criteria for Phase 2:** all four currently-open issues (#2, #5, #7, #9) closed.

## Phase 3: Hardening for External Use (Weeks 4-6) — Priority: Medium

Once the workspace compiles cleanly under CI and the open issues are closed, harden for downstream consumers.

| # | Task | Files | Effort | Category |
|---|------|-------|--------|----------|
| 3.1 | **Add `deny.toml` + `cargo audit` to CI.** | `deny.toml` (new), `.github/workflows/ci.yml` | M | 6 |
| 3.2 | **Per-item rustdoc with `# Examples`** for every `pub` constructor in `mkui-core`. | `crates/mkui-core/src/{components.rs, theme.rs, layout.rs, input.rs}` | M | 10 |
| 3.3 | **Create `docs/architecture/`** with ADRs for the contract crate, registry dispatch, bridge crate feature selection, and the native/WGPU decision from 2.1. | `docs/architecture/00*.md` (new) | M | 10 |
| 3.4 | **Add `benches/` to `mkui-core` and `mkui-web`** with criterion suites for `walk_component`, `WebRendererRegistry::render`, and a 50-button tree traversal. | `crates/mkui-core/benches/`, `crates/mkui-web/benches/` (new) | M | 8 |
| 3.5 | **Fix `WebButton` rebuild churn** (`mkui-web/src/components.rs:52-80`). | `crates/mkui-web/src/components.rs` | S | 3, 8 |
| 3.6 | **Make `ColorTheme::all() -> &'static [ColorTheme]`** and cache the active class on `WebApp`. | `crates/mkui-core/src/theme.rs:73-89`, `crates/mkui-web/src/app.rs:67-103` | S | 8 |
| 3.7 | **Remove unused workspace deps** (`derive_more`, `tracing`, `tracing-subscriber`, `serde_json` if not used). | `Cargo.toml:26-56` | S | 6 |
| 3.8 | **Either remove `mkui-rsx` or start the macro.** Drop `syn`/`quote`/`proc-macro2` if removed. | `Cargo.toml:7`, `crates/mkui-rsx/` | M | 1, 6 |
| 3.9 | **Start a `CHANGELOG.md`** at the workspace root, retroactively documenting Sprint 1 changes. | `CHANGELOG.md` (new) | S | 7, 10 |
| 3.10 | **Add 4-5 tests to `mkui-wgpu/src/builder.rs`** covering `UiBuilder::heading`, `NumberRow`, `ListRow`. | `crates/mkui-wgpu/src/builder.rs` (test mod) | M | 5 |

**Exit criteria for Phase 3:** `cargo deny check` and `cargo audit` clean; rustdoc renders without warnings; criterion baseline checked in; `mkui-rsx` either has real code or is removed.

---

## Appendix A — Categories Not Scored

- **Category 4 (Async & Concurrency)** is intentionally excluded from the weighted average. The workspace has no async surface, no shared mutable state, and no concurrency primitives. The only finding (document the no-async stance) is a documentation task, not a remediation.

## Appendix B — Top Three Findings by Priority

1. **`mkui-c` FFI is unsound by clippy's standards (11 hard errors).** The entire public extern surface must be re-declared `unsafe extern "C" fn` with `// SAFETY:` documentation of the pointer contracts callers must uphold. Anything else built on top of `mkui-c` is built on a foundation that doesn't compile under `clippy -D warnings`. Sprint 2's first PR.
2. **No CI exists.** `.github/` is absent. Every regression in this audit (clippy errors, broken `mkui-py`, README drift, MSRV ambiguity) would have been caught by a 40-line `ci.yml`. Sprint 2's second PR.
3. **`mkui-core` ships eight pre-existing clippy errors that are also real API bugs.** `ColorTheme::from_str` shadowing `FromStr::from_str` and `StyleClass::to_string` shadowing `ToString::to_string` are not just style issues — they are silent ambiguity traps for any downstream crate that brings the std traits into scope. The agent that closed PR #15 already documented these; Phase 1.3 fixes them in roughly a half day.
