//! `TextSystem` trait + the supporting types every implementation agrees on.
//!
//! The trait isolates layers 1-5 of the text pipeline (discover, parse, shape,
//! layout, raster). Layer 6 (atlas + GPU upload) lives in the renderer crate
//! and consumes [`LayoutRun`] / [`GlyphImage`] outputs from this trait.
//!
//! Per the workspace's "own the stack" commitment, no external text-stack
//! crates appear here — the types are deliberately small and the trait is
//! shaped to admit future SFNT-parser / outline-rasterizer / Slug-encoder
//! implementations without changing consumer code.
//!
//! ## Identity domain
//!
//! [`FontId`] is the single public face identity; it is opaque and minted only
//! by a [`FontIdAllocator`](crate::FontIdAllocator) (or the reserved
//! [`FontId::BITMAP`]). Outline-affecting state — variation coordinates and the
//! outline-local affine transform — is carried in canonical fixed-point form
//! ([`VariationSettings`], [`Affine2Fixed`]) so it can key a cache directly.
//!
//! ## Render-lane propagation
//!
//! Each [`LayoutRun`] carries a [`TextRenderClass`] alongside its `font_id`, so
//! a single render-lane signal flows from font resolution through layout into
//! the renderer's command builder. Bitmap is the compatibility default.

use std::sync::Arc;

use crate::canonical::{Affine2Fixed, OpenTypeTag, VariationSettings};
use crate::outline::{GlyphOutline, OutlineKey};

pub use crate::font_id::{FontId, FontIdAllocator};

/// Backend-neutral render-lane signal carried by every [`LayoutRun`].
///
/// A renderer reads this to route a run to the appropriate command builder.
/// `Bitmap` is the compatibility default — backends that do not yet implement
/// a non-bitmap lane (web, console in Sprint 7) may ignore the other variants
/// without regressing, because the bitmap path always emits `Bitmap`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TextRenderClass {
    /// Raster bitmap face — the permanent debug fallback / default lane.
    #[default]
    Bitmap,
    /// Slug-style GPU vector encoding.
    Slug,
    /// CPU/GPU outline fill.
    Outline,
    /// Multi-channel signed-distance-field.
    Msdf,
}

/// Horizontal alignment of a wrapped text block. The `Start`/`End` variants
/// follow logical-direction conventions (LTR locales: left/right; RTL would
/// flip them) — the bitmap implementation only sees LTR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum TextAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Pixel format of a [`GlyphImage`] returned by [`TextSystem::rasterize`].
///
/// The Sprint 2 bitmap path only emits `Alpha`; the other variants are
/// reserved for subpixel-RGB and color-bitmap paths in later sprints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum GlyphFormat {
    #[default]
    Alpha,
    SubpixelRgb,
    ColorBitmap,
}

/// Hinting level passed through to the rasterizer.
///
/// The Sprint 2 bitmap path ignores hinting (each glyph is a fixed 5×7 bit
/// pattern); the enum is part of the API so consumers can already key caches
/// on the requested hint mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum HintingMode {
    #[default]
    None,
    Light,
    Normal,
    Full,
    Autohint,
}

/// Query passed to [`TextSystem::resolve_font`]. The Sprint 2 bitmap path
/// ignores every field and always returns [`FontId::BITMAP`]; the structure
/// exists so consumers can already write production-shaped lookups.
#[derive(Debug, Clone, Default)]
pub struct FontQuery {
    pub family: Option<String>,
    pub weight: u16,
    pub italic: bool,
    pub stretch: u16,
}

/// Per-call layout configuration. Combines font selection (id + generation),
/// type metrics (size + line height), alignment, and the same forward-compat
/// fields surfaced on [`GlyphCacheKey`].
#[derive(Debug, Clone)]
pub struct LayoutSpec {
    pub font_id: FontId,
    pub font_generation: u32,
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub align: TextAlign,
    pub max_lines: Option<usize>,
    pub hinting: HintingMode,
    /// Canonical variation coordinate, keyed into the glyph cache.
    pub variations: VariationSettings,
    pub synthesis_flags: u32,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            font_id: FontId::BITMAP,
            font_generation: 0,
            font_size_px: 10.0,
            line_height_px: 14.0,
            align: TextAlign::Start,
            max_lines: None,
            hinting: HintingMode::None,
            variations: VariationSettings::empty(),
            synthesis_flags: 0,
        }
    }
}

/// Per-run constants + line-level metrics. One `LayoutRun` per wrapped line
/// in the bitmap implementation; future shapers may emit one per script /
/// font-fallback segment within a line.
///
/// Run-level positions (`origin_x_px`, `origin_y_px`, `line_y_baseline_px`)
/// are expressed relative to the text box origin the caller passed to
/// [`TextSystem::layout`] — the renderer translates them into surface space.
///
/// `render_class` is the per-run render-lane signal; the bitmap path always
/// emits [`TextRenderClass::Bitmap`].
#[derive(Debug, Clone)]
pub struct LayoutRun {
    pub font_id: FontId,
    pub render_class: TextRenderClass,
    pub font_generation: u32,
    pub font_size_px: f32,
    pub variations: VariationSettings,
    pub synthesis_flags: u32,
    pub hinting: HintingMode,
    pub origin_x_px: f32,
    pub origin_y_px: f32,
    pub line_y_baseline_px: f32,
    pub line_ascent_px: f32,
    pub line_descent_px: f32,
    pub glyphs: Vec<LayoutGlyph>,
}

impl LayoutRun {
    /// Compose a [`GlyphCacheKey`] from this run's per-run constants and a
    /// glyph's per-glyph deltas. Centralising the composition here means
    /// renderers never replay shaping math when keying their atlas cache.
    pub fn cache_key(&self, glyph: &LayoutGlyph) -> GlyphCacheKey {
        GlyphCacheKey {
            font_id: self.font_id,
            font_generation: self.font_generation,
            glyph_id: glyph.glyph_id,
            size_px_q26_6: (self.font_size_px * 64.0).round() as i32,
            variations: self.variations.clone(),
            format: glyph.format,
            subpixel_variant: glyph.subpixel_variant,
            synthesis_flags: self.synthesis_flags,
            hinting: self.hinting,
            transform: Affine2Fixed::IDENTITY,
        }
    }
}

/// Per-glyph layout output. `x_px` / `y_px` are **absolute positions** within
/// the parent [`LayoutRun`] (top-left of the glyph cell, relative to the run
/// origin) — consumers never replay shaping math to compose them. The
/// `*_advance` / `*_offset` fields are retained for caret hit-testing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutGlyph {
    pub glyph_id: u32,
    pub x_px: f32,
    pub y_px: f32,
    pub x_advance_px: f32,
    pub x_offset_px: f32,
    pub y_offset_px: f32,
    pub cluster: u32,
    pub subpixel_variant: u8,
    pub format: GlyphFormat,
}

/// 10-field cache key capturing every dimension along which two
/// rasterizations of the "same glyph" could legitimately differ.
///
/// The field count is non-negotiable per the design: if a new rasterizer
/// introduces a new dimension, it needs to live on this struct so the atlas
/// can never silently alias two distinct outputs to the same key.
///
/// `size_px_q26_6` is `font_size_px × 64` rounded to `i32`; the Q26.6 name
/// matches the standard font-metric convention even though we never go
/// negative in practice. `variations` and `transform` are canonical
/// fixed-point values, never pre-hashed summaries.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GlyphCacheKey {
    pub font_id: FontId,
    pub font_generation: u32,
    pub glyph_id: u32,
    pub size_px_q26_6: i32,
    pub variations: VariationSettings,
    pub format: GlyphFormat,
    pub subpixel_variant: u8,
    pub synthesis_flags: u32,
    pub hinting: HintingMode,
    pub transform: Affine2Fixed,
}

/// CPU-side rasterized glyph. `left_px` / `top_px` are the bearing offsets
/// from the glyph's pen origin to the top-left of the bitmap; the bitmap
/// path always reports `(0, 0)` because the 5×7 cells start at the cell
/// top-left.
///
/// `data` is shared via `Arc` so caches can hand the same allocation to
/// many call sites without copying.
#[derive(Debug, Clone)]
pub struct GlyphImage {
    pub width_px: u32,
    pub height_px: u32,
    pub left_px: i32,
    pub top_px: i32,
    pub format: GlyphFormat,
    pub data: Arc<[u8]>,
}

/// Errors surfaced by [`TextSystem`] methods.
///
/// The variants stay narrow on purpose; richer diagnostics belong in
/// implementation-specific extensions rather than the shared contract.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextError {
    /// Returned by implementations that do not parse external font bytes
    /// (the bitmap path, primarily).
    #[error("invalid font bytes")]
    InvalidFontBytes,
    /// The `font_id` carried by a cache key or layout spec is not known to
    /// this implementation.
    #[error("unknown font: {0:?}")]
    UnknownFont(FontId),
    /// The `glyph_id` carried by a cache key cannot be rasterized by this
    /// implementation.
    #[error("unknown glyph: {0}")]
    UnknownGlyph(u32),
    /// The `1..=u64::MAX` [`FontId`] space is exhausted; allocation never
    /// wraps onto live or reserved ids.
    #[error("font id space exhausted")]
    FontIdOverflow,
    /// This implementation does not support glyph-outline extraction (the
    /// compatibility default; e.g. the raster-only bitmap path).
    #[error("glyph outline not supported by this text system")]
    UnsupportedOutline,
    /// A [`VariationSettings`] construction saw the same axis tag twice.
    #[error("duplicate variation axis: {0:?}")]
    DuplicateVariationAxis(OpenTypeTag),
    /// A fixed-point conversion received a non-finite or out-of-range value.
    #[error("invalid fixed-point value")]
    InvalidFixedPoint,
}

/// Domain-neutral text-system interface. Renderers consume the trait so the
/// implementation can be swapped (bitmap → SFNT → Slug) without source
/// changes on the consumer side.
///
/// Implementors must be `Send + Sync + 'static` so the renderer can hold an
/// `Arc<dyn TextSystem>` across threads.
pub trait TextSystem: Send + Sync + 'static {
    /// Resolve a font query to a registered face. Returns `None` if no face
    /// matches. The bitmap path always returns `Some(FontId::BITMAP)` because
    /// it ships exactly one face.
    fn resolve_font(&self, query: &FontQuery) -> Option<FontId>;

    /// Register a font from raw bytes. The bitmap path always returns
    /// [`TextError::InvalidFontBytes`] because it does not parse external
    /// fonts.
    fn register_font_bytes(&self, bytes: Arc<[u8]>, index: u32) -> Result<FontId, TextError>;

    /// Lay out `text` under `spec`, wrapping at `max_width_px` if set.
    /// Returns one [`LayoutRun`] per produced line.
    fn layout(&self, text: &str, spec: &LayoutSpec, max_width_px: Option<f32>) -> Vec<LayoutRun>;

    /// Rasterize the glyph identified by `key` to a CPU-side [`GlyphImage`].
    fn rasterize(&self, key: GlyphCacheKey) -> Result<GlyphImage, TextError>;

    /// Resolve the vector outline for `key`. The compatibility default returns
    /// [`TextError::UnsupportedOutline`] so existing/custom implementors do not
    /// break merely by omitting outline support; raster-only systems (the
    /// bitmap path) keep this default. #62 routes this generically and #67
    /// implements it for SFNT.
    fn glyph_outline(&self, key: &OutlineKey) -> Result<GlyphOutline, TextError> {
        let _ = key;
        Err(TextError::UnsupportedOutline)
    }

    /// List the human-readable family names this implementation exposes.
    fn families(&self) -> Vec<String>;
}
