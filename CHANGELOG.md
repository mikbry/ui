# Changelog

All notable changes to mkui will be documented in this file. The format is based
on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html) (pre-1.0,
breaking changes can land on minor bumps).

## [Unreleased]

### Added
- **SFNT outline → `SlugGlyph` extraction bridge (`mikbry/ui#169`, Part B.1 of
  `#165`'s Sprint 8 tail).** `mkui-vector2d::sfnt_bridge::extract_slug_glyph`
  is the first fallible, reusable bridge from `mkui-text`'s narrow SFNT
  decoder (`SfntFace`) to the Slug encoder — replacing the hand-assembled,
  `.expect()`-laden call chain `tests/slug_slice.rs` (#67) used per call site.
  It lives in `mkui-vector2d`, not `mkui-text`, because #64's layering is
  one-directional (`mkui-vector2d` depends on `mkui-text`, never the
  reverse) and `SlugGlyph`/`SlugBlobCache` are owned by `mkui-vector2d`.
  - Resolves a glyph's outline via `SfntFace::glyph_outline`, converts it to
    a `VectorPath`, and encodes it through
    `SlugBlobCache::encode_with_units_per_em` using the face's own
    `units_per_em` — the pre-#169 slice used the cache's default (`1.0`)
    instead, leaving the #157 Phase 2 step 5 band-overlap epsilon
    unnormalized to the font's real design-unit scale.
  - Returns `Result<Arc<SlugGlyph>, TextExtractionError>`; `TextExtractionError`
    is a typed, `#[non_exhaustive]` enum with `CompositeGlyph`,
    `NonQuadraticCurve`, and `MissingOutlineData(&'static str)` variants, so a
    caller can degrade to the bitmap lane by reason instead of panicking.
  - Unit + integration tests exercise the licensed Abel-Regular fixture
    (`crates/mkui-vector2d/tests/sfnt_bridge_abel.rs`): calibrated-bounds
    parity for glyph `M`, five ASCII glyphs, cache hit/miss reuse, a real
    out-of-range-glyph-id fallible branch, `units_per_em` plumbing, and
    cross-font key non-collision.
  - **Scope decision**: composite-glyph decomposition and CFF/cubic-Bézier
    support are explicitly out of scope (typed rejection only, per #169);
    component call-site routing (B.2), example integration (B.3), and
    golden-image regression tests (B.4) are follow-up phases of `#165`.

- **Slug rendering completion, Phase 4 — bitmap text restricted to integer
  scales + device-pixel snap (mkui-slug-rewrite mission, `mikbry/ui#157`
  step 8, variant B).** See ADR
  [0008](docs/architecture/0008-bitmap-vs-slug-labels.md) for the full
  variant-A-vs-B rationale (the ratified reference-harness adapter has no
  way to render at an arbitrary font size, making variant A's own BLESS
  criterion unverifiable without amending an immutable oracle).
  - **Integer-only bitmap scale.** `mkui-text`'s `bitmap::bitmap_scale` now
    rounds to the nearest integer instead of returning an arbitrary float
    (e.g. a 16px request previously scaled `1.6`×, now `2`×) — the 5×7
    bitmap face has no intermediate representation, so a fractional scale
    forced nearest-neighbor upscaling to duplicate source rows/columns
    unevenly. A `debug_assert!` documents the always-integer invariant.
  - **Device-pixel position snap.** `mkui-wgpu`'s `tessellate_text` snaps
    every glyph cell's screen position to the device-pixel grid,
    unconditionally, using the frame's fresh `device_pixel_ratio` — computed
    once per `Renderer::render` call and shared with the Slug lane's
    dilation/baseline-snap math. Deliberately unconditional, unlike Phase
    3's Slug baseline snap, which gates on a small-text threshold to protect
    Phase 1/2's large-text adapter-parity fixtures — the bitmap lane has no
    such fixtures to protect. (An earlier revision incorrectly copied
    Phase 3's threshold onto bitmap; Codex round 1 of this PR's review
    caught that it left the demo's own 16px label unsnapped — see Codex
    plan step 8's literal text, "snap every glyph to device pixels" — and it
    was removed.) `tessellate_primitives` gained a `device_pixel_ratio`
    parameter; `tessellate_scene`/`tessellate_scene_with_text` (used by
    `native-window`, `atoms-on-wgpu`, and `mkui-wgpu`'s non-windowed
    `Renderer` helper) keep their existing signatures, passing `1.0`
    internally — unaffected in behavior. Bitmap tessellation already re-runs
    fresh every frame from the declarative `Scene`, unlike Phase 3's first
    cut of the Slug baseline snap, so no Phase-3-style staleness redesign
    was needed.
  - Piecewise-constancy self-check tests mirror Phase 3's exact pattern:
    isolated tests on the snap function (100 sub-pixel offsets -> exactly 2
    cells at 1x DPI, correctly scaled at 1x/1.5x/2x/3x) plus an integration
    test through the real `Scene` -> `tessellate_primitives` path proving a
    "#" glyph's position quantizes in one-physical-pixel steps at 2x DPR at
    every font size tested, including the demo's actual 16px label size.
  - Verified: `cargo fmt --check`, workspace clippy, and feature-slug matrix
    clippy (all `-D warnings`) clean; `cargo test -p mkui-text` 81/81;
    `cargo test -p mkui-wgpu` (default, `slug`, `gpu-tests,slug`) all pass.

- **Slug rendering completion, Phase 2 — bounded dilation + band overlap
  epsilon (mkui-slug-rewrite mission, `mikbry/ui#157` steps 4-5).**
  - **Half-physical-pixel dilation (bounded 2D case).** `crates/mkui-vector2d-wgpu`'s
    quad expansion changed from a flat `1.5`-pixel constant to a half-pixel
    dilation derived from each glyph's own placement scale and the frame's
    logical→physical pixel ratio (`half_pixel_dilation_units`, applied to the
    font-unit bounds before pixel projection): `0.5 / (device_pixel_ratio *
    scale)`, scaled back by the same `scale` at projection time, is exactly
    half a *physical* pixel at every DPI. `SlugAdapter::prepare`/`prepare_paths`
    gained a `device_pixel_ratio` parameter (`1.0` for a caller with no
    logical/physical split); the real windowed renderer (`mkui-wgpu`'s
    `render/mod.rs`) derives it from the physical surface config vs. the
    logical scene viewport, closing the exact logical→physical gap Codex's
    original Sprint 8 review flagged at `app.rs`. mkui's screen transform is
    a uniform, axis-aligned scale + translate (no rotation/skew/perspective),
    so this closed form is exactly what the reference adapter's general
    Jacobian-based `slug_dilate` collapses to for that transform. Full
    per-render MVP/viewport-derived dynamic dilation stays out of scope
    (mkui doesn't ship perspective/transform text yet).
  - **Band overlap epsilon.** `crates/mkui-vector2d`'s CPU band builder
    (`build_bands`) now widens the scan-axis overlap test by an epsilon on
    both sides, closing the floating-point gap that could otherwise drop a
    curve sitting almost exactly on a band boundary. `SlugConfig` gained a
    `units_per_em` field (default `1.0`, additive and `Eq`/`Hash`-preserving —
    every existing `SlugConfig::new` call site is unaffected) and a
    `with_units_per_em` builder; the epsilon is `units_per_em / 1024.0`,
    matching the upstream README's recommended `1/1024` em overlap.
    `SlugBlobCache` gained `encode_with_units_per_em` so the real SFNT-backed
    glyph path (`mkui-wgpu`'s `slug_text::place_slug_run`) can normalize the
    epsilon against each face's *actual* units-per-em instead of the cache's
    default — a 2048-upem face would otherwise get an epsilon 2048x too
    small to have any effect.
  - Verified against the ratified `reference-harness/` adapter under the same
    pinned Docker + Lavapipe image as Phase 1: all 24 comparisons still hold
    (Δ ≤ 1/255, SSIM = 1.000000).
  - **Thin-gap regression test.** dame-rubric.md's Phase 2 "no thin-gap
    regressions" criterion, applied at its literal ≥50% bordering-alpha
    threshold, was jointly unsatisfiable with the same section's Δ ≤ 4/255
    reference-comparison criterion: the ratified `g` fixture (2× DPI) itself
    produces a zero-alpha texel at (157,156) bordered by 206/248 alpha — a
    literal hit under the rubric's own threshold — and mkui reproduces that
    texel byte-for-byte from the reference. No implementation that matches
    the oracle could pass both criteria as literally written (not resolved
    by loosening the detection threshold — tried and rejected by Codex
    review as redefining a frozen criterion). Resolved by operator rubric
    amendment 1 (v1.2.1): the check now excludes any thin-gap texel that is
    byte-identical to the reference golden at the same coordinate — a hit
    only counts as a regression if it also diverges (Δ > 0) from the
    reference there. Test un-ignored.
  - Two new CPU-level tests exercise the real encoder end to end (not a
    pre-encoded fixture): `band_overlap_epsilon_normalizes_to_units_per_em`
    proves a curve straddling a band boundary by `0.0005` em joins the band
    under the default epsilon and is excluded under a tighter `units_per_em`;
    `cache_encode_with_units_per_em_overrides_the_cache_default_per_call`
    proves the same through `SlugBlobCache`'s per-call override — the exact
    path `place_slug_run` depends on.
  - No change to `encode_slug_glyph`'s or `subdivide_cubic`'s signatures, the
    linear-color present-pass contract (#155), or the render-pass
    architecture.
- **Slug rendering completion, Phase 3 — cap-height/baseline snap for small
  UI text + golden-image regression tests (mkui-slug-rewrite mission,
  `mikbry/ui#157` steps 6-7).**
  - **Baseline snap, applied at per-frame pack time.** `crates/mkui-vector2d-wgpu`'s
    `pack` now rounds a glyph's baseline Y to the nearest physical pixel
    (`snap_to_physical_pixel`) when `PlacedSlugGlyph::cap_height_px` is below
    a `SMALL_TEXT_CAP_HEIGHT_PX` (16px) threshold, using the frame's *fresh*
    `device_pixel_ratio` — the same parameter Phase 2's dilation already
    reads there. `PlacedSlugGlyph` gained the `cap_height_px` field (the
    text-layout caller's font-size proxy for cap height, since this codebase
    doesn't parse the SFNT `OS/2.sCapHeight` table; producers with no
    cap-height concept — hand-authored fixtures, arbitrary path/stroke fills
    — pass `f32::INFINITY` to opt out unambiguously). `origin_px` itself is
    never mutated or snapped by the caller: `mkui-wgpu`'s
    `slug_text::place_slug_run` sets `cap_height_px` from `run.font_size_px`
    and leaves the baseline exactly as computed.
    - **Design history (Codex round 1 finding, real and fixed in round 2):**
      an earlier revision applied the snap *inside* `place_slug_run` itself,
      using a caller-supplied `device_pixel_ratio` baked in at
      scene-construction time — before a real window (and its real DPR)
      exists, and with no re-snap on `ScaleFactorChanged`. Codex correctly
      flagged that a fractional-DPI display or a window moved between
      monitors could leave small text snapped to the wrong physical grid,
      and that the golden tests (which bypass `place_slug_run` entirely)
      couldn't have caught a disconnected snap. Moving the snap into `pack`
      — mirroring exactly where Phase 2's dilation already lives — means
      every render call re-derives it from the frame's actual
      `device_pixel_ratio` (`crates/mkui-wgpu`'s `render/mod.rs` computes it
      from `self.config.width / scene.viewport.width` on every frame), so a
      DPI change self-corrects on the very next frame with no separate
      invalidation path and no `app.rs` change needed.
  - **Piecewise-constancy self-check.** `mkui-vector2d-wgpu` gains: two
    CPU-only tests on the isolated `snap_to_physical_pixel` function proving
    100 sub-pixel offsets across one physical-pixel period group into
    exactly 2 cells split at the midpoint, correctly scaled at device-pixel
    ratios 1x/1.5x/2x/3x; three `pack`-level tests proving the gate applies
    correctly (12px snaps, 16px does not — `<`, not `<=`) and that packing
    the *same* unsnapped `PlacedSlugGlyph` at two different DPRs produces two
    independently-correct results with no compounding (proving `pack` never
    mutates its input). `mkui-wgpu`'s `slug_text.rs` keeps two focused wiring
    tests (`cap_height_px` reflects `run.font_size_px`; `origin_px` is never
    snapped there regardless of font size). A new GPU-level test in
    `render/slug_gpu.rs`,
    `small_text_snap_reaches_real_pixels_at_a_fractional_device_pixel_ratio`,
    renders small text through the real `SlugAdapter::prepare`/`draw` path at
    a fractional DPR (1.5x, the case Codex called out): two pen positions in
    the same physical-pixel cell render byte-identical; crossing the cell
    boundary changes the render.
  - **Golden-image regression tests.** `crates/mkui-wgpu/tests/goldens/`
    gains 24 committed PNGs (the 8 ratified glyph fixtures × 1x/1.5x/2x DPI),
    captured from mkui's own renderer via a new `#[ignore]`d
    `capture_phase3_golden_images` regen tool (analogous to the
    reference-harness's `--write-fixtures`). A new always-on test,
    `phase3_golden_images_match_committed_baseline_at_all_dpis`, re-renders
    and diffs against these committed files at the same thresholds as Phase
    1 (Δ ≤ 4/255, differing pixels ≤ 10, SSIM ≥ 0.995) — a real, reviewable
    artifact distinct from Phase 1's dynamic re-render-and-diff, addressing
    Codex's original step-7 complaint that prior tests only asserted "some
    pixels changed" and couldn't catch softness or asymmetric antialiasing.
    (These fixtures render at a fixed, large em size — the small-text snap
    doesn't engage for them, by design: the external reference-harness
    adapter has no cap-height-snap concept and can't regenerate small-text
    goldens, so this reuses the same 8-glyph/3-DPI set Phase 1 already
    validates rather than attempting an oracle-ambiguous comparison.)
  - Verified under the same pinned Docker + Lavapipe image as prior phases:
    `mkui-vector2d`, `mkui-vector2d-wgpu`, `mkui-wgpu` (default, `slug`,
    `gpu-tests,slug`) all pass; `cargo fmt --check` and workspace +
    feature-slug-matrix clippy (`-D warnings`) clean.

- **Slug rendering completion, Phase 1 — dual-ray coverage + `fwidth` AA
  (mkui-slug-rewrite mission, `mikbry/ui#157` steps 1-3).** `crates/mkui-vector2d-wgpu`'s
  Slug coverage pipeline now implements the full published algorithm instead
  of a horizontal-only prototype:
  - **Vertical band + curve-index upload.** `SlugAdapter`'s CPU packer now
    uploads each glyph's `vertical_bands`/`vertical_curve_indices` (already
    produced by #65's encoder) into the shared GPU `bands`/`indices` buffers
    right after the horizontal ones, with new per-glyph `vband_base` /
    `vband_count` / `vindex_base` offsets (`GpuGlyph` grows 64 → 96 bytes).
  - **Dual-ray weighted coverage in `slug.wgsl`.** The fragment shader now
    casts one horizontal ray *and* one vertical ray per sample, each through
    the band selected by a clamped index derived from the glyph's own
    font-unit bounds (matching the Slug reference's `band_transform` lookup,
    not a containment scan — so AA coverage stays correct in the dilation
    margin just outside the exact bounds). Root validity is decided by the
    `0x2e74` sign-bit lookup (`calc_root_code`) rather than a `t in [0, 1)`
    range test, and the two axis coverages are combined via the published
    weighted formula (`calc_coverage`) — replacing the old single-ray,
    direct-root-clamp implementation.
  - **`fwidth`-derived AA width.** The shader's anti-aliasing width is now
    `1 / fwidth(font_pos)` (screen-derivative pixels-per-em), replacing the
    logical-pixel `scale_px_per_unit` that produced asymmetric horizontal vs.
    vertical edge weights. `scale_px_per_unit` remains the vertex-stage
    placement scale (unchanged use).
  - **Debug coverage entry point.** `fs_slug_debug_coverage` exposes the
    pre-clamp combined coverage plus per-axis accumulations for an unclamped
    intermediate capture; not used by the production pipeline.
  - Verified against the ratified `docs/chevalier/mkui-slug-rewrite/reference-harness/`
    adapter: all 24 glyph×DPI comparisons (8 glyphs × 1×/1.5×/2× DPI) render
    within max per-channel Δ ≤ 1/255 and 0 differing pixels under Lavapipe
    (rubric thresholds: Δ ≤ 4/255, ≤ 10 differing pixels).
  - No change to `mkui-vector2d`'s public API, the linear-color present-pass
    contract (#155), or the render-pass architecture; `QUAD_DILATION_PX`
    unchanged (Phase 2 scope).
- **`mkui-vector2d` extended beyond glyph outlines — Sprint 8 substrate Wave 1
  (#137).** The backend-neutral path crate now encodes *arbitrary* 2D vector
  geometry, not only resolved glyph outlines:
  - **`Stroke` descriptor** (`stroke` module) — a value-type stroke descriptor
    (`width_px`, `LineCap` {Butt, Round, Square}, `LineJoin` {Miter, Round,
    Bevel}, optional `DashPattern`) carried alongside a `VectorPath`. Descriptor
    only; stroke *expansion* lands in #138 (`stroke_expand::stroke_to_fill`).
  - **General Bézier encoder** (`encode::encode_vector_path`) — emits the same
    Slug-compatible curve/band records as the glyph lane for any `VectorPath`.
    Cubics are lowered to quadratics by the new `bezier` module using a
    hard-coded 1° angular-error tolerance
    (`CUBIC_SUBDIVISION_TOLERANCE_DEG`); NaN/Inf coordinates are rejected at the
    input boundary; band bounds are derived from the flattened geometry; fill is
    hard-coded non-zero winding.
  - **Content-derived cache key** (`encode::VectorPathKey` +
    `VectorPathBlobCache`) — a canonical, deterministic byte serialization of a
    path's commands (`-0.0` folded to `+0.0`) that is byte-identical across
    threads, enabling per-frame caching and reuse without font identity.
  - No public API break to the existing `SlugGlyph`/`encode_slug_glyph` path;
    zero new dependencies; MSRV unchanged.
- **`mkui-vector2d-wgpu` renders `VectorPath` + `Stroke` — Sprint 8 substrate
  Wave 1 (#138).** The native WGPU adapter now draws arbitrary vector geometry
  through the *unchanged* Slug coverage pipeline — the WGSL shader is untouched
  because it is already a general quadratic-curve rasterizer; only CPU
  prepare-time work is added:
  - **`PlacedVectorPath` fill lane** — an arbitrary `VectorPath` placed on
    screen (origin + uniform scale + colour) is encoded at
    `SlugAdapter::prepare_paths` time via `encode_vector_path` (cubics
    subdivided) and packed into the identical curve/band/glyph buffers as a
    glyph blob.
  - **`PlacedStroke` stroke lane** — a stroked centreline is expanded to a
    fillable non-zero outline by the new `mkui-vector2d`
    `stroke_expand::stroke_to_fill` (Butt/Round/Square caps, Miter/Round/Bevel
    joins with an SVG-default miter limit, and CPU-side dash-pattern splitting),
    then encoded like any other fill. Overlapping convex pieces union
    seam-free under the pipeline's analytic winding coverage.
  - The existing `PlacedSlugGlyph` glyph lane, `pack`, and `SlugAdapter::prepare`
    are byte-identical; both new lanes share `prepare_packed` so every lane hits
    the same buffer/bind-group/draw path. Zero dependency on `mkui-wgpu`; no new
    dependencies beyond `mkui-vector2d` + `wgpu` + `bytemuck`.

### Changed
- **Bridge `Mkui` rustdoc — audit round-7 Phase 1 (#144).** Added `///`
  rustdoc with `# Examples` sections to the bridge `Mkui::new`, `Mkui::child`,
  and `Mkui::run` methods in `crates/mkui/src/lib.rs`, so adopters landing on
  crates.io see the `new` → `child` → `run` builder flow instead of bare
  signatures. Examples use the real public API (`Text`, `Button`, `View` from
  `mkui_core`), compile under `cargo test --doc -p mkui`, and cross-reference
  the relevant `mkui-core` component types. Documentation-only; no public API
  change.
- **Gamma-correct alpha blending in linear color space (#135).** The wgpu
  backend now renders every lane (UI triangles, bitmap text, Slug curves) into
  a **linear-space intermediate framebuffer** (`Rgba8Unorm`) and encodes
  linear→sRGB once, in a final full-screen present pass, at the surface
  boundary. Alpha blending therefore composites in physically-correct linear
  light. Previously the UI lane pre-linearized its vertex colors but the Slug
  outline-text lane did not linearize its glyph color at all, so anti-aliased
  edges composited in sRGB-encoded space and darkened — visible as the
  "bitmap-enhanced" look on `examples/text --features slug`. `Color` is now
  documented as an sRGB-perceptual value converted to linear exactly once at the
  render boundary (`Color::to_linear_rgba`); `Color::from_srgb` /
  `from_srgb_a` are added as intent-signalling constructors. A Lavapipe GPU
  acceptance test reads back raw bytes proving an authored sRGB `0.5` gray
  linearizes to byte ~55 (not the byte ~128 of the old pipeline). The Sprint 6
  v0.9.1 MSAA-resolve-into-sRGB double-encode is closed by construction — the
  intermediate is linear, so no sRGB texture sits in the compositing path. No
  new dependencies, no MSRV change. See ADR 0006 §"Color space + blending".
  **Reintroduced after revert `f8da740`:** the original PR #149 shipped with
  29 green CI checks and immediately panicked on macOS Metal at first UI draw
  (`bind group index 0, the buffer bound at binding index 2 is bound with
  size 16 where the shader expects 32`) — `present.wgsl`'s `PresentFlags`
  uniform padded `encode_srgb: u32` with a `vec3<u32>` field, which WGSL's
  host-shareable layout rules align to 16 bytes and round the whole struct up
  to 32 bytes, while the CPU side (`create_present_flags_buffer`) only ever
  uploaded a flat 16-byte `[u32; 4]`. Fixed by replacing the `vec3<u32>` pad
  with three scalar `u32` fields, keeping the struct a true 16 bytes on both
  sides (`crates/mkui-wgpu/src/render/present.wgsl`). A new
  `present_flags_uniform_size_matches_wgsl_layout` test
  (`crates/mkui-wgpu/src/render/mod.rs`) computes `PresentFlags`'s size via
  naga's own layout algorithm and asserts it matches the CPU buffer, without
  needing a GPU. CI's Vulkan Validation Layers enforcement (#153/#154) is the
  safety net going forward: the `gpu-offscreen` job now fails outright if a
  bind-group/buffer-size mismatch like this one reaches Lavapipe undetected.

### Tooling
- **Vulkan Validation Layers on the Lavapipe `gpu-offscreen` job (#153).**
  PR #149 (gamma-correct alpha blending) shipped with 29 green CI checks and
  immediately panicked on macOS Metal at first UI draw; it was reverted at
  `f8da740`. Root cause: the `gpu-offscreen` job's Lavapipe adapter had no
  Vulkan validation layer manifest installed, so a `wgpu` bind-group/layout
  mismatch that Metal rejected at runtime was negotiated without complaint in
  CI. `.github/workflows/ci.yml` now installs `vulkan-validation-layers`
  alongside the Mesa/Lavapipe ICD and adds a load-bearing (non-`|| true`)
  log-grep step that fails the job if `VK_LAYER_KHRONOS_validation` is not
  confirmed active in the Vulkan loader trace, so this gap cannot silently
  reopen. `crates/mkui-wgpu/src/render/vvl_regression.rs` adds a companion
  regression test (`gpu-tests` feature, Lavapipe-only) that binds an
  undersized uniform buffer and asserts `wgpu` raises a validation error —
  documented as testing `wgpu-core`'s own always-on validation rather than
  VVL specifically (VVL enforces a different, Vulkan-driver-specific class of
  check that this CPU-side test cannot exercise); the CI log-grep step is the
  actual VVL-active sentinel. **Known limitation:** neither mechanism
  reproduces Metal-only validation behavior — this closes the "CI ran a real
  Vulkan pipeline with no validation enforcement at all" gap, not every class
  of backend-specific bug #149 could have hit.
- **CI cost reduction — fail-fast + path-gated code jobs + `[skip ci]`
  discipline (#151).** GitHub Actions billing hit ~$14 over three days of the
  Sprint 7 tail; at Sprint 8 pace an unchanged pipeline projected to $50–100.
  Four cheap wins in `.github/workflows/ci.yml` (and contributor docs):
  - The package-aware `test` matrix flips from `fail-fast: false` to
    `fail-fast: true` — a failing feature combo now stops the matrix instead of
    letting the other 13 legs run to completion (~30–50% less on a red run).
  - A new lightweight `changes` job computes whether a PR touches code paths
    (`crates/`, `examples/`, `bindings/`, workflows, Cargo manifests,
    toolchain/deny config) and gates the expensive code jobs — the two Lavapipe
    GPU lanes, `mkui-py-bindings`, `feature-slug`, `mkui-backend-matrix`,
    `mkui-examples`, `build-release`, `msrv`, and `cbindgen-header-clean` — with
    `needs: changes` + `if:`. A docs-only PR (README / CHANGELOG / ADR refresh)
    now runs ~20 jobs instead of ~28; code PRs are unchanged (full coverage
    preserved). Job-level `if:` skips report as "skipped" (branch-protection
    treats them as success), so gating never wedges a required check.
  - `docs/CONTRIBUTING.md` documents the `[skip ci]` iteration convention: mark
    intermediate review-round fix commits `[skip ci]`, drop it on the final
    push so CI runs once against the state that merges.
  - **Before/after (per PR):** docs-only PR ~28 → ~20 jobs (skips both GPU
    lanes + bindings ≈ the run's most expensive minutes); a red code PR saves
    the wasted matrix legs; a 4-commit review round that marks the first three
    `[skip ci]` pays for one CI run instead of four. No public API change, no
    MSRV bump, no new dependencies; `ci.yml` remains a single file.

## [0.10.0] — 2026-07-03

### Added
- **SFNT text-system integration — Phase 1, CPU-side (#67).** A narrow,
  from-scratch SFNT/TrueType decoder in `mkui-text` (`SfntFace`) reads the
  licensed Abel-Regular fixture — table-directory validation, `head`/`maxp`/
  `hhea`/`hmtx`/`cmap` format 4/`loca`/`glyf`/`name`, Unicode→glyph mapping,
  advances, and simple quadratic outlines (font units, y-up) — and rejects
  CFF/CFF2, color/bitmap, composite, malformed, and out-of-range inputs with
  typed `SfntError`s. No runtime font-stack dependency (`ttf-parser` is a
  `[dev-dependencies]` parity oracle only). `CompositeTextSystem::register_sfnt_face`
  registers the face as a `TextRenderClass::Slug` provider beside the built-in
  bitmap face through #62's registry-issued `FontId`s, with layout-time mixed
  fallback honoring `RoutedRun::Fallback(ValidatedFallback)` end-to-end (real
  SFNT + bitmap, cluster order/advances/positions preserved). The Slug CPU
  vertical slice flows decoded outlines through #65's `mkui-vector2d` encoder
  into a size-independent blob reused across 12/16/24/48 px, and glyph-`M`
  calibration proves the Tier-2 baseline-diff threshold feasible. Phase 2 wires
  that outline output into #66's GPU Slug lane: `mkui-wgpu::place_slug_run` turns
  a Slug `LayoutRun` into `PlacedSlugGlyph` primitives, and font-backed offscreen
  readback tests (on #106's Lavapipe harness) render glyph `M` at 12/16/24/48 px
  and assert at least the calibrated changed-pixel count inside its decoded ink
  rectangle. A cross-provider test composes a real SFNT-Slug run and a bitmap
  fallback run through the renderer's ordered command stream, and `examples/text
  --features slug` renders Abel through the Slug lane beside a bitmap label.
- **`mkui-vector2d-wgpu` adapter + ordered Slug render lane (#66).** A new
  acyclic crate packs #65's backend-neutral Slug glyph blobs into WGPU storage
  buffers and renders them through a single-horizontal-ray WGSL coverage
  pipeline (one dilated quad per glyph; band membership/ordering consumed
  verbatim from `mkui-vector2d`, never recomputed). The crate depends only on
  `mkui-vector2d` + `wgpu` — never on `mkui-wgpu` — and owns no surface, adapter
  selection, or frame lifecycle. `mkui-wgpu` gains a default-off `slug` Cargo
  feature (forwarded by `examples/atoms-on-wgpu`); off-feature builds stay
  bitmap-only and v0.9.3-compatible. The renderer now derives an ordered
  `RenderCommand` stream from `Scene::primitives` — preserving paint order,
  coalescing only adjacent same-lane primitives — and draws UI/bitmap triangles
  and Slug glyphs in scene order within one pass. Scenes carry Slug glyphs via a
  new `Primitive::SlugGlyph` variant (the seam #67's outline text system emits
  into); `WgpuApp::with_app_tree_and_text_system` /
  `Mkui::from_core_with_text_system` let declarative apps supply and retain a
  custom/composite text system. Slug curve/band encoding reproduces the
  public-domain Slug contract (Eric Lengyel) from scratch. GPU acceptance tests
  run hand-authored records through #106's offscreen Lavapipe harness — no font
  parser (that is #67).
- **Composite text-system router + font registry in `mkui-text` (#62).**
  `CompositeTextSystem` lets bitmap and Slug (and, from #67, outline) faces
  coexist in one scene without renderer-global conditionals or provider-local
  `FontId` collisions. The registry owns the shared `FontIdAllocator` handle
  and is the only thing that mints public `FontId` values; it maps
  `(ProviderId, LocalFontId) → FontId` and routes every request — `resolve_font`,
  `layout`, `rasterize`, `glyph_outline` — by reverse lookup, handing providers
  only their private `LocalFontId`. The router never constructs a `FontId`.
  Provider-emitted mixed-fallback runs are preserved (a `RoutedRun` discriminator
  keeps each run's validated face) so #67's layout-time SFNT/bitmap fallback
  contract works end-to-end rather than being flattened onto one face.
  `FontSource` models a face's origin separately from its `TextRenderClass`.
  Consumes #61's canonical identity/render/outline contract rather than
  redefining it. Generic-only: no SFNT parser, TTF bytes, Slug encoder, or WGPU
  type lands here.
- **`mkui-vector2d` crate — backend-neutral 2D paths + Slug glyph encoder (#65).**
  New workspace crate owning the CPU/path half of the Sprint 7 vector text
  lane (per the #64 ownership ADR): a renderer-independent `VectorPath`
  model and a deterministic Slug-style glyph curve/band encoder. The encoder
  emits backend-neutral records — glyph bounds, quadratic curve records
  `(p0, p1, p2)`, horizontal/vertical band ranges, and curve-index streams,
  all in font units y-up — cached by the collision-free `SlugGlyphKey`. It
  consumes `mkui-text`'s canonical identity and outline contract from #61
  (`FontId`, `Fixed16_16`, `Affine2Fixed`, `VariationSettings`, `OutlineKey`,
  `GlyphOutline`) rather than redefining them. Implements the public-domain
  Slug algorithm by Eric Lengyel from scratch (no Slug source). No WGPU type
  appears in the crate or its dependency tree; GPU packing is #66's scope.

### Changed
- **Audit round-6 P1: rationale comments on the two `too_many_arguments`
  allows in `mkui-wgpu` components (#104).** `components/chip_group.rs` and
  `components/dot.rs` each carry a `#[allow(clippy::too_many_arguments)]`; their
  explanatory comments now use the `// allow:` prefix, matching the existing
  walker-style pattern (`walker.rs:418`). The rationale text is preserved
  verbatim (signatures mirror `button` / `badge`; a param-struct would diverge
  from the components/atom call shape without clarity gain). Comment-only — no
  signature, public API, dependency, or MSRV change; clippy stays clean.
- **Breaking (pre-1.0): `mkui` enforces exactly one primary backend feature
  at compile time (#102).** The bridge crate (`crates/mkui`) previously
  resolved a build with multiple primary backend features enabled (`web`,
  `console`, `wgpu`) by hidden `cfg` precedence — `console` silently shadowed
  `web`, which shadowed `wgpu`. Cargo feature unification across a dependency
  graph could therefore select a different backend than a downstream consumer
  intended, with no diagnostic. The bridge now emits a `compile_error!` for
  every conflicting pair (and the all-three combo):

  > `mkui: enable exactly one primary backend feature: \`web\`, \`console\`, or \`wgpu\``

  There is no silent precedence remaining; each valid single-backend build has
  exactly one `inner` field and implementation path. The no-backend build is
  still valid (`Mkui::new()` returns a clear `MkuiError` naming which feature
  to enable). This is a **breaking change** for any consumer relying on
  accidental multi-feature precedence — enable exactly one of `web`,
  `console`, or `wgpu`. No runtime backend-selection API, dependency, or MSRV
  change. ADR 0006 documents the invariant; a package-aware CI job
  (`mkui-backend-matrix`, never workspace `--all-features`) asserts the four
  valid builds compile and the four conflicting combos fail.

### Security
- **Bump pyo3 0.28.3 → 0.29.0 (#114).** Resolves two RUSTSEC advisories
  published 2026-06-11 against pyo3 0.28.x, both fixed in 0.29.0:
  - **RUSTSEC-2026-0176** — out-of-bounds read (memory exposure) in
    `BoundListIterator`/`BoundTupleIterator` `nth`/`nth_back`, where a large
    `n` could overflow the unchecked `index + n` addition before the
    bounds-check (PyO3/pyo3#6086).
  - **RUSTSEC-2026-0177** — missing `Sync` bound on
    `PyCFunction::new_closure` closures, allowing concurrent closure
    execution (especially under free-threaded Python) without a `Sync`
    guarantee (PyO3/pyo3#6096).

  mkui-py does not call any of the affected APIs directly, so the bump is a
  pure dependency update with no source changes. Unblocks `cargo-deny` +
  `cargo-audit` CI, which treat `vulnerability` as `error`.
- **Bump memmap2 0.9.10 → 0.9.11 + ignore three build-time-only advisories
  (#132, PR #133).** Resolves RUSTSEC-2026-0186 (memmap2 unsound
  `pointer::offset` in `Mmap::[unchecked_]advise_range`) via a
  cargo-resolver-reachable patch bump within `^0.9`. Three additional
  advisories published 2026-06-28/29 against the Wayland/Linux
  windowing chain — RUSTSEC-2026-0194 + RUSTSEC-2026-0195 (quick-xml 0.39.4
  DoS: quadratic parse + unbounded namespace-declaration allocation) and
  RUSTSEC-2026-0192 (ttf-parser 0.25.1 unmaintained, informational) — are
  ignored in both `deny.toml` and `.cargo/audit.toml` with detailed
  rationale: quick-xml is reached only at BUILD TIME via `wayland-scanner`
  (`proc-macro = true`) parsing static, trusted Wayland protocol XML
  bundled with `wayland-protocols` — its `CVSS:AV:N` threat model
  (remote network attacker with crafted XML) does not apply; ttf-parser
  is reached at runtime only via `sctk-adwaita → ab_glyph` for
  window-decoration font parsing on Linux/Wayland (system fonts, not
  user input) plus a `[dev-dependencies]` parity oracle in `mkui-text`
  per Sprint 7 plan v10 §3.6 (test-only). All three transitive pins are
  out of reach of `cargo update` (`wayland-scanner` pins `quick-xml
  ^0.39`; `ab_glyph` pins `owned_ttf_parser` pins `ttf-parser`);
  clean upstream removal path tracked in #132 with soft 2026-10-01
  re-review deadline.
- **Completed `showcase-common` wgpu action arms and corrected example run
  commands (#94).** All seven shared-showcase button actions (GitHub, Primary,
  Secondary, Destructive, Outline, Ghost, Source Code) previously carried only
  `web` and `console` cfg arms, leaving native WGPU interaction incomplete; each
  now has a `#[cfg(feature = "wgpu")]` arm with a diagnostic print mechanically
  equivalent to its console arm (the current runtime tree has no stateful visual
  action for these buttons). A source-level test in `showcase-common` asserts all
  seven arms are present. The three WGPU package binaries (`atoms-on-wgpu`,
  `native-showcase`, `native-window`) documented the invalid
  `cargo run --example <name>` form even though they are workspace packages; their
  doc comments — plus the matching `README.md` invocations — now use
  `cargo run -p <package> --release`. No public API, dependency, or MSRV change;
  bridge multi-feature precedence semantics remain out of scope (#102).
- **FFI panic safety in `mkui-c` (audit round-6 P1, #103).** A Rust panic
  unwinding across an `extern "C"` boundary is undefined behaviour. Two fixes
  close that gap. (1) `MkuiResult::error` dropped its nested
  `CString::new(...).unwrap()` fallback for `unwrap_or_default()`, so ordinary
  error construction has no panic path from the `CString` fallback. (2) All
  **12** exported `extern "C"` functions now run their body inside
  `std::panic::catch_unwind(AssertUnwindSafe(...))` via a private `ffi_guard`
  helper; a caught panic returns the type-appropriate, allocation-free fallback
  (`null`/`null_mut` for pointers, `invalid_node()`/`invalid_action()` for the
  id mirrors, and a non-allocating `MkuiRuntimeError` + null-message
  `MkuiResult` — never `MkuiResult::error`, which allocates). Best-effort
  logging on the recovery path is static-message-only and itself wrapped in a
  nested `catch_unwind`, so a logging failure cannot escape the ABI. This
  guards Rust panics using mkui's own unwind runtime; it does **not** claim to
  catch process aborts, foreign-language exceptions, or a panic/exception
  crossing an `extern "C"` callback boundary. Panic-injection tests exercise
  every one of the 12 wrappers through a `#[cfg(test)]`-only hook (no Cargo
  feature); release/public builds contain no injection hook or new exported
  symbol. No new `MkuiErrorCode` variant, no `MkuiResult::Panicked`, no
  C-header change, and no new dependency or MSRV change — the cbindgen
  header-drift check and cross-binding parity tests stay green.
- **macOS fast-resize jerk fixed at the presentation layer:
  `CAMetalLayer.presentsWithTransaction` (#101).** The fast-shrink-direction
  vibration (right→left, bottom→top) on macOS live-resize is a **presentation-
  layer race**, not an event-scheduling one. AppKit commits a window's new
  bounds inside a `CATransaction`, but `CAMetalLayer` defaults to
  `presentsWithTransaction = false` — it presents drawables on an independent
  asynchronous timeline, so during a resize Core Animation stretches the
  previous (stale-size) drawable into the new window rect for one or more frames.
  Two earlier event-layer attempts (v0.9.3's `about_to_wait` pump; the
  `CursorMoved` M2 bridge below) could not close it because they change only
  *when* redraws are requested, not how the drawable commits.

  Fix (`render::enable_presents_with_transaction`, macOS-only): reach the
  surface's `CAMetalLayer` via `wgpu::Surface::as_hal::<hal::api::Metal>()` and
  set `presentsWithTransaction = true` once after `configure`. wgpu-hal's Metal
  present path then commits the command buffer, `waitUntilScheduled()`, and calls
  `drawable.present()` inside the current `CATransaction`, so drawable + window
  rect resize atomically — no stretch frame. This is the sole `unsafe` seam in
  the crate: `mkui-wgpu` moves from `#![forbid(unsafe_code)]` to
  `#![deny(unsafe_code)]` with a single scoped `#[allow(unsafe_code)]` +
  `SAFETY:` note on that FFI call; every other module stays unsafe-denied. The
  call is `#[cfg(target_os = "macos")]`-gated (Windows/Linux keep the standard
  wgpu present path, unchanged), uses no new dependencies (the `objc2-quartz-core`
  /`wgpu-hal` Metal path is already in the tree), and changes no public API.
  Verified by operator visual-verify on Retina; not unit-testable without a live
  Metal surface (displayless CI has none). References: Tristan Hume "Glitchless
  Metal Window Resizing"; Raph Levien "The smooth resize test"; wgpu#1168;
  winit#3644.
- **wgpu `CursorMoved` M2 bridge during the active-resize window (#101, event
  layer — orthogonal hygiene; retained, not the root-cause fix).** PR #100 (#99)
  shipped the `about_to_wait` resize redraw pump (cap=60, `Drawn`-decrement,
  `Skipped`-no-burn). This adds a second redraw trigger — **mechanism M2**: a
  `CursorMoved` event inside a recent-resize-active window calls
  `window.request_redraw()` directly, **independent of the pump's counter**,
  giving the redraw cadence a cursor-driven trigger to complement the pump's
  `Resized`-arm trigger. It is deliberately *not* M1 (re-arming the pump): the
  active-resize-window guard is an explicit frame-count timeout
  (`RESIZE_ACTIVE_WINDOW_FRAMES = 60`, ~1s at 60Hz), **not** the circular
  `resize_redraw_pending > 0` predicate (Codex 2026-06-09 #101 review P1#2).
  `about_to_wait` advances a monotonic `frame_counter`;
  `Resized`/`ScaleFactorChanged` stamp `last_resize_frame`; the window closes on
  the timeout so idle pointer motion stays quiescent. The bridge is **macOS-only**
  — compile-time gated with `#[cfg(target_os = "macos")]`, folding to a constant
  `false` on Windows/Linux (those platforms unchanged); the active-resize-window
  state machine is shared infra, compiled + unit-tested on every native target.
  This work keeps redraw cadence sane during a gesture but, per operator
  visual-verify, is **not** what eliminates the jerk — the
  `presentsWithTransaction` fix above is. No public API change, no new deps, no
  MSRV bump. ADR 0006 §"Resize-active redraw pump" documents both layers.
- **wgpu live-resize jerk eliminated on macOS (#99).** PR #98 (#97) fixed
  the HiDPI viewport-units math but the visible scale-snap on shrinking
  resize gestures (right→left, bottom→top) persisted. Root cause was at
  the event-loop redraw-cadence layer: mkui-wgpu had no `about_to_wait`
  handler, so during continuous resize gestures the OS could present
  frames at the new layer size before the next rendered frame caught
  up — visible as a Metal swapchain transition stretch.

  Adds a narrow `about_to_wait`-driven resize redraw pump (60-frame cap,
  arm on `Resized` + `ScaleFactorChanged`). The pump's decay measures
  *presented* frames (`RenderOutcome::Drawn`) rather than event-loop ticks,
  so skipped frames under GPU pressure don't accidentally drain the bridge
  during fast live-resize gestures. The 60-frame cap (~1s at 60Hz) keeps
  the pump saturated through fast gestures — each `Resized` re-arms it —
  then decays back to idle once the drag stops; a smaller cap, or a
  tick-counted one, exhausted faster than fast drags presented frames and
  left the jerk visible (tuned 4 → 60 across Sprint 6.5; a 120-frame cap was
  tested and found *worse* by operator visual verify — likely swapchain
  saturation from over-queued redraws — so the cap stayed at 60). A small
  residual fast-drag vibration remains at 60; it is **not** pump exhaustion
  (the 120 result refutes that) and is tracked as a known issue (#101) for a
  follow-up `CursorMoved`-armed bridge during the active-resize window. Resize handlers only arm the pump; `about_to_wait`
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

  **Known issue (residual):** fast shrink-direction drags (right→left,
  bottom→top) still show a small residual vibration that is NOT fully
  resolved by this pump. Three hypotheses were ruled out during the
  v0.9.3 cycle (Sprint 6.5 close, 2026-06-09):
  - Pump-cap exhaustion (cap=60 and cap=120 produced the same residual;
    cap=120 was actually worse, suggesting GPU/swapchain saturation
    above the operator-verified sweet spot)
  - Scene-complexity by primitive count (native-showcase has the largest
    tree but vibrates least among the three wgpu examples)
  - Absolute-vs-relative primitive coords during resize (modifying
    native-window's quad to viewport-relative coords did not change the
    visible vibration)

  The residual root cause remained undiagnosed at v0.9.3 ship time. #101
  (above, this release) addresses it with the substrate-tier C2 expansion —
  the CursorMoved M2 redraw bridge during the active-resize window — rather
  than a further cap tune.
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

### Tooling
- **Package-aware CI feature matrix (#105).** The `test` job in
  `.github/workflows/ci.yml` is now a per-crate strategy matrix instead of a
  single default-feature `--workspace` run, so feature-gated code (e.g.
  `mkui-runtime`'s `snapshot`, `mkui-core`'s `serde`) is exercised where it
  lives instead of rotting undetected. Compatible crates (`mkui-core`,
  `mkui-runtime`, `mkui-text`, `mkui-web`, `mkui-console`, `mkui-vector2d`,
  `mkui-vector2d-wgpu`) run `--all-features` per-crate; `mkui-wgpu` compiles
  `--all-features --no-run` (its `gpu-tests` need the Lavapipe `gpu-offscreen`
  job to actually run). The `mkui` bridge is tested with each primary backend
  (`web` / `console` / `wgpu`) and `--no-default-features` separately — never
  combined, which would trip the #102 one-backend `compile_error!`. Workspace
  `--all-features` is deliberately NOT used (it fails structurally per the
  Codex Sprint 7 v5-review: the bridge's mutually-exclusive backends and
  `mkui-py`'s interpreter-linked vs. `extension-module` link modes). `mkui-c`
  is tested per-backend for the same one-backend reason. `mkui-py` continues to
  run its dedicated interpreter-linked `mkui-py-bindings` job, not duplicated
  here.
- **Surfaceless offscreen GPU readback harness (#106).** Adds
  `mkui_wgpu::render::offscreen::OffscreenRenderer`, a reusable
  window-less renderer that owns a surfaceless adapter/device/queue, an
  `Rgba8Unorm` render target with `RENDER_ATTACHMENT | COPY_SRC`, and a
  readback buffer padded to `wgpu::COPY_BYTES_PER_ROW_ALIGNMENT` (256-byte)
  row alignment. It exposes submission, explicit device polling, buffer
  mapping, and row-unpadding so native WGPU pipelines can be tested
  deterministically without opening a window (the window-bound `Renderer`
  exits before adapter creation under `HEADLESS=1`). The window-bound
  `Renderer` API is unchanged; the shared `DeviceDescriptor` is the only
  refactor.

  The harness and its tests are gated behind the `gpu-tests` cargo feature
  so the default displayless CI `test` job needs no Vulkan ICD. A dedicated
  `gpu-offscreen` CI job provisions `mesa-vulkan-drivers vulkan-tools` and
  pins `VK_ICD_FILENAMES` to Mesa/Lavapipe; the harness selects
  `Backends::VULKAN`, asserts a Vulkan + CPU adapter (logging the adapter
  name diagnostically), and treats adapter/device unavailability as a test
  failure rather than a silent skip. Generic tests cover clear-and-read-back,
  a solid-color triangle vs. a clear-only baseline, non-256-aligned row
  widths, and map completion via explicit polling. Unblocks the GPU
  acceptance tests in #66 (Slug pipeline) and #67 (TTF/font), which own
  their own backend-specific tests.

## [0.9.3] — 2026-06-10

### Fixed
- **macOS live-resize jerk — `about_to_wait`-driven resize pump (#99, PR #100).**
  Post-v0.9.2 the wgpu examples rendered correctly on first paint but visibly
  jerked on resize with an asymmetric shrink-direction-only pattern (bottom→top +
  right→left). Root cause was architectural divergence from wgpu+winit reference
  apps: mkui-wgpu lacked an `about_to_wait` handler driving continuous redraws
  during resize, so swapchain transitions caught the render pump between input
  events. Fix ships a narrow `about_to_wait`-driven resize pump —
  `resize_redraw_pending: u8` armed on `Resized` + `ScaleFactorChanged` to a
  `RESIZE_REDRAW_PUMP_TICKS` cap; `about_to_wait` requests redraws while armed;
  decays via `RenderOutcome::Drawn`-only-decrement (`Skipped` frames under GPU
  pressure never drain the bridge). Non-monotonic cap iteration during PR #100
  (4 → 60 → 120 → 60) confirmed cap=60 as the operator-verified architectural
  ceiling; cap=120 over-fired redraws during fast drag and saturated
  GPU/swapchain. Residual fast-drag vibration remains (tracked in #101 for
  v0.9.4+ CursorMoved-armed bridge substrate work — subsequently fixed at the
  presentation layer via `CAMetalLayer.presentsWithTransaction` in v0.10.0).

## [0.9.2] — 2026-06-09

### Fixed
- **Viewport units — logical vs physical pixels (#97, PR #98).** Post-v0.9.1 the
  wgpu examples rendered primitives but on Retina displays (`scale_factor=2.0`)
  they landed in the upper-left quadrant instead of expected positions.
  `Scene::viewport` was logical pixels after resize but `Renderer::render`
  projected vertices against `self.config.{width, height}` (physical pixels,
  wgpu's surface-config contract). Fix (Codex round-N Candidate B.2 via
  `scene.viewport`): project against `scene.viewport.{width, height}` (logical)
  instead of `self.config.{width, height}` (physical). No new `Renderer` field,
  no `scale_factor` plumbing. Added `logical_viewport_from_physical_size` helper
  at the resize/scale-change boundary, `WindowEvent::ScaleFactorChanged` handler,
  and 3 unit tests including fractional-scale (1.5×).

### Changed
- **ADR 0006 §"Viewport units contract" added.** `Scene::viewport` + primitive
  coords are **logical pixels** (matches web's CSS-pixel + console's
  character-grid conventions). Wgpu surface config stays physical. The
  cross-binding identity contract is now explicit.

## [0.9.1] — 2026-06-09

### Fixed
- **Wgpu examples first-paint + resize regressions (#93, PR #96).** Operator
  visual sanity-check post-v0.9.0 found both wgpu examples (`atoms-on-wgpu`,
  `native-window`) rendering only the clear color (gray) — primitives invisible.
  The wgpu backend had been visually unverified since v0.6.0 across four minor
  releases (0.6.0 → 0.7.0 → 0.8.0 → 0.9.0) because CI's `HEADLESS=1` smoke gate
  validates walker-doesn't-crash but NOT walker-emits-primitives-that-reach-screen.
  Three fixes ship in one PR (Codex round-N+1 BLOCK scope-fix per the
  substrate-tier 4-round cycle):
  - **`WindowEvent::Resized` clobbers `with_scene` primitives.** Resize
    unconditionally replaced `self.scene = Scene::new(new_viewport)`, wiping
    user primitives in `with_scene` mode before first paint (macOS fires a
    `Resized` post-creation). Fix: split per-path resize contract — AppTree
    scenes rebuild from tree; raw scenes preserve primitives + update viewport
    in place.
  - **First-paint `Skipped→retry` state machine.** `RenderOutcome::Skipped` was
    a no-op; if the initial `resumed()` `request_redraw` hit a not-yet-ready
    surface, the window stayed blank-gray until the user resized or interacted.
    Adds `first_paint_pending: bool` + `first_paint_skip_retries: u8` with retry
    cap, decay on `Drawn`, idle-quiescence preserved post-first-paint.
  - **MSAA disabled (`sample_count=1`).** Sprint 5's 4× MSAA path
    (no upstream reference) was the suspected source of resize-time darkening
    and atoms-on-wgpu rendering empty despite emitting 9012 valid triangles.
    MSAA-resolve-into-sRGB was double-applying sRGB encoding on macOS Metal.
    Disable pending correct sRGB orchestration (deferred to #95).

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
