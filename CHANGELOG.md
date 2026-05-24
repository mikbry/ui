# Changelog

All notable changes to mkui will be documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (pre-1.0,
breaking changes can land on minor bumps).

## [Unreleased]

### Added
- (next sprint's additions land here)

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
