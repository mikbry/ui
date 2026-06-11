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
//! - [`TextEngine`] / [`PreparedText`] / [`TextLayout`] — cached preparation
//!   separated from width-dependent layout, so a string can be laid out at
//!   many widths without re-preparing it.
//! - [`BitmapTextSystem`] — Sprint 2 implementation: 5×7 ASCII bit-pattern
//!   port of an earlier reference prototype, behind the trait so it can
//!   be swapped out without touching renderer code.
//!
//! The bitmap implementation is permanent debug-fallback and visual
//! regression oracle — when Sprint 3+ Slug-style rendering lands it stays
//! shipping, it just stops being the default.
//!
//! ## Identity & render-lane domain
//!
//! [`FontId`] is the single, opaque, globally-unique public face identity,
//! minted only by a [`FontIdAllocator`] (raw `1..=u64::MAX`) or the reserved
//! [`FontId::BITMAP`] (raw `0`). Outline-affecting state is carried in
//! canonical fixed-point form ([`VariationSettings`], [`Affine2Fixed`]) so it
//! keys a cache directly — never as a pre-hashed summary. Each [`LayoutRun`]
//! carries a [`TextRenderClass`] render-lane signal (bitmap is the
//! compatibility default), and the renderer-neutral [`OutlineKey`] /
//! [`GlyphOutline`] contract lets a face expose vector outlines (default:
//! [`TextError::UnsupportedOutline`]) without dragging renderer types into
//! this crate.

#![forbid(unsafe_code)]

pub mod bitmap;
pub mod canonical;
pub mod engine;
pub mod font_id;
pub mod outline;
pub mod system;

pub use bitmap::{
    bitmap_glyph, bitmap_scale, cache_key_for, max_glyphs_for_width, measure_line_width,
    normalize_bitmap_char, wrap_text_lines, BitmapTextSystem, BITMAP_FAMILY, GLYPH_ADVANCE_PX,
    GLYPH_CELL_HEIGHT_PX, GLYPH_CELL_WIDTH_PX, REFERENCE_FONT_SIZE_PX,
};
pub use canonical::{Affine2Fixed, Fixed16_16, OpenTypeTag, VariationAxis, VariationSettings};
pub use engine::{LineMetrics, PreparedText, TextEngine, TextLayout};
pub use font_id::{FontId, FontIdAllocator};
pub use outline::{GlyphOutline, OutlineBounds, OutlineCommand, OutlineKey};
pub use system::{
    FontQuery, GlyphCacheKey, GlyphFormat, GlyphImage, HintingMode, LayoutGlyph, LayoutRun,
    LayoutSpec, TextAlign, TextError, TextRenderClass, TextSystem,
};
