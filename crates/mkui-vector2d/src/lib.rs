#![forbid(unsafe_code)]
//! # mkui-vector2d — backend-neutral 2D paths + Slug glyph encoder
//!
//! `mkui-vector2d` owns the **CPU/path** half of mkui's vector text lane: a
//! renderer-independent path/curve model and a deterministic Slug-style glyph
//! curve/band encoder. Per the Sprint 7 ownership ADR (#64) it sits one layer
//! above `mkui-text` and one layer below any GPU adapter:
//!
//! ```text
//! mkui-vector2d  ->  mkui-text          (this crate; no GPU)
//! mkui-vector2d-wgpu  ->  mkui-vector2d + wgpu   (#66; GPU packing)
//! ```
//!
//! ## What this crate owns
//!
//! - [`path`] — a backend-neutral [`path::VectorPath`] of move/line/quadratic/
//!   cubic/close commands plus fill, transform, and bounds. General icons,
//!   strokes, and analytic primitives are *representable*; only the glyph lane
//!   is implemented end-to-end in Sprint 7.
//! - [`outline`] — the resolved [`outline::GlyphOutline`] input contract and
//!   its faithful 1:1 conversion into a [`path::VectorPath`]. Conversion never
//!   reapplies variation, synthesis, affine transform, or bounds — the outline
//!   arrives fully resolved from `mkui-text`.
//! - [`slug`] — the deterministic [`slug::encode_slug_glyph`] encoder and the
//!   [`slug::SlugBlobCache`], keyed by the collision-free
//!   [`slug::SlugGlyphKey`].
//! - [`fixed`] — canonical fixed-point identity values
//!   ([`fixed::VariationSettings`], [`fixed::Affine2Fixed`]) used by the key.
//!
//! ## What this crate must NOT own
//!
//! No WGPU type, shader, buffer, bind group, device, queue, or surface appears
//! anywhere in this crate, and its dependency tree contains no WGPU crate. GPU
//! packing, screen-space y-down conversion, and ordered render lanes are #66's
//! responsibility.
//!
//! ## Two distinct caches
//!
//! This crate's [`slug::SlugBlobCache`] is the **size-independent outline
//! cache**: the encoded curve/band blob depends only on font identity, glyph
//! id, variations, synthesis, and the outline-local affine — never on pixel
//! size, hinting, or subpixel position. It is deliberately separate from
//! `mkui-text`'s **size-dependent bitmap raster cache** (`GlyphCacheKey` /
//! `GlyphImage`), which keys on pixel size and subpixel variant because a
//! rasterized bitmap *does* change with size. One outline blob serves every
//! pixel size; one raster bitmap serves exactly one.
//!
//! ## Contract for downstream consumers (#66)
//!
//! The records produced here — glyph bounds, quadratic curve records
//! `(p0, p1, p2)`, horizontal/vertical band ranges, and curve-index streams,
//! all in font units y-up — are the versioned contract. #66 may serialize and
//! GPU-pack them but may not reimplement or reinterpret the band algorithm.
//!
//! ## Integration note
//!
//! The [`outline::GlyphOutline`] / [`outline::OutlineKey`] input types and the
//! [`fixed`] identity values mirror the contract owned by `mkui-text` (#61).
//! They are reproduced here so the crate builds and tests ahead of #61 landing;
//! a follow-up integration replaces them with re-exports from `mkui-text`.

pub mod fixed;
pub mod outline;
pub mod path;
pub mod slug;

pub use fixed::{
    Affine2Fixed, Fixed16_16, FixedError, OpenTypeTag, VariationAxis, VariationError,
    VariationSettings,
};
pub use outline::{GlyphOutline, OutlineBounds, OutlineCommand, OutlineKey};
pub use path::{Affine2, Bounds, FillRule, PathCommand, Vec2, VectorPath};
pub use slug::{
    encode_slug_glyph, BandRange, GlyphBounds, SlugBlobCache, SlugConfig, SlugCurve,
    SlugEncodeError, SlugGlyph, SlugGlyphKey,
};

// Re-export the global font identity this crate keys on, so consumers need not
// also depend on `mkui-text` just to name a `FontId`.
pub use mkui_text::FontId;
