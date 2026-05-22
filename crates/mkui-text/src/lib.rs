//! # mkui-text — domain-neutral text system trait + from-scratch bitmap prototype.
//!
//! `mkui-text` owns the contract every mkui backend uses to lay out and
//! rasterize text. Per the workspace's "own the stack" commitment, the crate
//! never pulls in an external text-stack dependency — no `cosmic-text`,
//! `swash`, `HarfRust`, `rustybuzz`, `skrifa`, `fontdb`, `glyphon`, `parley`,
//! `fontdue`, `ab_glyph`, `ttf-parser`, `harfbuzz_rs`, `core-text`,
//! `core-graphics`, `core-foundation`, `windows` (DWrite), `pango`, or
//! `freetype`. Each subsequent sprint adds a fully in-house implementation
//! behind the same trait.
//!
//! ## Layers
//!
//! - [`TextSystem`] / [`LayoutRun`] / [`LayoutGlyph`] / [`GlyphCacheKey`] /
//!   [`GlyphImage`] — the call shape every implementation honours.
//! - [`BitmapTextSystem`] — Sprint 2 implementation: 5×7 ASCII bit-pattern
//!   port of StoneSketch's existing prototype, behind the trait so it can
//!   be swapped out without touching renderer code.
//!
//! The bitmap implementation is permanent debug-fallback and visual
//! regression oracle — when Sprint 3+ Slug-style rendering lands it stays
//! shipping, it just stops being the default.

#![forbid(unsafe_code)]

pub mod bitmap;
pub mod system;

pub use bitmap::{
    bitmap_glyph, bitmap_scale, cache_key_for, max_glyphs_for_width, measure_line_width,
    normalize_bitmap_char, wrap_text_lines, BitmapTextSystem, BITMAP_FAMILY, GLYPH_ADVANCE_PX,
    GLYPH_CELL_HEIGHT_PX, GLYPH_CELL_WIDTH_PX, REFERENCE_FONT_SIZE_PX,
};
pub use system::{
    FontId, FontQuery, GlyphCacheKey, GlyphFormat, GlyphImage, GlyphTransform, HintingMode,
    LayoutGlyph, LayoutRun, LayoutSpec, TextAlign, TextError, TextSystem,
};
