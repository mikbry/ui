//! SFNT-loaded glyph outline → [`SlugGlyph`] extraction bridge (#169, Part
//! B.1 of #165's Sprint 8 tail).
//!
//! Bridges `mkui-text`'s narrow SFNT/TrueType decoder ([`SfntFace`]) to this
//! crate's Slug encoder, so the wgpu Slug lane can render glyphs loaded from
//! any SFNT font — not just the hand-authored fixtures #67's vertical slice
//! (`tests/slug_slice.rs`) exercised. That slice hand-assembles the same
//! outline → path → blob pipeline per call site with `.expect()` throughout;
//! [`extract_slug_glyph`] is the single fallible, reusable entry point B.2's
//! component call sites drive instead.
//!
//! ## Why this lives in `mkui-vector2d`, not `mkui-text`
//!
//! #64's layering is one-directional: `mkui-vector2d` depends on `mkui-text`,
//! never the reverse (see the crate-root docs). `SlugGlyph`, `SlugBlobCache`,
//! and `encode_slug_glyph` all live in this crate's [`crate::slug`] module —
//! `mkui-text` depending back on them to host this bridge would be a
//! dependency cycle, which Cargo rejects outright. The extraction bridge
//! produces exactly those types, so it lives one layer up, beside them,
//! consuming `mkui-text`'s [`SfntFace`] the same way [`crate::outline`]
//! already consumes its [`mkui_text::GlyphOutline`].
//!
//! ## Fallible by typed reason
//!
//! [`TextExtractionError`] gives a caller (a B.2 component call site) a
//! reason to degrade gracefully to the bitmap lane instead of panicking:
//! composite glyphs, non-quadratic curve data, and missing/empty outline data
//! are distinct, matchable variants — never a generic catch-all.
//!
//! ## Cache-aware
//!
//! Extraction slots into
//! [`SlugBlobCache::get_or_try_encode_with_units_per_em`] using `face`'s own
//! [`SfntFace::units_per_em`], so the band-overlap epsilon (#157 Phase 2 step
//! 5) normalizes to the font's real design-unit scale instead of the cache's
//! default. The cache is consulted for `key` **before** the SFNT decoder ever
//! runs: on a hit, neither the decoder nor the encoder does any work — a
//! naive "decode, then hand the path to the cache" ordering would still pay
//! the full outline-decode cost on every hit, defeating the point of caching
//! an expensive glyph decode. `units_per_em` is not folded into
//! [`SlugGlyphKey`] itself: it is a property of the font a given `font_id`
//! names (one font, one units-per-em), so `key.font_id` already keeps two
//! fonts' blobs from aliasing even when they share a `glyph_id` — see
//! [`SlugBlobCache::encode_with_units_per_em`]'s own docs for the identical
//! argument.

use std::sync::Arc;

use mkui_text::{GlyphOutline, OutlineCommand, SfntError, SfntFace};

use crate::path::{Bounds, FillRule, PathCommand, Vec2, VectorPath};
use crate::slug::{SlugBlobCache, SlugEncodeError, SlugGlyph, SlugGlyphKey};

/// Typed failure modes of the SFNT → [`SlugGlyph`] extraction bridge.
///
/// Every variant is a distinct, matchable reason extraction declined, never a
/// generic catch-all — the acceptance-criterion shape #169 requires so a
/// caller can degrade gracefully to the bitmap lane per reason.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TextExtractionError {
    /// The glyph is a composite (component) glyph — the narrow SFNT decoder
    /// declines it explicitly ([`SfntError::CompositeGlyph`]) rather than
    /// decompose it. Component decomposition (e.g. an accented character
    /// built from a base glyph + accent glyph) is out of scope for B.1; see
    /// #169's "Not in scope".
    #[error("composite glyphs are not supported by the SFNT-to-Slug extraction bridge")]
    CompositeGlyph,
    /// A non-quadratic curve segment was encountered while building the
    /// glyph's [`VectorPath`], or the Slug encoder rejected one as such
    /// ([`SlugEncodeError::UnsupportedSegment`]). TrueType `glyf` outlines
    /// are natively quadratic, so the narrow SFNT decoder can never actually
    /// produce one today — this is a defensive branch against a future
    /// [`OutlineCommand`] variant (the enum is `#[non_exhaustive]`) or a
    /// future non-TrueType outline source, not a case reachable through
    /// [`SfntFace`] as it stands.
    #[error(
        "non-quadratic curve segments are not supported by the SFNT-to-Slug extraction bridge"
    )]
    NonQuadraticCurve,
    /// The glyph outline is missing, out of range, malformed, or resolves to
    /// no drawable curves (e.g. the space glyph, or a glyph id past the
    /// face's glyph count). Carries a short static label for the reason.
    #[error("glyph outline data is missing or unavailable: {0}")]
    MissingOutlineData(&'static str),
}

impl From<SlugEncodeError> for TextExtractionError {
    fn from(err: SlugEncodeError) -> Self {
        match err {
            SlugEncodeError::EmptyOutline => {
                TextExtractionError::MissingOutlineData("outline is empty: no drawable curves")
            }
            SlugEncodeError::UnsupportedSegment => TextExtractionError::NonQuadraticCurve,
        }
    }
}

/// Extract `key.glyph_id` from `face` into a cached [`SlugGlyph`] blob.
///
/// Checks the cache for `key` **first**, via
/// [`SlugBlobCache::get_or_try_encode_with_units_per_em`] — only on an actual
/// miss does it resolve the glyph's outline via [`SfntFace::glyph_outline`]
/// and convert it to a [`VectorPath`], which is then encoded using `face`'s
/// own [`units_per_em`](SfntFace::units_per_em). On a hit for `key` neither
/// the decoder nor the encoder runs. On any error nothing is cached — mirrors
/// [`SlugBlobCache`]'s own "never poisons the cache on error" contract.
///
/// The raw SFNT glyph id is derived from `key.glyph_id` (SFNT glyph ids are
/// always `u16`; [`SlugGlyphKey::glyph_id`] is a wider `u32` for generality)
/// rather than taken as a separate parameter, so there is exactly one source
/// of truth and no way for a caller to pass a `glyph_id` that disagrees with
/// `key` — out-of-`u16`-range values are a typed
/// [`TextExtractionError::MissingOutlineData`], not undefined behavior.
pub fn extract_slug_glyph(
    cache: &mut SlugBlobCache,
    key: SlugGlyphKey,
    face: &SfntFace,
) -> Result<Arc<SlugGlyph>, TextExtractionError> {
    let glyph_id = u16::try_from(key.glyph_id)
        .map_err(|_| TextExtractionError::MissingOutlineData("glyph id exceeds u16 range"))?;

    cache.get_or_try_encode_with_units_per_em(key, face.units_per_em() as f32, || {
        let outline = face.glyph_outline(glyph_id).map_err(map_sfnt_error)?;
        outline_to_vector_path(&outline)
    })
}

/// Map the narrow SFNT decoder's typed error onto the bridge's error surface.
/// Only [`SfntError::CompositeGlyph`], [`SfntError::GlyphOutOfRange`], and
/// [`SfntError::Malformed`] are reachable from
/// [`SfntFace::glyph_outline`] — the match still names every other variant
/// via a fallback arm because [`SfntError`] is `#[non_exhaustive]`.
fn map_sfnt_error(err: SfntError) -> TextExtractionError {
    match err {
        SfntError::CompositeGlyph => TextExtractionError::CompositeGlyph,
        SfntError::GlyphOutOfRange(_) => {
            TextExtractionError::MissingOutlineData("glyph id out of range")
        }
        SfntError::Malformed(_) => {
            TextExtractionError::MissingOutlineData("malformed glyf/loca data")
        }
        _ => TextExtractionError::MissingOutlineData("SFNT glyph decode failed"),
    }
}

/// Convert a fully-resolved [`GlyphOutline`] into a [`VectorPath`], rejecting
/// an empty outline and any non-quadratic command up front instead of
/// deferring to the Slug encoder for every case (an empty `commands` list —
/// the blank/space-glyph shape — never reaches [`crate::slug::encode_slug_glyph`]
/// at all otherwise, since [`VectorPath::new`] does not validate its input).
fn outline_to_vector_path(outline: &GlyphOutline) -> Result<VectorPath, TextExtractionError> {
    if outline.commands.is_empty() {
        return Err(TextExtractionError::MissingOutlineData(
            "outline has no drawable commands",
        ));
    }
    let mut commands = Vec::with_capacity(outline.commands.len());
    for cmd in &outline.commands {
        let mapped = match *cmd {
            OutlineCommand::MoveTo { x, y } => PathCommand::MoveTo(Vec2::new(x, y)),
            OutlineCommand::LineTo { x, y } => PathCommand::LineTo(Vec2::new(x, y)),
            OutlineCommand::QuadTo { cx, cy, x, y } => PathCommand::QuadTo {
                control: Vec2::new(cx, cy),
                to: Vec2::new(x, y),
            },
            OutlineCommand::Close => PathCommand::Close,
            // `OutlineCommand` is `#[non_exhaustive]`; unlike
            // `crate::outline::glyph_outline_to_path` (which panics — it
            // trusts an already-validated GPU pipeline input), this bridge
            // is the fallible boundary and must degrade instead of crash.
            _ => return Err(TextExtractionError::NonQuadraticCurve),
        };
        commands.push(mapped);
    }
    let b = outline.ink_bounds;
    let bounds = Bounds::new(Vec2::new(b.min_x, b.min_y), Vec2::new(b.max_x, b.max_y));
    Ok(VectorPath::new(commands, FillRule::NonZero, bounds))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_text::OutlineBounds;

    fn quad_outline() -> GlyphOutline {
        GlyphOutline {
            units_per_em: 1000,
            ink_bounds: OutlineBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 100.0,
                max_y: 100.0,
            },
            commands: vec![
                OutlineCommand::MoveTo { x: 0.0, y: 0.0 },
                OutlineCommand::QuadTo {
                    cx: 50.0,
                    cy: 100.0,
                    x: 100.0,
                    y: 0.0,
                },
                OutlineCommand::LineTo { x: 0.0, y: 0.0 },
                OutlineCommand::Close,
            ],
        }
    }

    #[test]
    fn outline_to_vector_path_is_a_faithful_one_to_one_copy() {
        let outline = quad_outline();
        let path = outline_to_vector_path(&outline).unwrap();
        assert_eq!(path.commands.len(), 4);
        assert_eq!(path.fill, FillRule::NonZero);
        assert_eq!(path.bounds.min, Vec2::new(0.0, 0.0));
        assert_eq!(path.bounds.max, Vec2::new(100.0, 100.0));
        assert_eq!(
            path.commands[1],
            PathCommand::QuadTo {
                control: Vec2::new(50.0, 100.0),
                to: Vec2::new(100.0, 0.0),
            }
        );
    }

    #[test]
    fn empty_commands_is_a_typed_missing_outline_rejection() {
        let mut outline = quad_outline();
        outline.commands.clear();
        assert_eq!(
            outline_to_vector_path(&outline),
            Err(TextExtractionError::MissingOutlineData(
                "outline has no drawable commands"
            ))
        );
    }

    #[test]
    fn map_sfnt_error_distinguishes_composite_from_other_decode_failures() {
        assert_eq!(
            map_sfnt_error(SfntError::CompositeGlyph),
            TextExtractionError::CompositeGlyph
        );
        assert_eq!(
            map_sfnt_error(SfntError::GlyphOutOfRange(9)),
            TextExtractionError::MissingOutlineData("glyph id out of range")
        );
        assert_eq!(
            map_sfnt_error(SfntError::Malformed("glyf flags")),
            TextExtractionError::MissingOutlineData("malformed glyf/loca data")
        );
    }

    #[test]
    fn slug_encode_error_converts_unsupported_segment_to_non_quadratic() {
        assert_eq!(
            TextExtractionError::from(SlugEncodeError::UnsupportedSegment),
            TextExtractionError::NonQuadraticCurve
        );
        assert_eq!(
            TextExtractionError::from(SlugEncodeError::EmptyOutline),
            TextExtractionError::MissingOutlineData("outline is empty: no drawable curves")
        );
    }
}
