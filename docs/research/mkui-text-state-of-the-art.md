# `mkui-text`: state of the art for Rust text rendering on wgpu/WebGPU

- **Project:** ui (`miklabs/ui`, `mkui`)
- **Path:** `/Users/mik/dev/mikbry/ui`
- **Date:** 2026-05-21
- **Audience:** Codex (adversarial code reviewer) + future-me.
- **Status:** Reference document backing a sprint-scope decision. Not a survey.

---

## Pre-implementation verification checklist (BLOCKING)

Per Codex review 2026-05-21, the verification caveat is **blocking** — the `mkui-text` implementation issue cannot be filed until each item is checked off and the result is amended back into this document. The conclusions in this doc do not turn on point-version detail, but the *evidence* does, and the doc's authority degrades fast as the ecosystem evolves.

The mkui-text issue body (or the agent's first commit on the mkui-text branch) must verify the following against current state (crates.io + GitHub) and amend this document with the verified versions and dates:

- [ ] **`cosmic-text`** — verify current crate version on crates.io, last commit date on `pop-os/cosmic-text`. The doc claims §3 are training-cutoff (January 2026). If the version has bumped or the API has shifted, update §3.1 and §5 accordingly.
- [ ] **`glyphon`** — verify current crate version, last commit date on `grovesNL/glyphon`, and confirm the `TextAtlas` / `TextRenderer` shapes described in §6 match the current public API. If glyphon's internal pipeline has been refactored, §6's "lift the structure, not the dep" call may need to swap to direct dependency or to a different reference.
- [ ] **`swash`** — verify current version, last commit on `linebender/swash`. Especially: confirm COLR/CBDT emoji rasterization is still supported (§3.2 claims it is).
- [ ] **`parley`** — verify whether parley has hit 1.0 or shipped a public GPU/atlas story since the doc's writing (cutoff: pre-1.0, no atlas). If yes, §1's rejection of parley needs re-examination.
- [ ] **GPUI's `PlatformTextSystem` trait shape** — confirm via the current `crates/gpui/src/text_system.rs` in `zed-industries/zed` that the trait surface described in §4 still matches. The doc's confidence in "cosmic-text now, platform-native later behind one trait" depends on GPUI's pattern being production-stable.
- [ ] **iced's cosmic-text wiring** — confirm `iced_wgpu/src/text.rs` still uses cosmic-text + glyphon (or that iced has switched to something else). The doc treats iced as the most direct precedent (§4); if iced has moved off the stack, that changes risk assessment.
- [ ] **shadcn-era assumptions** — the doc references shadcn naming/variant conventions; verify those references match the current `docs/components/mkui-to-shadcn-mapping.md` after its 2026-05-21 rewrite.

**The verification artifact**: amend this section in-place with a "verified: 2026-MM-DD by <author>, results: …" block before the mkui-text PR opens for Codex review. The PR description must link to the verification commit.

**Why this is blocking, not advisory:** the original draft of this doc framed the verification as a footnote (line 8). Codex flagged that as a P1 boundary violation: a decision doc cannot rest on unverified time-sensitive evidence. This section converts that footnote into a gate.

---

---

## 1. Decision summary

**`mkui-text` will wrap `cosmic-text` (which itself wraps `swash` + `rustybuzz` + `fontdb` + `unicode-bidi` + `unicode-script`), behind a `PlatformTextSystem` trait, and will render glyphs through an atlas pipeline modeled on `glyphon`.** That is the single technical commitment of this document.

The alternatives, and why each is rejected for Sprint-2-scoped `mkui-text`:

- **`glyphon` adopted as-is** — strongest near-term competitor; rejected because adopting it directly couples `mkui-text` to a specific wgpu pipeline shape (`TextAtlas`, `TextRenderer`, `Resolution`) that we do not yet want exposed to consumers. `glyphon` is used as the *reference* atlas implementation, not as the dependency. See section 6.
- **`parley` (Linebender)** — emerging, swash-based, very promising; rejected because as of cutoff January 2026 it is pre-1.0, has no atlas/GPU story of its own, and its layout model is in active flux (`linebender/parley`). Re-evaluate before Sprint 5.
- **`fontdue` / `ab_glyph` only** — pure-Rust rasterizers without shaping or fallback; rejected because they cannot render Arabic, Indic scripts, complex Latin (combining marks, contextual alternates), or emoji. egui chose this path and consistently lists it as a known limitation. See section 4.
- **Platform-native (CoreText / DirectWrite / Pango) directly** — what GPUI does on macOS and Windows; rejected for Sprint 2 because (a) it is at least three sprints of work to implement three backends, (b) wasm32 has no platform-native option, and (c) the trait-based swap path (section 5) keeps this open for Sprint 6+ without blocking anything today.
- **Vello + Skrifa (Linebender's vector-on-GPU stack)** — rejected because Vello is a *2D vector renderer*, not a text crate; using it for text means buying its entire compute-shader pipeline. Out of scope for an UI-library text layer. See section 3.

Why this matters: the call is not "cosmic-text is best in 2026"; the call is "**cosmic-text is the cheapest commitment that unblocks `mkui-wgpu`'s text in Sprint 2 without foreclosing platform-native later.**" The trait shape in section 5 is therefore load-bearing — Codex should review that first.

---

## 2. Background: what "text rendering" actually means

The field has six layers. Most ecosystem confusion (egui-vs-iced-vs-Zed comparisons) stems from comparing crates that cover different subsets of them. The trait in section 5 is shaped by these layers, so it is worth listing them explicitly before the survey.

| # | Layer | What it does | What it needs | Failure mode if missing |
|---|---|---|---|---|
| 1 | **Font discovery** | Map family name + style ("SF Pro Regular") to file paths / system handles | OS APIs (`fontconfig`, `CoreText`, `DirectWrite`) or a directory walker | "font not found", silent fallback to default sans |
| 2 | **Font parsing** | Read SFNT containers, extract tables (cmap, glyf, CFF, GSUB, GPOS, COLR, CBDT, sbix) | TrueType/OpenType spec compliance | Garbled glyphs, missing characters |
| 3 | **Shaping** | Turn a Unicode string + font + script + direction into a sequence of `(glyph_id, x_advance, y_advance, x_offset, y_offset)` | HarfBuzz-equivalent with full OpenType GSUB/GPOS | Arabic without ligatures, Devanagari with wrong cluster ordering, broken kerning, missing emoji ZWJ sequences |
| 4 | **Layout** | Break shaped runs into lines respecting wrapping, bidi (UAX #9), justification, and tab stops | Line-breaking (UAX #14), bidi reordering, font metrics | Right-to-left text rendered LTR; words split mid-grapheme |
| 5 | **Rasterization** | Render a glyph at a specific pixel size into an alpha (or color) bitmap, with optional hinting and subpixel positioning | Outline rasterizer (CFF/TrueType), COLR/CBDT/sbix for color | Pixelated edges, wrong stem widths, no emoji color |
| 6 | **Atlasing + GPU sampling** | Pack rasterized glyphs into a texture and emit textured quads | Shelf or skyline packer, texture atlas, eviction policy, an alpha/SDF shader | Texture exhaustion, glyph thrash, blurry text |

The mapping below is what makes the survey in section 3 readable:

| Crate | 1 (discover) | 2 (parse) | 3 (shape) | 4 (layout) | 5 (raster) | 6 (atlas) |
|---|---|---|---|---|---|---|
| `fontdb` | ✓ | (delegates to `ttf-parser`) | — | — | — | — |
| `ttf-parser` | — | ✓ | — | — | — | — |
| `rustybuzz` | — | (uses `ttf-parser`) | ✓ | — | — | — |
| `swash` | — | ✓ (internal) | ✓ (own shaper) | partial (LTR line builder) | ✓ (CFF, TT, COLR) | — |
| `cosmic-text` | ✓ (via `fontdb`) | ✓ (via `ttf-parser`/`swash`) | ✓ (via `rustybuzz`) | ✓ (own buffer + bidi) | ✓ (via `swash`) | — |
| `glyphon` | (delegates to cosmic-text) | (delegates) | (delegates) | (delegates) | (delegates) | ✓ (own `TextAtlas`) |
| `fontdue` | — | ✓ (own) | — | — | ✓ | — |
| `ab_glyph` | — | ✓ (own, via `owned_ttf_parser`) | — | — | ✓ | — |
| `harfbuzz_rs` | — | — | ✓ (C lib) | — | — | — |
| `parley` | ✓ (via `fontique`) | (via `skrifa`) | ✓ (via `swash`/`skrifa`) | ✓ (own) | (delegates) | — |
| `vello` | — | — | — | — | ✓ (compute-shader path) | (own scene-graph) |

The trait in section 5 owns layers 1-5 behind one interface and lets `mkui-wgpu` own layer 6. The reason for that split is the next decision Codex will challenge — see section 6.

---

## 3. State of the Rust text-rendering ecosystem (2025-2026)

> **Verification flag.** Version numbers and last-commit dates here come from the assistant's training data (cutoff January 2026). Codex should re-verify via `cargo info` or crates.io. The conclusions hold even if minor versions bumped, but a crate dormant 12+ months at review time is decision-affecting.

### `cosmic-text` — https://github.com/pop-os/cosmic-text

- Last known version family: **0.12.x** (active commits through end of 2025). "A pure Rust multi-line text handling library" (README).
- Bundles `rustybuzz` (shape), `swash` (raster), `fontdb` (discovery), `unicode-bidi` (UAX #9), `unicode-script`, `unicode-segmentation`, `self_cell`.
- Used by iced, the COSMIC desktop, Zed *on Linux* (see §4), Slint (since 1.3).
- Limitations relevant to mkui:
  - **`fontdb` is filesystem-based on every platform**, including macOS/Windows where the OS exposes richer metadata via CoreText/DirectWrite. The canonical reason GPUI did not pick cosmic-text on mac/win (§4).
  - No native bidi-aware caret hit-testing API (computable from `LayoutGlyph`, but not one call).
  - No on-the-fly variation-axis interpolation — variable fonts work, axis sweeps require a new face. Acceptable for UI; not for type design.
  - Color emoji renders via swash's COLR/CBDT, but emoji *shaping* is only as good as the bundled font's GSUB ZWJ sequences.

### `swash` — https://github.com/linebender/swash (formerly `dfrg/swash`, transferred 2024)

- Parser + own-shaper + rasterizer in one crate. Author Chad Brokaw (`@dfrg`).
- Native color-emoji (COLR v0/v1, CBDT, sbix, OT-SVG). Variable-font axis support.
- cosmic-text uses swash for rasterization but `rustybuzz` for shaping — swash's shaper has narrower OpenType GSUB/GPOS coverage than HarfBuzz.
- **Status caveat:** swash development slowed in 2023–2024 after the author moved focus to `skrifa`/`fontique` (Linebender's newer parser/discovery crates backing `parley`). Still maintained; innovation moved upstream.

### `rustybuzz` — https://github.com/harfbuzz/rustybuzz (transferred to HarfBuzz org in 2023)

- A complete port of HarfBuzz's shaper to safe Rust, passing HarfBuzz's own testsuite.
- The reason cosmic-text (and therefore mkui-text) can claim correct Arabic, Devanagari, Khmer, Thai, and emoji ZWJ without C dependencies.
- Lags HarfBuzz by some versions on new OpenType features (CFF2 hinting, COLRv1 paint expressions, OT-SVG). Invisible for UI text in mainstream fonts.

### `fontdb` — https://github.com/RazrFalcon/fontdb

- Read-only font database. Walks system font directories, parses with `ttf-parser`, queries by family/style/weight/stretch.
- **Weakest link of the cosmic-text stack on mac/win.** Does not call `CTFontCollection` or `IDWriteFontCollection`; walks `~/Library/Fonts`, `/Library/Fonts`, `/System/Library/Fonts` (macOS) or `C:\Windows\Fonts`. Misses app-bundled fonts. Codex should flag as known debt; the trait in §5 lets us swap.

### `fontdue` — https://github.com/mooman241/fontdue

- Pure-Rust rasterizer only. **No shaping. No fallback. No discovery.** Cannot render Arabic ligatures, Devanagari conjuncts, combining marks, color emoji, variable fonts, OT features.
- Adopted for compile time and minimal deps; viable for Latin-only HUDs and debug overlays, not for the Operator Console's catalog.

### `ab_glyph` — https://github.com/alexheretic/ab-glyph

- Same family as fontdue — rasterizer over `owned_ttf_parser`, no shaping. egui's current text path (§4).

### `ttf-parser` — https://github.com/RazrFalcon/ttf-parser

- Zero-allocation, `no_std` TrueType/OpenType parser. Reads cmap, glyf, CFF, CFF2, GSUB, GPOS, COLR, CBDT, sbix. Foundation for `fontdb`, `rustybuzz`, parts of `swash`. Boring infrastructure; rarely broken.

### `harfbuzz_rs` — https://github.com/manuel-rhdt/harfbuzz_rs (largely dormant)

- Safe Rust bindings to C HarfBuzz. Requires linking C HarfBuzz; wasm32 requires building HarfBuzz to wasm32, adding C-toolchain deps our matrix doesn't carry. `rustybuzz` is strictly easier and sits inside the HarfBuzz org since 2023.

### `glyphon` — https://github.com/grovesNL/glyphon

- **The most important crate in this document for mkui.** "Fast, simple 2D text rendering for wgpu" — a thin wgpu frontend that owns `TextAtlas` + `TextRenderer` and delegates everything above layer 6 to cosmic-text. The only production-tested precedent for "cosmic-text + wgpu + atlas".
- Public surface:
  - `TextAtlas::new(device, queue, format)` — wgpu texture atlas.
  - `TextRenderer::new(atlas, device, multisample, depth_stencil)` — render pipeline.
  - `TextRenderer::prepare(...)` — uploads dirty glyphs from cosmic-text `Buffer`s.
  - `TextRenderer::render(atlas, render_pass)` — textured-quad draw.
- Used by iced (since the 0.10 era) and several wgpu editors.
- Limitations relevant to mkui:
  - Single atlas page (no paging at last known version).
  - Cache key `(font_id, glyph_id, size, subpixel_variant)` tightly coupled to cosmic-text's `CacheKey`.
  - Render pipeline opinionated about `ColorTargetState` and blending — embedding in mkui-wgpu's scene takes care (§6).

### `parley` — https://github.com/linebender/parley

- Linebender's answer to cosmic-text's `Buffer`, built on `swash` (and increasingly `skrifa`/`fontique`).
- Pre-1.0 as of cutoff; frequent breaking changes. Backs Vello's text story.
- Not for Sprint 2 because of API churn. Re-evaluate at Sprint 5/6 — Linebender's stack is arguably more consistently maintained than System76's.

### `piet` / `vello` — https://github.com/linebender/vello

- Out of scope. Vello is a 2D vector renderer, not a text crate. It *can* render text by tessellating glyph outlines via `skrifa`, but adopting it for text means buying the whole vello compute-shader pipeline. mkui-wgpu's primitives aren't vello-shaped.

---

## 4. How production Rust UI frameworks handle text

The pattern is consistent: **anyone targeting Linux or web ends up at cosmic-text; anyone willing to write three platform backends prefers OS-native on desktop and falls back to cosmic-text on Linux.** mkui-text keeps "fall back to cosmic-text" as the default and the platform-native swap reachable.

### GPUI (Zed) — https://github.com/zed-industries/zed

- Platform-native everywhere they can:
  - macOS: `crates/gpui/src/platform/mac/text_system.rs` wraps CoreText.
  - Windows: `crates/gpui/src/platform/windows/direct_write.rs` wraps DirectWrite.
  - Linux: `crates/gpui/src/platform/linux/cosmic_text.rs` uses cosmic-text as the fallback.
- The abstraction is `PlatformTextSystem` at `crates/gpui/src/text_system.rs` — the precise shape mkui-text borrows in §5.
- Zed blog (2024, "Leveraging Rust and the GPU to render user interfaces at 120 FPS", https://zed.dev/blog/videogame and adjacent posts at https://zed.dev/blog): GPUI uses the OS for font fallback and shaping, then rasterizes and caches glyphs on the GPU.
- **Implication:** GPUI is the gold standard for *the trait*. Cosmic-text everywhere first; platform-native later behind the same interface. Codex should verify §5 is close enough that we could lift GPUI's CoreText implementation as a reference.

### egui — https://github.com/emilk/egui

- `ab_glyph` rasterization + hand-rolled atlas in `egui/src/text/`. **No shaping. No real bidi.** Font fallback is "stack the fonts and pick the first with the glyph", which breaks cluster boundaries.
- Acknowledged in the README and tracked across https://github.com/emilk/egui/issues: no complex script support, no Arabic/Indic, approximate kerning.
- egui's identity is "immediate-mode tool UI, mostly Latin". For mkui's Operator Console and StoneSketch, that bar is too low.

### iced — https://github.com/iced-rs/iced

- **cosmic-text + glyphon — exactly mkui-text's stack.** See `iced_wgpu/src/text.rs`.
- Most direct precedent for mkui: both are general-purpose Rust UI frameworks, both target wgpu, both target wasm. iced has shipped this through dozens of releases. Integration risk is bounded.

### Slint — https://github.com/slint-ui/slint

- cosmic-text since 1.3 (replacing an earlier custom path). See `internal/core/textlayout.rs` and `internal/renderers/`. Retains a Qt backend when compiled against Qt.
- Adds weight to the "cosmic-text is the de-facto Rust text engine" claim.

### Dioxus + Freya — https://github.com/marc2332/freya

- Freya uses **Skia** via `rust-skia` for text and rendering. Skia's text path is HarfBuzz + Skia's own rasterizer + atlas. Not a Rust-native stack.
- Cost is ~20 MB binary + a Skia build chain (clang, python). mkui has explicitly chosen the pure-Rust path; that closes the Skia door.

### Bevy UI — https://github.com/bevyengine/bevy

- `bevy_text` (`crates/bevy_text/`) uses `ab_glyph` + a custom `FontAtlasSet`. No shaping.
- Tracked work to adopt cosmic-text (search https://github.com/bevyengine/bevy/issues for "cosmic-text"); as of cutoff still on `ab_glyph`.
- Game engines tolerate this because most game UI is Latin or pre-rasterized. mkui can't.

### Tally

Three production frameworks (iced, Slint, Zed-on-Linux) ship cosmic-text. One (GPUI mac/win) goes platform-native + cosmic-text fallback. Two (egui, Bevy) use `ab_glyph` and consistently list text as a known limitation. **Among wgpu-targeting Rust UI frameworks, cosmic-text is the default.** mkui-text inheriting that default is the low-risk move.

---

## 5. Sketch of `mkui-text`'s `PlatformTextSystem` trait

This is the section Codex will scrutinize most. The trait is shaped by section 2's six layers and by GPUI's existing `PlatformTextSystem` (the closest production precedent, at `crates/gpui/src/text_system.rs` in `zed-industries/zed`).

```rust
// crates/mkui-text/src/platform.rs
use std::sync::Arc;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct FontId(pub u32);

/// A line/run of shaped glyphs — the unit `mkui-wgpu` consumes.
///
/// Per Codex review (2026-05-21, P1): glyph positions must be absolute (relative
/// to the run's own origin), not just advances + offsets. The renderer should
/// not have to replay shaping math to recover final glyph positions; that
/// defeats the trait's isolation of the text-layout layer.
#[derive(Clone, Debug)]
pub struct LayoutRun {
    pub font_id: FontId,
    pub font_size_px: f32,
    pub glyphs: Vec<LayoutGlyph>,
    /// Origin point in the parent layout's coordinate space. Glyphs' `(x_px, y_px)`
    /// are relative to this origin. The renderer typically translates by
    /// `(origin_x_px, origin_y_px)` before emitting quads.
    pub origin_x_px: f32,
    pub origin_y_px: f32,
    pub line_y_baseline_px: f32,
    pub line_ascent_px: f32,
    pub line_descent_px: f32,
}

/// A single shaped glyph with absolute positioning *within its parent run*.
///
/// The renderer takes the parent `LayoutRun.origin_{x,y}_px` and adds
/// `(x_px, y_px)` to get the final pen position. Advances/offsets are kept
/// for callers that need to do additional layout math (caret hit-testing,
/// selection-rect computation), but the absolute coordinates are the
/// canonical positioning.
#[derive(Copy, Clone, Debug)]
pub struct LayoutGlyph {
    pub glyph_id: u16,
    /// Absolute x position, relative to `LayoutRun.origin_x_px`.
    pub x_px: f32,
    /// Absolute y position, relative to `LayoutRun.origin_y_px`.
    /// Typically equal to the line's baseline + y_offset.
    pub y_px: f32,
    /// Shaping outputs retained for callers that need them (hit-testing,
    /// selection-rect math). The implementation MAY derive `x_px` from
    /// `x_advance_px` + `x_offset_px`, but the trait guarantees `x_px`/`y_px`
    /// are the final positions.
    pub x_advance_px: f32,
    pub x_offset_px: f32,
    pub y_offset_px: f32,
    pub cluster: u32,         // byte offset into source string
    pub subpixel_variant: u8, // 0..=3
}

/// CPU-side rasterized glyph, handed to the atlas in `mkui-wgpu`.
#[derive(Clone, Debug)]
pub struct GlyphImage {
    pub width_px: u32,
    pub height_px: u32,
    pub left_px: i32,
    pub top_px: i32,
    pub format: GlyphFormat,  // Alpha | SubpixelRgb | ColorBitmap
    pub data: Arc<[u8]>,
}

/// Cache key for the GPU atlas — must capture *every* dimension along which
/// two rasterizations could differ. Per Codex review (2026-05-21, P2): a
/// minimal `(font_id, glyph_id, size, subpixel)` key collides on variable-axis
/// values, color-vs-mask format, font source identity (e.g. when a font is
/// re-registered), transform/synthesis flags, and hinting mode. Each field
/// below addresses a known collision class.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct GlyphCacheKey {
    /// Identity of the font face. Two different `FontId`s never collide, even
    /// if they refer to the same on-disk file (re-registration produces a new ID).
    pub font_id: FontId,

    /// Generation counter on the font registry. Bumped when a font is
    /// re-registered, replaced, or invalidated. Distinguishes "same font_id,
    /// different generation" — important for hot-reload paths and for adapters
    /// that may evict and re-register fonts under churn.
    pub font_generation: u32,

    /// OpenType glyph index.
    pub glyph_id: u16,

    /// Font size in px × 64 (fixed-point Q26.6 — avoids f32 hashing, matches
    /// FreeType / HarfBuzz convention).
    pub size_px_q16: u32,

    /// Variable-font axis values, packed. For non-variable fonts, all zero.
    /// The tuple shape is implementation-detail; what matters is that two
    /// rasterizations at different `wght`/`wdth`/`opsz`/etc. coordinates do
    /// not share a cache slot. A `[u16; 4]` packed normalized fixed-point
    /// representation (axis_index → value) covers the common variable-axis
    /// count without making the key huge.
    pub variation_axes: [u16; 4],

    /// Output pixel format. Mask (alpha-only) vs. subpixel-RGB vs. color bitmap
    /// (COLR/CBDT/sbix) MUST not collide — a color-emoji rasterization and an
    /// alpha-mask rasterization of the same glyph_id are different cache entries.
    pub format: GlyphFormat,

    /// Subpixel horizontal positioning bucket, 0..=3 (quarter-pixel).
    pub subpixel_variant: u8,

    /// Synthesis flags — synthetic bold (faux-bold via stroke), synthetic italic
    /// (oblique slant), embolden/condense transforms. Each combination must be
    /// a distinct cache entry. The implementation packs these into a bitfield.
    pub synthesis_flags: u8,

    /// Hinting mode. None / light / normal / full / autohint. Different hints
    /// produce different rasterizations of the same glyph + size.
    pub hinting: HintingMode,

    /// Reserved for future transform support (2D affine rotation/skew). Today
    /// `None` is the only value; the trait reserves the field so the cache key
    /// width is stable when transforms land. See §5 "What Codex should
    /// challenge" item 2.
    pub transform: Option<GlyphTransform>,
}

/// Output pixel format for a rasterized glyph.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum GlyphFormat {
    /// 8-bit alpha mask. The common case for body text. Atlas page format `R8Unorm`.
    Alpha,
    /// Subpixel RGB triplet (LCD-style anti-aliasing). Atlas page format `Rgba8UnormSrgb`,
    /// sampled with RGB-channel-aware shading. Used selectively for high-DPI body text.
    SubpixelRgb,
    /// Pre-rendered color bitmap from COLR/CBDT/sbix tables (color emoji, multi-color
    /// glyphs). Atlas page format `Rgba8UnormSrgb`.
    ColorBitmap,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum HintingMode {
    None,
    Light,
    Normal,
    Full,
    Autohint,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub struct GlyphTransform {
    // 2x2 affine, fixed-point. Reserved for Sprint 6+.
    pub a: i16, pub b: i16, pub c: i16, pub d: i16,
}

/// Owns layers 1-5 (discover, parse, shape, layout, raster).
/// Layer 6 (atlas + GPU) lives in `mkui-wgpu` and consumes the outputs.
pub trait PlatformTextSystem: Send + Sync + 'static {
    fn resolve_font(&self, query: &FontQuery) -> Option<FontId>;
    fn register_font_bytes(&self, bytes: Arc<[u8]>, index: u32) -> Result<FontId, TextError>;
    fn layout(&self, text: &str, spec: &LayoutSpec, width_px: Option<f32>) -> Vec<LayoutRun>;
    fn rasterize(&self, key: GlyphCacheKey) -> Result<GlyphImage, TextError>;
    fn families(&self) -> Vec<String>;
}
```

Implementations:

- `crates/mkui-text/src/cosmic.rs` — `CosmicTextSystem` (Sprint 2). Wraps `cosmic_text::FontSystem` + `SwashCache` behind `parking_lot::Mutex`, translates `FontId` to/from `cosmic_text::fontdb::ID`.
- `crates/mkui-text/src/core_text.rs` — `CoreTextSystem` (Sprint 6+, `cfg(target_os = "macos")`). Empty stub today.
- `crates/mkui-text/src/direct_write.rs` — `DirectWriteSystem` (later, `cfg(target_os = "windows")`).

`mkui-wgpu` only ever holds `Arc<dyn PlatformTextSystem>`. The atlas consumes `LayoutRun`s and `GlyphImage`s; it never sees a cosmic-text type. That is the entire swap path.

### What this trait *deliberately does not do*

- **No `Buffer` type.** Cosmic-text's `Buffer` is its own caching layer; exposing it forces every implementation to invent an equivalent. The trait is stateless per call; implementations memoize internally.
- **No async.** Glyph rasterization is sub-millisecond. Consistent with the workspace's no-async stance (audit-report.md §4).
- **No GPU resource handles.** Returns CPU-side `GlyphImage`s. Layout and rasterization are unit-testable without a wgpu device.
- **No font subsetting / hinting controls** beyond OpenType feature passthrough on `LayoutSpec`.

### What Codex should challenge

1. **Is `LayoutRun` the right granularity?** Cosmic-text emits `LayoutLine`s containing per-font glyph runs; ours is one run per (font, size) within a line. GPUI uses the latter shape. If Codex prefers `LayoutLine`-grained, change before Sprint 2 lands.
2. **Should `rasterize` take a transform (rotation, skew)?** GPUI does. mkui-wgpu has no transformed text and won't in Sprint 2. Trait can grow `rasterize_transformed` later without breaking the alpha path.
3. **`FontId` u32 vs u64?** `u32` matches cosmic-text's `fontdb::ID` width. CoreText returns 64-bit `CTFontRef` — that adapter hashes down to u32.

---

## 6. The atlas question

This is where the second hard decision sits. Cosmic-text gives us shaping + layout + rasterization (layers 3-5). The atlas (layer 6) is where mkui-wgpu earns its keep.

### What glyphon does

`glyphon::TextAtlas` (https://github.com/grovesNL/glyphon/blob/main/src/text_atlas.rs) owns:
- A `wgpu::Texture` with format `Rgba8UnormSrgb` (for color emoji) and a separate alpha mask region.
- An `etagere::BucketedAtlasAllocator` for shelf packing.
- A `LruCache<CacheKey, AtlasEntry>` for eviction when the atlas fills.
- An upload queue that writes new glyphs via `queue.write_texture`.

The cache key is `(font_id, glyph_id, font_size_bits, subpixel_variant)` — effectively the same shape as the `GlyphCacheKey` in section 5.

### Why not just depend on glyphon?

Three reasons:

1. **Pipeline ownership.** glyphon owns the wgpu render pipeline, the bind group layout, the vertex/fragment shaders, the `ColorTargetState`, and the multisample state. mkui-wgpu already has a renderer with opinions about all of those. Embedding glyphon means either (a) running glyphon's pass separately (extra render pass, extra command buffer overhead) or (b) somehow getting glyphon's draws into our pass (currently requires reaching into private fields). iced lives with (a); mkui-wgpu can do better.
2. **Atlas paging.** As of cutoff, glyphon is single-page. mkui's Operator Console design specifies dual-font (SF Pro + JetBrains Mono) × multiple sizes (compact/comfortable/spacious density) × CJK fallback. That is comfortably past one 2048×2048 page and into paging-or-LRU territory.
3. **Coupling to cosmic-text types.** glyphon's public API takes `&cosmic_text::Buffer` directly. If we adopt glyphon we leak cosmic-text through `mkui-wgpu`'s public surface, defeating the trait in section 5.

### What mkui-wgpu should do instead

**glyphon is treated as a behavioral reference only — not a code source, not a dependency.** Per Codex review (2026-05-21, P2), the boundary must be sharp: mkui re-implements the atlas structure from first principles, citing glyphon as the architectural reference (named in this doc) and as the comparable production-tested precedent (named in `docs/sprint-2-plan.md`'s acceptance criteria). The implementation does **not** vendor any glyphon source code, does **not** copy glyphon's private pipeline choices that might not match mkui's renderer, and does **not** take a transitive license dependency on glyphon (which is MIT/Apache-2.0, compatible with mkui — but the deliberate decision is to own the surface and the maintenance).

Lift glyphon's *structure*:

- **Use `etagere`** (https://github.com/nical/etagere) for shelf packing. Same crate glyphon uses; well-tested; no transitive deps of note. This is a direct dependency, not a copy.
- **Cache key:** `GlyphCacheKey` from section 5 — already expanded (per Codex P2) beyond glyphon's minimal key to cover variation axes, color/mask format, font generation, synthesis, hinting, and reserved transform. mkui-wgpu owns this type.
- **Atlas paging:** start with one 2048×2048 page; add `Vec<AtlasPage>` with allocation rolling onto the next page when shelf-fit fails. Eviction policy is LRU within a page, page dropped only when empty for N frames.
- **Color vs alpha:** two atlases (or two regions of one atlas), one `R8Unorm` for alpha glyphs, one `Rgba8UnormSrgb` for color glyphs. The fragment shader picks based on a sampler index in the vertex data.
- **Pipeline:** mkui-wgpu's existing tessellation pipeline emits textured quads. The glyph atlas is one more sampler binding. The render pass is shared. **No second pass.** This is the architectural win over depending on glyphon.

### Render contract for atlas paging

Per Codex review (2026-05-21, P2), the "no second pass" claim depends on the render contract handling multi-page atlases correctly. Specifically:

- **Vertex data carries page identity.** Each emitted glyph quad's vertices include a `page_index: u16` (or `sampler_index` if alpha/color are split into separate atlases per page). The shader uses this to select the correct sampler binding.
- **Batching groups by page.** Within a single draw call, all glyphs must come from the same page (or sampler-bound combination). The renderer's instance buffer is sorted by `(page_index, sampler_kind)` before submission. For the Operator Console's expected glyph load (~2000-4000 active cache entries at steady state), 1-3 pages is the common case and the batching cost is acceptable.
- **Eviction invalidates cached text meshes.** When a glyph is evicted (because its page is being recycled or the LRU policy fires), any cached `LayoutRun` that referenced that glyph's atlas slot is invalidated. The implementation's mesh cache (if any — Sprint 2's `mkui-text` likely re-shapes each frame and doesn't cache meshes) keys against `(LayoutRun, atlas_generation)`. When `atlas_generation` bumps (on eviction), prior meshes are dropped.
- **Page sampler binding limit.** WebGPU's standard limit is 16 sampled-texture-bindings per pipeline. Each atlas page is one binding; the practical ceiling on simultaneous pages is ~12-14 (reserving slots for other uses). If a UI ever blows past that, the renderer either (a) splits into multiple draw calls per text element (acceptable for extreme cases), or (b) evicts a page sooner.

### Eviction

LRU is the standard choice and what glyphon implements. The wrinkle: text atlases are pathologically prone to "almost-full + steady churn" because every new size of every glyph is a new entry. A complex UI rendering ~50 data rows at three densities × two themes × two fonts is at least ~1200 distinct glyph cache entries before any text changes. We need to budget for that:

- A 2048×2048 R8 atlas is 4 MB and holds ~64K small glyphs at 8×8. Comfortable.
- A 2048×2048 RGBA atlas is 16 MB. One page is plenty for a UI's worth of color emoji.
- Pre-warming: on app start, rasterize ASCII for the primary text+mono font families at each density. ~300 glyphs × 3 sizes × 2 fonts = ~1800 entries, < 50ms on a cold cache.

---

## 7. Web / wasm32 compatibility

All four primary crates build for `wasm32-unknown-unknown`:

- **`cosmic-text`** builds for wasm32 with `default-features = false` and the `std` feature on; the only platform-specific code is in `fontdb` and is guarded.
- **`swash`** is pure Rust, no `std` dependence in the hot path, builds for wasm32 cleanly.
- **`rustybuzz`** is pure Rust, builds for wasm32.
- **`fontdb`** on wasm32 returns an empty database unless you `load_font_data` explicitly. Expected: there is no filesystem to scan in the browser. mkui-text ships embedded default fonts (sans + mono) and registers them at boot.

The known traps:

- **`getrandom`** on wasm32 needs the `js` feature. Cosmic-text doesn't pull it directly, but some transitive paths (memmap or rand-via-hashbrown) historically have. Audit `cargo tree -e features -p mkui-text --target wasm32-unknown-unknown` once the crate exists; this is exactly the kind of regression CI should catch (audit-report.md category 9).
- **`mmap`.** `fontdb` has historically used `memmap2` for system-font loading. On wasm32 this code path is dead (no system fonts) but if `memmap2` ever gains a `target_arch = "wasm32"` build break it surfaces here. Pin behind a `default-features = false` declaration and re-enable only on native.
- **`std::time::Instant`.** Some atlas/LRU implementations reach for `Instant`; on wasm32 that panics on `wasm32-unknown-unknown` without `js-sys`. Our atlas uses frame counters, not wall-clock time — sidestep the issue entirely.

**glyphon on web:** glyphon runs in browser via wgpu's WebGPU backend (and previously WebGL via downlevel). iced ships a web demo using exactly this path. The atlas + render pipeline are wgpu-native; the only browser-specific concern is the WebGPU device limits (texture size cap is 8192 on Chrome's WebGPU, 4096 on some configurations). 2048×2048 is well inside both.

**Conclusion:** the stack is wasm32-clean. Codex should verify with one `cargo build --target wasm32-unknown-unknown` run once `mkui-text` lands.

---

## 8. Long-term cost analysis

Four options, four axes. Numbers are **order-of-magnitude estimates**; Codex should verify with `cargo bloat` + `cargo tree` against the real `mkui-text` crate before signing off.

| Option | Bin Δ (release x86_64) | Compile Δ (cold debug) | Deps | Maintenance | 6-mo switch cost |
|---|---|---|---|---|---|
| **A. cosmic-text + own atlas (chosen)** | ~3-4 MB | ~60-90s | ~25-35 | Track cosmic-text releases | **Low** — trait isolates `mkui-text` |
| **B. cosmic-text + glyphon as dep** | ~3.5-4.5 MB | ~70-100s | ~30-40 | Track two upstreams | Medium — glyphon types leak into `mkui-wgpu` |
| **C. Custom on swash only** | ~1.5-2 MB | ~30-50s | ~10-15 | **High** — we own shaping + discovery | High — rewriting shaping |
| **D. Trait-only, no impl** | 0 | 0 | 0 | Pushed to consumers | N/A — decision deferred |

A over B: §6 (pipeline ownership, paging, type isolation). A over C: §4 — egui and Bevy demonstrate the no-shaping path can't ship internationally. A over D: postpones without learning; Operator Console needs working text in Sprint 4+, not a stub.

### What you give up if you change your mind in 6 months

- **A → CoreText:** new file in `mkui-text`, ~1500-2500 LOC of `objc2` calls. No `mkui-wgpu` change. Trait is the firewall.
- **A → parley:** new file, ~1000-1500 LOC adapter. Hardest part is matching layout semantics so existing UI doesn't shift pixels.
- **A → vello/skrifa:** Vello owns the renderer; this is the "rewrite mkui-wgpu" path. Avoid.
- **A → Skia:** C++ toolchain (clang, python, gn) — the reason mkui is Rust is to not have that.

---

## 9. Why NOT cosmic-text? — the strongest counterargument

The honest case against, charitably:

**1. Dependency weight (~25-35 transitive crates).** A real cost after audit-report.md §6 trimmed unused deps. **Counter:** every alternative pulls similar (parley + skrifa + fontique ~25-30; Skia is a different class). For correct Arabic/Devanagari/emoji, the cost is acceptable.

**2. Not pixel-identical to macOS.** cosmic-text + swash renders different stem positions, hinting, and subpixel layout than CoreText. For an Operator Console living next to Xcode that is a perception issue. **Counter:** exactly why the §5 trait exists. Sprint 2 ships cosmic-text; Sprint 6+ swaps to CoreText. Divergence is time-bounded.

**3. Opinionated layout decisions.** cosmic-text's `Buffer` picks wrap points, tab handling, and line breaking we may want to override. **Counter:** the trait is the override point. Wrap overrides go on `LayoutSpec`, not a cosmic-text fork.

**4. Integration friction with wgpu.** Cosmic-text is renderer-agnostic; we make the atlas decision ourselves (§6). **Counter:** this is the *good* shape — the alternative is glyphon's "here is your pipeline, take it or leave it."

**5. rustybuzz lag behind HarfBuzz.** Browsers (HarfBuzz) will diverge from mkui-text on OT-rich fonts over time. **Counter:** invisible for mainstream UI fonts (SF Pro, Inter, JetBrains Mono). Would matter for a typography tool. mkui is not.

**6. cosmic-text release cadence.** System76 ships on COSMIC desktop's schedule; bug-fix turnaround could be months. **Counter:** we can vendor a forked copy if a blocker hits — same upstream pattern as PR #12.

None are decisive. Together they argue for the §5 trait shape (cosmic-text is replaceable), not for picking something else today.

---

## 10. Honest unresolved questions

Codex: please rule on these before Sprint 2 starts.

1. **Should `mkui-text` be a separate crate, or fold into `mkui-wgpu`?** Arguments for separate: (a) cleanly testable without a wgpu device, (b) future consumers of just the text layer (e.g. `mkui-console` for terminal width measurement) can depend on it without pulling wgpu, (c) matches the `mkui-core` precedent of "contract crate has no backend dep". Arguments for folding in: (a) one less crate to maintain, (b) the only consumer today is `mkui-wgpu`. **My lean: separate.** It mirrors the contract/backend split that audit-report.md praised. But Codex's call if it disagrees.

2. **One atlas per face, or one atlas across the workspace?** glyphon does one-atlas-per-`TextAtlas`-instance. iced creates one per renderer. For mkui, the Operator Console + StoneSketch could either share an atlas (mkui-wgpu singleton) or each pay their own. **My lean: one global atlas per `WgpuRenderer` instance.** Two reasons: (a) most apps have one renderer, (b) sharing means cross-app glyph reuse, which matters for the chrome-shared-across-panels case.

3. **Font fallback ordering, especially when CoreText lands later.** Today cosmic-text picks fallbacks from `fontdb`. When CoreText lands on macOS, do we use CoreText's fallback chain (the OS's choice) or mkui's explicit list? **My lean: explicit list, OS as last resort.** Reproducibility across platforms matters for the Operator Console's "same chrome on mac + linux" goal.

4. **Subpixel positioning quanta.** Glyphon uses 4 quanta (variant 0..=3). Some renderers use 8. More quanta = better-looking text + larger atlas. **My lean: 4 quanta to start (cosmic-text default).** Revisit if Operator Console screenshots show visible jitter.

5. **Should `mkui-text` ship a default UI font?** wasm32 has no system fonts, so without an embedded default we render blank. Options: ship Inter (free, ~300 KB compressed), ship a tiny default (Roboto/IBM Plex), or require the consumer to register. **My lean: ship Inter as default.** 300 KB in a wasm bundle is acceptable for a UI framework; the alternative is "mkui works on native and silently breaks on web".

---

## Decision-readiness checklist

What Codex needs to read before reviewing this document:

- [ ] `docs/audit-report.md` — for tone, score conventions, and the workspace's current health
- [ ] `docs/downstream-consumers.md` — to confirm StoneSketch and Operator Console actually need this text quality
- [ ] `docs/sprint-2-plan.md` — to confirm `mkui-text` fits the sprint's effort budget
- [ ] cosmic-text README at https://github.com/pop-os/cosmic-text — to confirm section 3's claims about what cosmic-text bundles
- [ ] glyphon README + `src/text_atlas.rs` at https://github.com/grovesNL/glyphon — to confirm section 6's claims about its architecture
- [ ] GPUI's `crates/gpui/src/text_system.rs` + the three `platform/*/text_system.rs` files in `zed-industries/zed` — to confirm the trait shape in section 5 is GPUI-equivalent

What evidence supports the cosmic-text call:

- **Three production frameworks ship it** (iced, Slint, Zed-on-Linux). Section 4.
- **No pure-Rust alternative covers shaping + emoji + bidi.** Section 3 — fontdue, ab_glyph, ttf-parser all fail this bar.
- **The trait shape (section 5) is the same one GPUI uses,** with a documented swap path for CoreText / DirectWrite later.
- **wasm32 builds cleanly** (section 7); no other Rust text stack with full shaping has shipped to wasm32 at scale outside iced + Slint.
- **The atlas can be lifted from glyphon's design** without taking glyphon's pipeline (section 6).

What would change the call:

- **cosmic-text becoming unmaintained.** If `pop-os/cosmic-text` has its last commit > 12 months before Sprint 2 starts, parley (Linebender) becomes the default instead. Re-evaluate.
- **parley reaching 1.0 with a clear atlas story.** parley's API stabilizing would make it a stronger candidate. As of cutoff that has not happened.
- **WebGPU adding a native text primitive.** It hasn't and won't soon, but if a future browser ships `GPUTextRenderPipeline`-equivalent we revisit everything.
- **A consumer needing Skia-pixel-identical rendering.** Today no consumer does. If StoneSketch starts shipping documents-into-Word-compat features, the Skia question reopens.
- **Discovering a blocking wasm32 issue in cosmic-text/glyphon's transitive deps** — `getrandom`, `memmap2`, or `Instant` regressions. Section 7 flags these as the things to check; if any fail on `cargo build --target wasm32-unknown-unknown`, the decision becomes "vendor cosmic-text and patch" or "fall back to parley".

---

**Last updated:** 2026-05-20. Author: Sprint-2 planning agent. Reviewers: Codex (adversarial) + future-me (continuity).
