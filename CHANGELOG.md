# Changelog

All notable changes to mkui will be documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (pre-1.0,
breaking changes can land on minor bumps).

## [Unreleased]

### Added
- (next sprint's additions land here)

### Fixed
- **wgpu live-resize jerk eliminated on macOS (#99).** PR #98 (#97) fixed
  the HiDPI viewport-units math but the visible scale-snap on shrinking
  resize gestures (right→left, bottom→top) persisted. Root cause was at
  the event-loop redraw-cadence layer: mkui-wgpu had no `about_to_wait`
  handler, so during continuous resize gestures the OS could present
  frames at the new layer size before the next rendered frame caught
  up — visible as a Metal swapchain transition stretch.

  Adds a narrow `about_to_wait`-driven resize redraw pump (120-frame cap,
  arm on `Resized` + `ScaleFactorChanged`). The pump's decay measures
  *presented* frames (`RenderOutcome::Drawn`) rather than event-loop ticks,
  so skipped frames under GPU pressure don't accidentally drain the bridge
  during fast live-resize gestures. The 120-frame cap (~2s at 60Hz) keeps
  the pump saturated through fast gestures — each `Resized` re-arms it —
  then decays back to idle once the drag stops; a smaller cap, or a
  tick-counted one, exhausted faster than fast drags presented frames and
  left the jerk visible (tuned upward 4 → 60 → 120 across Sprint 6.5 as
  operator visual verify narrowed the residual fast-drag vibration). Resize handlers only arm the pump; `about_to_wait`
  drives redraws after the resize event batch drains, and cursor movement
  while the pump is armed re-arms the pump and requests a redraw immediately
  to mirror StoneSketch's input-event redraw path during fast edge/corner
  drags. `RedrawRequested` also performs a final `window.inner_size()` sync
  before rendering so a frame is not acquired/presented against stale surface
  dimensions when AppKit has advanced the native layer before the matching
  winit resize event drains. The renderer also mirrors the upstream
  reference's `Suboptimal` swapchain handling: render/present the frame, then
  reconfigure the surface so fast live-resize does not keep presenting
  against a stale surface state.
  Independent of the first-paint state machine; idle windows stay idle. ADR
  0006 §"Resize-active redraw pump" documents the design + the load-bearing
  "`Drawn` decrements but does not clear the pump" invariant.
- **wgpu primitives now land at logical-pixel positions on HiDPI displays
  (#97).** The Sprint 5 bridge inherited a viewport-units mismatch: scene
  primitives were authored in logical pixels (matching web's CSS-pixel and
  console's character-grid conventions) but `Renderer::render` projected
  vertices against the physical-pixel surface config. On Retina
  (`scale_factor=2.0`), a logical-200 coord projected to NDC -0.75 instead
  of -0.5 → primitives landed in the upper-left quadrant. Each resize
  surfaced a 1-2 frame scale-snap as walker and renderer briefly desynced.
  Fix: `Renderer::render` now uses `scene.viewport.{width, height}` (logical)
  as the projection denominator instead of `self.config.{width, height}`
  (physical). `WindowEvent::ScaleFactorChanged` is now handled so windows
  dragged between displays of different DPIs reproject correctly. ADR 0006
  §"Viewport units contract" makes the logical-pixel convention explicit
  across the cross-binding API surface. No public API change.
- **wgpu examples render their primitives again; gray backdrop no longer
  darkens on resize (#93).** Two regressions, silently present since the
  Sprint 5 bridge merge (v0.6.0), left both `native-window` and
  `atoms-on-wgpu` showing only the clear color:
  - **Resize clobbered the raw-scene escape hatch.** `WindowEvent::Resized`
    unconditionally replaced the scene with a fresh empty one, which is
    correct for the declarative AppTree path (rebuilt from the tree) but
    wiped the user's primitives in `with_scene` mode before first paint —
    macOS fires a `Resized` post-creation, so `native-window`'s quad was
    gone before it ever drew. The resize handler now honours a per-path
    contract (ADR 0006 §"Resize behaviour contract"): AppTree scenes rebuild
    from the tree; raw scenes keep their primitives and only update the
    viewport in place.
  - **MSAA disabled pending correct sRGB orchestration.** The Sprint 5 4×
    MSAA path (no StoneSketch upstream parent) is the suspected source of
    the resize-time darkening and of `atoms-on-wgpu` rendering empty despite
    emitting 9012 valid triangles — consistent with the MSAA-resolve-into-
    sRGB step double-applying sRGB encoding on macOS Metal. The UI pass now
    runs at `sample_count=1` (the StoneSketch-proven direct-write path) as a
    deliberate temporary policy (ADR 0006 §"MSAA disabled pending correct
    sRGB orchestration"); re-introducing MSAA with a correct sRGB resolve is
    tracked in #95, and anti-aliasing is intentionally off until then.
  - **Blank first paint until resize.** `resumed()` schedules a single
    redraw; wgpu returns `Skipped` when the surface isn't ready yet, and an
    idle UI event loop had no other redraw trigger, so the window stayed
    blank-gray until the user resized or interacted. The first paint now
    retries on `Skipped` until a frame is `Drawn` (capped to avoid an
    occluded-window spin); once drawn, `Skipped` returns to a no-op so idle
    frames idle (ADR 0006 §"First-paint render scheduling").

  Adds displayless regression tests (raw-scene survives resize, declarative
  resize rebuilds from tree, non-empty render input for both examples'
  scenes, and the first-paint retry state machine) so the empty-render class
  can't reach a release visually-unverified again.

## [0.9.0] — 2026-06-04

### Added
- (no functional additions in this release — Sprint 6 Phase B was hygiene + deletions)

### Removed
- **Deleted the unused `mkui-native` placeholder crate.** `mkui-native` was a
  Sprint-2-era scene-walker placeholder that walked the shared
  `mkui_runtime::AppTree` but rendered nothing and had no consumer — no crate or
  example depended on it, and `mkui` exposed no `native` feature. Sprint 5's
  `mkui-wgpu` is the production native backend with a real `AppTree`-consuming
  bridge (ADR 0006), so the placeholder was pure workspace bloat. The crate
  directory, its `Cargo.toml` workspace-member entry, and the README crate-status
  row are removed; stale doc-comment references to `mkui-native` as a backend now
  point at `mkui-wgpu`. Closes the original Sprint-1 tracker #9; round-5 audit
  Cat 1 + Phase 1 Task 1.3 (#73).
- **Deleted the `mkui-rsx` placeholder crate.** Its `src/lib.rs` was three
  lines (`#![forbid(unsafe_code)]` + a `// Placeholder` line comment) with no
  `//!` doc, no public surface, and no other crate depending on it — pure
  workspace-build time + audit surface for zero current value (round-5 audit
  Cat 1 + Phase 1 Task 1.4). mkui's identity is the fluent builder API
  (`Mkui::new()?.child(View::new()...)`); an RSX-style proc-macro is a separate
  aesthetic, not current scope. The workspace `members` entry is removed too.
  Re-adding the crate is trivial if an RSX macro is ever wanted — there is no
  migration cost to deletion (#74)

### Tooling
- **CI: new `msrv` job pins `dtolnay/rust-toolchain@1.87.0`** and runs
  `cargo check --workspace --exclude mkui-py --locked`. Every other job floats
  on `@stable`, so the declared `rust-version` was never tested. The job fails
  loudly if a dep ever requires a newer toolchain than the declared MSRV,
  before the breakage reaches a user on a fresh-MSRV install (round-5 audit
  Cat 9 + Phase 3 Task 3.1, #72).
- **MSRV bumped from 1.84 to 1.87.** The new gate immediately caught that the
  declared `rust-version = "1.84"` had been silently false since Sprint 4
  (#51 / commit 05fb386): wgpu 29.0.3 (`wgpu-hal` / `wgpu-types` /
  `wgpu-naga-bridge`) requires rustc 1.87. Truth wins — the declared MSRV now
  matches what the workspace actually compiles on; downgrading wgpu would lose
  Sprint 4 substrate work (#72).

### Changed
- **Docs: refreshed the workspace `README.md` + ADR index to v0.8.0
  reality.** The "Current capabilities" header now tracks the actual
  `Cargo.toml` version; ADRs 0005 (`mkui-runtime` substrate) and 0006
  (`mkui-wgpu` declarative bridge) are listed in the README architecture
  section; the `mkui-c` soundness framing reflects the Sprint 4 `// SAFETY:`
  annotations + CI re-entry (no longer "deferred to Sprint 4"); the Python
  3.14 / PyO3 0.28 note points at [#53] instead of a stale Sprint-4/5
  deferral; and `docs/architecture/README.md` says "six records" to match its
  own index. Pure documentation hygiene — no behavior change (#71)
- **`mkui-core`: sharper diagnostic when `Mkui::child` panics on a class-parse
  error.** `.child()` stays infallible (the fluent builder surface is
  unchanged), but the panic is now `#[track_caller]` so the blame points at the
  consumer's `.child(...)` call site instead of mkui-core internals, and the
  panic message carries the offending class token (via `ClassParseError`'s
  `Display`) instead of the opaque `"lowering failed — see ClassParseError"`.
  A typo in a Tailwind-like utility class now shows exactly which token failed.
  The rustdoc gains standard `# Panics` + `# Examples` sections pointing at the
  fallible [`Mkui::try_child`] for callers who want a `Result`. No call-site
  changes (backwards-compatible) (#69)
- **`mkui-runtime`: removed the dead `free` reuse pool from `ActionRegistry`.**
  The pool (and the generation-bump path that fed it) was designed as a
  use-after-free guard for a future node-removal API (Codex round-7
  §"Concrete Shape"), but no public API removes an action, so it was never
  populated — dead infrastructure flagged by the round-5 audit (Phase 2 Task
  2.6). `register_local`/`register_local_action`/`register_remote` are now
  plainly append-only. The public `ActionId` shape is unchanged: the
  `generation` field is retained as an always-`0` forward-compat reservation,
  so `ActionId::from_raw` callers are unaffected, and the lookup guard still
  rejects forged/out-of-band ids. The `free` pool + an
  `ActionRegistry::remove(id)` will be re-introduced if a node-removal API
  ever lands (#70).
- **`mkui-wgpu`: reorganized component implementations into a `components/`
  subdirectory with a grouped layout.** The flat `src/components.rs`
  (15 components) plus the already-extracted `src/badge.rs` / `src/dot.rs` are
  now a single `src/components/` directory. Related functions are grouped per
  file rather than one file per function: `button + button_with` in
  `button.rs`, `panel + titled_panel` in `panel.rs`,
  `text + heading + label` in `text.rs`, `slider + SliderRegions` in
  `slider.rs`. The remaining components (`badge`, `card`, `chip_group`, `dot`,
  `info_list`, `scrollbar`, `swatch`, `text_field`) each ship in their own
  file under `components/`. Pure module reorganization — no behavior change.
  The `components::*` re-export surface (and the `widgets` glob alias) is
  unchanged, but moving the two previously crate-root atom modules into
  `components/` removes their old public module paths (breaking, pre-1.0):
  - `mkui_wgpu::badge::*` → `mkui_wgpu::components::badge` (the `badge` fn is
    now re-exported from `components`; the crate-root `badge` module is gone)
  - `mkui_wgpu::dot::*` → `mkui_wgpu::components::dot` (same shape as `badge`)
  - the input-field component `mkui_wgpu::components::input` →
    `mkui_wgpu::components::text_field` (resolves the name collision with the
    crate's pointer-routing plumbing, now `src/pointer.rs`; "TextField" also
    matches the renderer's single-line text-input semantic better than
    shadcn's generic "Input") (#78)
- **BREAKING (`mkui-wgpu`): renamed `src/input.rs` → `src/pointer.rs`**
  (module path `mkui_wgpu::input` → `mkui_wgpu::pointer`). The file is pointer
  routing — it converts winit cursor/mouse events into press / arm / hit-test /
  activation over the current frame's interactive regions — not text-input
  semantics, so `pointer` names it accurately and frees the `input` namespace.
  The re-exported `hit_test` / `ClickHit` / `PointerState` items at the crate
  root are unchanged; only the module path moves. Pre-1.0 minor-bump policy
  permits the path break (#78)
- **BREAKING (`mkui-wgpu`): scrubbed the HUD paradigm from the backend's
  identity.** Post-Sprint-5 the wgpu backend renders `mkui_runtime::AppTree`
  end-to-end — a UI framework backend, not the 2D HUD pipeline it was ported
  from. The type system and docs now say so:
  - `HudTheme` → `WgpuTheme` (the root theme for the wgpu UI backend). Plain
    `Theme` was rejected because `mkui_core::Theme` already owns that name;
    `WgpuTheme` names the backend and disambiguates. Re-exported from
    `mkui_wgpu::prelude` under the new name.
  - `theme::WgpuTheme::hud_panel()` → `panel_style()`; `components::hud_list()`
    → `info_list()`.
  - Shader `render/hud.wgsl` → `render/ui_triangles.wgsl` (it draws the
    tessellated UI triangle stream, not a widget-specific pass); entry points
    `vs_hud`/`fs_hud` → `vs_ui_triangles`/`fs_ui_triangles`; the `Renderer`'s
    `hud_pipeline` field / `build_hud_pipeline` helper / wgpu debug labels
    reframed from "HUD" to "UI" (`ui_pipeline` / `build_ui_pipeline`).
  - File-level `//!` docs reframed from the HUD-overlay paradigm to the
    UI-framework / `AppTree` substrate identity (`lib.rs`, `render/mod.rs`,
    `theme.rs`, `components.rs`, `walker.rs`). The `Mkui::with_scene` doc keeps
    "custom HUDs" as one valid low-level escape-hatch use case (per ADR 0006).
  Pure rename + doc rewrite — no behavior change. ADR 0004's historical
  "2D HUD pipeline port" framing is preserved as engineering history (#77)
- Docs/comment hygiene pass: scrubbed proper-noun references to the
  predecessor private reference codebase from public-facing source-doc
  comments (`mkui-text`, `mkui-wgpu`), the CHANGELOG history entry for
  v0.3.0, and ADRs 0002 + 0004. The engineering-history meaning
  ("port of an earlier 2D HUD pipeline", "bitmap glyph table") is
  preserved; only the private-repo proper noun is replaced with
  domain-neutral phrasing. Zero functional impact (#58)
- Docs/comment hygiene pass (round-5 audit Cat 4 / 9 / 10): documented the
  `ViewProps` / `TextProps` / `ButtonProps` payload structs in
  `mkui-runtime::tree`; added justifying `// allow:` comments to the
  `#[allow(dead_code)]` on `walker::map_text_variant` and standardized the
  `#[allow(clippy::new_without_default)]` comment in `mkui-py` to the workspace
  pattern; annotated the `pollster::block_on` adapter-init call site in
  `mkui-wgpu::app` (ADR 0004); and added a **Threading model** section to the
  README documenting the single-threaded `AppTree` / `ActionRegistry` invariant
  (ADR 0005 + ADR 0006). Pure documentation — zero behavior impact (#75)

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
