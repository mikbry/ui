# Changelog

All notable changes to mkui will be documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (pre-1.0,
breaking changes can land on minor bumps).

## [Unreleased]

### Added
- (next sprint's additions land here)

### Changed
- Docs/comment hygiene pass: scrubbed proper-noun references to the
  predecessor private reference codebase from public-facing source-doc
  comments (`mkui-text`, `mkui-wgpu`), the CHANGELOG history entry for
  v0.3.0, and ADRs 0002 + 0004. The engineering-history meaning
  ("port of an earlier 2D HUD pipeline", "bitmap glyph table") is
  preserved; only the private-repo proper noun is replaced with
  domain-neutral phrasing. Zero functional impact (#58)

## [0.6.0] — 2026-05-27

### Added
- **`mkui-wgpu` declarative bridge over `mkui_runtime::AppTree`** —
  `Mkui::new()?.child(...).run()` on the wgpu backend now mirrors the
  web and console backends. The walker projects the runtime tree into
  the existing tessellation pipeline (no new GPU pipeline state); the
  input router does reverse paint-order hit-testing against the per-
  frame `Vec<HitTestEntry>` collected during the walk. ADR 0006
  documents the design (#56)
- **`WgpuRenderable` trait + `WgpuRendererRegistry`** in `mkui-wgpu`
  (backend-local placement per Codex round-7 Q1 ratification, mirrors
  `mkui-web::WebRendererRegistry`). Trait signature matches the Codex
  round-10 §"Concrete Shape" sketch: `render(&Node, &Value, &mut WgpuRenderCtx)
  -> Result<WgpuRenderOutcome, MkuiError>`. `WgpuRenderOutcome::{RecurseChildren,
  ChildrenHandled}` lets a renderer signal whether the walker should
  continue into the node's children. `WgpuRenderCtx<'a>` exposes the
  five round-10 fields (`tree`, `registry`, `scene`, `theme`, `hits`)
  plus three layout-state fields extension renderers need to position
  primitives in the walker's vertical flow. `with_defaults()` ships
  built-in `BadgeRenderer` + `DotRenderer` so the scene-primitive
  atoms ported in ADR 0004 stay accessible through the AppTree
  `NodeKind::Custom` slot. Custom-component + fallback tests in
  `crates/mkui-wgpu/src/bridge.rs` (#56)
- **`mkui-wgpu/src/walker.rs`** — `walk_app_tree(tree, registry, &WalkOptions)
  -> Result<WalkOutput, MkuiError>` AppTree → `Scene` walker matching
  the Codex round-10 §"Concrete Shape" sketch (`WalkOptions` carries
  viewport + theme; `WalkOutput` returns `scene` + `hit_tests` +
  `layouts` as a single move). Eager rebuild on
  `RuntimeCtx::RequestRedraw`; never indexed into the raw `nodes` vec
  (every lookup goes through `tree.get(id)` so the generation-counter
  staleness guard fires). Layout v1 is a deliberately minimal
  wgpu-local pass (top-down vertical flow + class-driven padding /
  gap / text-button sizing); a future shared `mkui-layout` module is
  the reserved seam for cross-binding layout parity (ADR 0006 §"Out
  of scope") (#56)
- **`mkui-wgpu/src/input.rs`** — press-to-arm pointer state machine
  (Codex round-10 Q4). Cursor latched from `WindowEvent::CursorMoved`;
  press hit-tests + arms the topmost node; release fires only if the
  release hit-tests the same armed node; `CursorLeft` and Escape
  clear the armed slot without firing. Logical/physical DPI
  conversion via `window.scale_factor()`. `window.request_redraw()`
  is called from the event-loop handler — never from inside action
  closures (Sprint 4 anti-pattern carry-forward) (#56)
- **`examples/atoms-on-wgpu`** re-introduced — 12-badge grid + dot
  showcase + title text built via `tree.push_custom("badge", …)` /
  `tree.push_custom("dot", …)`. The renderers ship in
  `WgpuRendererRegistry::with_defaults()` so the example needs no
  per-app registration (#56)
- **`examples/native-showcase`** — *added alongside* the existing
  `examples/native-window` HUD smoke (not a rename). Drives
  `examples/showcase-common::create_showcase_ui` end-to-end on the
  wgpu backend via `mkui::run!(create_showcase_ui, wgpu)`. The
  showcase function itself is byte-unchanged from main (Codex round-7
  Q6 audit-grade preservation) (#56)
- **`mkui::run!` wgpu arm** — the bridge crate now supports
  `mkui::run!(create_ui, wgpu)` alongside the existing `web` /
  `console` arms (#56)
- **`mkui-wgpu::Mkui::from_core(core)`** — wraps a pre-built
  `mkui_core::components::Mkui` so callers that constructed the
  `AppTree` directly (FFI bindings, the atoms-on-wgpu example) can
  hand it to the wgpu run loop (#56)
- **`mkui` bridge crate `wgpu` feature** — enables the wgpu backend
  through the unified `Mkui` type and the `mkui::run!` macro's `wgpu`
  arm (#56)
- **ADR 0006** — bridge design (walker, registry, input model,
  press-to-arm pointer activation), relationship to ADR 0005
  (consumes the substrate, doesn't extend it) and ADR 0004
  (tessellation pipeline preserved; bridge layers above), and
  `with_scene` retained as the documented low-level escape hatch
  (Codex round-10 Q5 ratification) (#56)

### Changed
- **`mkui-wgpu/Cargo.toml`** — adds `mkui-runtime` dependency. Sprint
  4 deliberately deferred this edge; Sprint 5 takes it (#56)
- **`mkui_wgpu::Mkui::with_scene` doc** — clarified as the retained
  low-level escape hatch for renderer tests, custom HUDs, headless
  tessellation demos, and future direct-GPU experiments (Slug,
  mkui-vector2d). Behaviour unchanged; the rewritten doc positions
  it alongside the declarative `Mkui::new` API as documented public
  API rather than a successor-pending path (#56)

## [0.5.0] — 2026-05-25

### Added
- `mkui-runtime` crate — portable application-tree substrate (`AppTree`,
  `NodeId`, `ActionId`, `NodeKind`, `ActionRegistry`, `RuntimeCtx`,
  `RuntimeSignal`, `StyleClass`, `ResolvedStyle`, JSON snapshots).
  Single arena every binding builds into; `(index, generation)` handles
  guard against use-after-free. ADR 0005 documents the design (#51)
- Runtime class parser owns `StyleClass` / `ResolvedStyle` for the
  Tailwind-shaped utility-class strings used by the showcase. 43 Tier-1
  tokens (the showcase set), 3 Tier-2 no-op patterns (`hover:*`, `sm:*`,
  `transition-colors`), Tier 3 → parse error with a helpful message
  naming the bad token and the tier system (#51)
- Canonical JSON snapshots of the `AppTree` (feature `snapshot`) — the
  parity gate that proves Rust / C / Python construction frontends
  produce byte-identical trees (#51)
- `mkui-runtime` `NodeKind::Custom { type_name, props }` extension slot
  + Sprint 4 `TestWidget` extension proof in the parity test suite
  (#51)
- **Byte-identical parity tests for Rust ↔ C ↔ Python** —
  `crates/mkui-c/tests/parity.rs` and `crates/mkui-py/tests/parity.rs`
  build the same non-trivial tree (nested View + Text + Button + class
  strings + action) through each binding's public surface and
  `assert_eq!` on the full canonical JSON string (Codex round-8 P1
  fix; the round-7 PR only did `contains(...)` substring matching on
  the C side and had no Python coverage at all) (#51)
- **`mkui-py-parity` CI job** — dedicated Linux job that builds mkui-py
  with PyO3 0.28.3 + sets up a real Python interpreter and runs the
  parity test. macOS hosts skip via `#[cfg(not(target_os = "macos"))]`
  because PyO3 cdylib link on macOS needs `maturin develop` (#51)
- **`cbindgen-header-clean` CI job** — runs `cargo build -p mkui-c`
  and asserts `git diff` on `crates/mkui-c/include/mkui_c.h` is empty.
  The checked-in header is the cbindgen output; drift between source
  and generated form is now a build failure instead of a silent
  problem caught downstream at link time (Codex round-8 P2 follow-up
  on audit Phase 2 Task 9) (#51)

### Changed
- `mkui-core::Mkui` / `View` / `Text` / `Button` internals now lower into
  an `mkui_runtime::AppTree` via the new `LoweringRegistry`. Public Rust
  ergonomic API unchanged — `examples/showcase-common/src/lib.rs`
  compiles byte-identical (#51)
- `mkui-core::components::Mkui::with_tree(tree)` constructor added so
  FFI bindings can hand a pre-built `AppTree` to a backend's `run` loop
  without rebuilding via `.child(...)` (#51)
- `mkui-console::Mkui::from_core(core)` constructor added — mkui-c and
  mkui-py route their `run_console` through this to invoke the real
  interactive backend (Codex round-8 P2: the round-7 PR shipped a stub
  `println!` summary in both bindings, dropping the v0.4.x capability)
  (#51)
- `mkui-core::headless::ButtonVariant` and `TextVariant` are now
  re-exports of the runtime types so every binding sees the same enum
  without going through `mkui-core` (#51)
- `mkui-web` consumes `AppTree` directly: built-in `View` / `Text` /
  `Button` render through fixed paths in `render::render_tree`;
  `WebRendererRegistry` keyed by `type_name` dispatches `NodeKind::Custom`
  to downstream-registered `CustomWebRenderable` implementations (#51)
- `mkui-web::high_level::fire_action_global` now **captures** the
  `RuntimeCtx` returned by `ActionRegistry::fire` and routes its
  `dirty` bit + `RequestRedraw` signals back to the tree (Codex
  round-8 P1: the round-7 PR dropped the ctx, silently breaking the
  substrate's redraw contract) (#51)
- `mkui-console` walks `AppTree` instead of the legacy
  `Vec<Box<dyn Component>>` shape. Actions fire through the runtime's
  `ActionRegistry` by id rather than via `Rc<dyn Fn()>` pointers, and
  the Enter/Space handler propagates the resulting `RuntimeCtx` dirty
  bit to the tree + rebuilds the layout so structural changes surface
  immediately (Codex round-8 P1 fix mirrors web's) (#51)
- `mkui-native` `NativeScene::collect` takes an `&AppTree` so the
  future wgpu bridge consumes the same shape as every other backend.
  Scope: this is the same one-line API rewire applied to every backend
  (web/console/wgpu); the broader `mkui-native` cleanup Codex round-7
  Q8 deferred is **not** touched here (#51)
- **`mkui-c` full rewrite** — flat `add_view` / `add_text` / `add_button`
  replaced by handle-based nested API: `mkui_app_view_child`,
  `mkui_app_text_child`, `mkui_app_button_child`,
  `mkui_app_register_callback`. New `MkuiNodeId` / `MkuiActionId` opaque
  handles (each carries `index` + `generation`). Every `unsafe` block
  carries a `// SAFETY:` annotation (audit Phase 1.1 fold-in). Each
  child constructor now validates `tree.get(parent).is_some()` before
  the runtime's `assert!`-on-invalid-parent path — a panic across
  `extern "C"` is undefined behaviour (Codex round-8 P1 fix) (#51)
- **`mkui-py` full rewrite** — flat `add_view` / `add_text` /
  `add_button` replaced by handle-based nested API on `App`:
  `app.view_child(parent, class)`, `app.button_child(parent, label,
  variant, class, callback_id)`. New `PyNodeId` / `PyActionId` classes.
  Stale parent handles surface as `PyValueError` instead of panicking
  across the PyO3 boundary (Codex round-8 P1 fix) (#51)
- `mkui-c/build.rs` now writes the cbindgen output to the **checked-in**
  `crates/mkui-c/include/mkui_c.h` (not just `target/include/...`), and
  cbindgen generation failure is fatal — drift between source and
  generated header surfaces at build time, not at downstream link time.
  Local-dev `MKUI_C_SKIP_CBINDGEN=1` and `DOCS_RS=1` opt-outs documented
  (Codex round-8 P2 follow-up on audit Phase 2 Task 9) (#51)
- `pyo3` 0.22 → 0.28.3 (unblocks Python 3.14; audit Phase 5 Task 24).
  Migrated to `Bound`-based API + `#[pyclass(unsendable)]` for the
  single-threaded runtime invariant (#51)
- `cbindgen` 0.26 → 0.29.2 (clears `atty` + `clap 3` + `bitflags 1` +
  `syn 1` transitive duplicates from the workspace dep graph). Replaces
  `mkui-c/build.rs`'s manual `cbindgen.toml` bypass with a real
  `cbindgen::Builder` invocation (audit Phase 2 Task 9 fold-in) (#51)
- `mkui-c` re-enters CI build-release + clippy gates — the handle-based
  rewrite + `// SAFETY:` annotations clear `not_unsafe_ptr_arg_deref` by
  design (#51)
- C and C++ examples + `bindings/cpp/mkui.hpp` rewritten for the new
  handle-based API. The C++ wrapper now includes the cbindgen-generated
  `mkui_c.h` directly (rather than hand-maintaining its own forward
  declarations, which drifted in v0.4.x and was Codex round-8 P2). New
  `mkui::App::registerCallback(std::function<void()>)` wraps the C
  callback table in a typed C++ surface (#51)
- README.md / bindings/README.md / per-example READMEs updated to show
  the new handle-based API in every quick-start snippet. The pre-Sprint-4
  flat `mkui_app_add_*` / `addView` / `addText` shapes are documented
  as removed; v0.4.x is the last release to ship them (#51)
- ADR 0005 added to `docs/architecture/` documenting the runtime crate.
  Does **not** supersede ADR 0001 — runtime is the contract-implementation
  layer, `mkui-core` remains the contract crate (#51)

### Removed
- **Breaking**: `mkui_web::ThemeSelector` re-export removed. The v0.4.x
  type implemented `WebRenderable` directly, which the substrate
  rewrite obsoleted. Restoring it as a real component lowering through
  `NodeKind::Custom` is Sprint 6+ scope alongside the shadcn theme
  picker. Downstream code should pin to v0.4.1 or build its own theme
  picker via `View` + `Button` + an action (#51)

### Tooling
- `cbindgen 0.29.2` upgrade prunes 3 of 4 advisory ignores from
  `deny.toml` / `.cargo/audit.toml` (`atty` / `paste` / `PyO3 0.22`
  cluster) — only the safer-ffi-via-`paste` ignore remains until the
  upstream releases an audit-clean revision (#51)
- `mkui-py` CI status: the main workspace `clippy` / `test` /
  `build-release` jobs still exclude `mkui-py`. The dedicated
  `mkui-py-parity` job covers the load-bearing snapshot equality
  assertion; full re-merge into the main jobs (rmkui-py participating
  in `cargo clippy --workspace`) is tracked as a Sprint 5
  follow-up — link-time PyO3 wiring on the macOS+Windows matrix
  needs `maturin develop` -shaped setup the workspace jobs do not
  provide today (#51)

## [0.4.1] — 2026-05-23

### Added
- CI hardening: `--locked` enforcement on every cargo invocation, `cargo test --doc`,
  `cargo deny check`, `cargo audit`, `rust-version = "1.84"` MSRV declaration (#36)
- `#![forbid(unsafe_code)]` on 7 Rust-only crates (#37)
- `#[non_exhaustive]` on 17 growing public enums across mkui-core, mkui-text, mkui-wgpu (#37)
- `docs/architecture/` with 4 ADRs documenting current architecture (#45)
- `CHANGELOG.md` at the workspace root, retroactively covering v0.1.0 through v0.4.1 (#46)

### Changed
- `MkuiError` migrated to `#[derive(thiserror::Error)]` with `#[from]` impls for
  `std::io::Error`, `mkui_text::TextError`, and (cfg-gated) `JsValue` (#38)
- `mkui_text::TextError` migrated to `#[derive(thiserror::Error)]` (#38)
- `mkui-text/Cargo.toml` adds `thiserror` as a deliberate exception to the
  zero-external-deps stance — the only Sprint-2-era external dep in mkui-text (#38)
- `ColorTheme::all()` now returns `&'static [ColorTheme]` (was `Vec`) (#37)
- `WebApp` caches the active theme class instead of recomputing per render (#37)
- Bridge `mkui/src/lib.rs` error conversions use `?` + `#[from]` chains instead of
  lossy `format!("{:?}", e).into()` paths (#38)
- README rewritten to reflect v0.4.0/v0.4.1 reality, including mkui-wgpu + mkui-text,
  the wgpu HUD pipeline, broken-on-Python-3.14 disclaimer, and dropped v0.2.0 framing (#7)

### Removed
- Workspace `[dependencies]` entries for `derive_more`, `tracing`, `tracing-subscriber`
  (all verified unused at sprint open) (#37)

## [0.4.0] — 2026-05-22

### Added
- `mkui-text` crate with `TextSystem` trait + `BitmapTextSystem` (5×7 ASCII bitmap
  prototype ported from upstream reference). Zero external text-stack deps (#19)
- `mkui-wgpu` 2D HUD rendering pipeline — real `wgpu::Surface` + MSAA picker +
  HUD pipeline, ported from a production reference renderer (3D scene / shadow / SSAO /
  selection outline / accumulator passes deliberately dropped from the port) (#20)
- `mkui-wgpu::App` winit `ApplicationHandler` shell — `Mkui::run()` opens a window
  via `Mkui::with_scene(scene).run()` in two lines (#20)
- `examples/native-window/` — minimal renderer smoke: clear color + single quad (#20)
- shadcn-aligned `Badge` (6 variants: Default, Destructive, Outline, Secondary, Ghost, Link)
  + `Dot` (status-color variants + halo + animation modifiers) (#21)

### Changed
- `mkui-wgpu` tessellation now delegates text glyph data to the `TextSystem` trait
  via `Arc<dyn TextSystem>` instead of inline bitmap function (#19 + #20)

### Fixed
- All 8 pre-existing `mkui-core` clippy errors (Default impls on three builders,
  `StyleClass::add` → `StyleClass::push_class`, `inherent_to_string` → `Display`,
  `ColorTheme::from_str` inherent → `FromStr` impl) (#18)
- `mkui-c/build.rs:5` unused `crate_dir` variable (#26)
- `mkui-wgpu` clippy debt: 19 doc-overindent + 4 field-reassign-with-default + 1
  too-many-args errors resolved (#25)
- Workspace `cargo fmt --all` drift swept (#27)

### Tooling
- `.github/workflows/ci.yml` introduced with fmt + clippy + test + build-release jobs;
  CI now gates every PR. Phased rollout: `clippy` + `build-release` initially commented
  out behind `BLOCKED-BY:` markers, uncommented in #31 after #18 + #25 + #26 cleared
  the pre-existing debt (#17, #31).

## [0.3.0] — 2026-05-20

### Added
- `mkui-wgpu` crate with scene primitives, theme tokens (cva-style ButtonVariant/Size/State),
  declarative builders (`UiBuilder<T>`, `NumberRow`, `ListRow`), and tessellation pipeline
  — upstreamed from the predecessor HUD crate's domain-neutral subset (#12)
- `mkui-console` real component tree renderer replacing the prior closed-set showcase
  path; `TextVariant`-driven Line styling (#13)
- `mkui-web` extensible component registry (`WebRendererRegistry`) replacing the prior
  closed-set downcast list — custom components can register render functions without
  patching mkui-web (#14)
- Three new test suites covering mkui-core component construction, mkui-web smoke,
  and bridge no-backend (#16)

### Changed
- `mkui-web`, `mkui-console`, `mkui-wgpu` aligned to the same 5-module template
  (`app` / `renderer` / `components` / `high_level` / `prelude`) (#15)

## [0.2.0] — 2026-05-12

### Added
- Workspace initial layout: `mkui-core`, `mkui-web`, `mkui-console`, `mkui-native`,
  `mkui-rsx`, `mkui-c`, `mkui-py`, `mkui` (bridge). Shared component contract via
  `mkui-core` (#1, #11)

## [0.1.0]

Initial commit.

---

## Format conventions

- **Versions** in `[major.minor.patch]` form, with date in ISO 8601 (YYYY-MM-DD).
- **Sections** within a version: Added / Changed / Deprecated / Removed / Fixed / Security
  / Tooling. Use only the sections that have entries.
- **References** to PRs/issues use `#N` after each bullet for trackability.
- **Pre-1.0 versioning**: breaking changes can land on minor bumps (v0.X.Y → v0.X+1.0).
  Once v1.0 ships, the project switches to strict SemVer.
