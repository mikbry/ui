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

use std::sync::Arc;

/// Opaque font handle returned by [`TextSystem::resolve_font`] and
/// [`TextSystem::register_font_bytes`].
///
/// `FontId(0)` is reserved for the bitmap face shipped by
/// [`crate::BitmapTextSystem`]; future implementations are free to allocate
/// the rest of the space however they like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontId(pub u64);

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

/// Reserved per-glyph 2×2 affine transform. The bitmap path never sets a
/// non-identity transform — the field exists so future shears / italic
/// synthesis / variable-axis skews flow through the cache key.
///
/// Storage is Q16.16 fixed-point so the value participates in `Hash`/`Eq`.
/// `matrix == [0; 4]` is the sentinel for "identity".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GlyphTransform {
    pub matrix: [i32; 4],
}

impl GlyphTransform {
    pub const IDENTITY: Self = Self { matrix: [0; 4] };

    pub const fn is_identity(self) -> bool {
        self.matrix[0] == 0 && self.matrix[1] == 0 && self.matrix[2] == 0 && self.matrix[3] == 0
    }
}

/// Query passed to [`TextSystem::resolve_font`]. The Sprint 2 bitmap path
/// ignores every field and always returns `FontId(0)`; the structure exists
/// so consumers can already write production-shaped lookups.
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
    pub variation_axes: u64,
    pub synthesis_flags: u32,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            font_id: FontId(0),
            font_generation: 0,
            font_size_px: 10.0,
            line_height_px: 14.0,
            align: TextAlign::Start,
            max_lines: None,
            hinting: HintingMode::None,
            variation_axes: 0,
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
#[derive(Debug, Clone)]
pub struct LayoutRun {
    pub font_id: FontId,
    pub font_generation: u32,
    pub font_size_px: f32,
    pub variation_axes: u64,
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
            variation_axes: self.variation_axes,
            format: glyph.format,
            subpixel_variant: glyph.subpixel_variant,
            synthesis_flags: self.synthesis_flags,
            hinting: self.hinting,
            transform: GlyphTransform::IDENTITY,
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
/// negative in practice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphCacheKey {
    pub font_id: FontId,
    pub font_generation: u32,
    pub glyph_id: u32,
    pub size_px_q26_6: i32,
    pub variation_axes: u64,
    pub format: GlyphFormat,
    pub subpixel_variant: u8,
    pub synthesis_flags: u32,
    pub hinting: HintingMode,
    pub transform: GlyphTransform,
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
}

/// Domain-neutral text-system interface. Renderers consume the trait so the
/// implementation can be swapped (bitmap → SFNT → Slug) without source
/// changes on the consumer side.
///
/// Implementors must be `Send + Sync + 'static` so the renderer can hold an
/// `Arc<dyn TextSystem>` across threads.
pub trait TextSystem: Send + Sync + 'static {
    /// Resolve a font query to a registered face. Returns `None` if no face
    /// matches. The bitmap path always returns `Some(FontId(0))` because it
    /// ships exactly one face.
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

    /// List the human-readable family names this implementation exposes.
    fn families(&self) -> Vec<String>;
}
