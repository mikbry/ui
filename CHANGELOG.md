# Changelog

All notable changes to mkui will be documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (pre-1.0,
breaking changes can land on minor bumps).

## [Unreleased]

### Added
- (next sprint's additions land here)

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

### Changed
- `mkui-core::Mkui` / `View` / `Text` / `Button` internals now lower into
  an `mkui_runtime::AppTree` via the new `LoweringRegistry`. Public Rust
  ergonomic API unchanged — `examples/showcase-common/src/lib.rs`
  compiles byte-identical (#51)
- `mkui-core::headless::ButtonVariant` and `TextVariant` are now
  re-exports of the runtime types so every binding sees the same enum
  without going through `mkui-core` (#51)
- `mkui-web` consumes `AppTree` directly: built-in `View` / `Text` /
  `Button` render through fixed paths in `render::render_tree`;
  `WebRendererRegistry` keyed by `type_name` dispatches `NodeKind::Custom`
  to downstream-registered `CustomWebRenderable` implementations (#51)
- `mkui-console` walks `AppTree` instead of the legacy
  `Vec<Box<dyn Component>>` shape. Actions fire through the runtime's
  `ActionRegistry` by id rather than via `Rc<dyn Fn()>` pointers (#51)
- `mkui-native` `NativeScene::collect` now takes an `&AppTree` so the
  future wgpu bridge consumes the same shape as every other backend (#51)
- **`mkui-c` full rewrite** — flat `add_view` / `add_text` / `add_button`
  replaced by handle-based nested API: `mkui_app_view_child`,
  `mkui_app_text_child`, `mkui_app_button_child`,
  `mkui_app_register_callback`. New `MkuiNodeId` / `MkuiActionId` opaque
  handles (each carries `index` + `generation`). Every `unsafe` block
  carries a `// SAFETY:` annotation (audit Phase 1.1 fold-in) (#51)
- **`mkui-py` full rewrite** — flat `add_view` / `add_text` /
  `add_button` replaced by handle-based nested API on `App`:
  `app.view_child(parent, class)`, `app.button_child(parent, label,
  variant, class, callback_id)`. New `PyNodeId` / `PyActionId` classes (#51)
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
- ADR 0005 added to `docs/architecture/` documenting the runtime crate.
  Does **not** supersede ADR 0001 — runtime is the contract-implementation
  layer, `mkui-core` remains the contract crate (#51)

### Tooling
- `cbindgen 0.29.2` upgrade prunes 3 of 4 advisory ignores from
  `deny.toml` / `.cargo/audit.toml` (`atty` / `paste` / `PyO3 0.22`
  cluster) — only the safer-ffi-via-`paste` ignore remains until the
  upstream releases an audit-clean revision (#51)

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
  — upstreamed from `stonesketch-gui`'s domain-neutral subset (#12)
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
