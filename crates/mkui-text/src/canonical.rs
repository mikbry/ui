//! Canonical fixed-point identity values for outline-affecting state.
//!
//! Two rasterizations of "the same glyph" can legitimately differ along a
//! handful of axes — the active variation coordinate, italic/weight synthesis,
//! and an outline-local affine transform. To key a cache on those axes the
//! values must be **canonical**: byte-for-byte comparable and hashable, with
//! no floating-point ambiguity and no pre-hashed lossy summaries.
//!
//! These are text/font domain types — they deliberately do **not** depend on
//! `mkui-vector2d`. Scene/layout placement is not an outline transform and
//! must never enter a cache key; only the outline-local transform does.

use crate::system::TextError;

/// A four-byte OpenType tag (e.g. `wght`, `ital`, `b"wdth"`).
///
/// Ordered byte-wise so [`VariationSettings`] can sort deterministically by
/// tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OpenTypeTag(pub [u8; 4]);

impl OpenTypeTag {
    /// Construct from a four-byte ASCII tag literal.
    pub const fn new(bytes: [u8; 4]) -> Self {
        OpenTypeTag(bytes)
    }
}

/// A signed Q16.16 fixed-point value.
///
/// The raw `i32` is the canonical representation: equality and hashing derive
/// from it directly, so two `Fixed16_16`s are equal iff their bits are equal.
/// One raw unit is `1 / 65536`; the integer `1.0` is raw `65536`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub struct Fixed16_16(pub i32);

impl Fixed16_16 {
    /// Raw value of the integer `1.0` (`2^16`).
    pub const RAW_ONE: i32 = 1 << 16;

    /// The value `0`.
    pub const ZERO: Self = Fixed16_16(0);

    /// The value `1`.
    pub const ONE: Self = Fixed16_16(Self::RAW_ONE);

    /// Convert from `f32` using one documented deterministic rounding rule:
    /// scale by `2^16`, then **round to nearest, ties away from zero**
    /// (`f32::round`).
    ///
    /// Rejects non-finite inputs and inputs whose scaled magnitude does not
    /// fit in `i32`, returning [`TextError::InvalidFixedPoint`]. This keeps the
    /// fixed-point space free of NaN/∞ sentinels and silent saturation.
    pub fn from_f32(value: f32) -> Result<Self, TextError> {
        if !value.is_finite() {
            return Err(TextError::InvalidFixedPoint);
        }
        let scaled = (value as f64) * (Self::RAW_ONE as f64);
        let rounded = scaled.round();
        if rounded < i32::MIN as f64 || rounded > i32::MAX as f64 {
            return Err(TextError::InvalidFixedPoint);
        }
        Ok(Fixed16_16(rounded as i32))
    }

    /// Convert back to `f32` (lossy for magnitudes beyond `f32` precision).
    pub fn to_f32(self) -> f32 {
        (self.0 as f32) / (Self::RAW_ONE as f32)
    }

    /// The canonical raw representation used for equality/hashing.
    pub const fn raw(self) -> i32 {
        self.0
    }
}

/// A single variation-axis coordinate: a tag and its canonical fixed-point
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariationAxis {
    pub tag: OpenTypeTag,
    pub value: Fixed16_16,
}

/// A canonical, deduplicated, tag-sorted set of variation-axis coordinates.
///
/// Built only through [`VariationSettings::new`], which sorts by tag and
/// **rejects duplicate tags**. Because the inner vector is always in canonical
/// order, the derived `Eq`/`Hash` are stable cache-key material — there is no
/// `variation_axes_hash` summary field.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct VariationSettings(Vec<VariationAxis>);

impl VariationSettings {
    /// Construct from an arbitrary-order axis list. The result is sorted by
    /// tag; a duplicate tag is rejected with
    /// [`TextError::DuplicateVariationAxis`].
    pub fn new(axes: impl IntoIterator<Item = VariationAxis>) -> Result<Self, TextError> {
        let mut axes: Vec<VariationAxis> = axes.into_iter().collect();
        axes.sort_by_key(|axis| axis.tag);
        for pair in axes.windows(2) {
            if pair[0].tag == pair[1].tag {
                return Err(TextError::DuplicateVariationAxis(pair[0].tag));
            }
        }
        Ok(VariationSettings(axes))
    }

    /// The empty (default-instance) settings — no axes set.
    pub const fn empty() -> Self {
        VariationSettings(Vec::new())
    }

    /// Canonical, tag-sorted view of the axes.
    pub fn axes(&self) -> &[VariationAxis] {
        &self.0
    }

    /// Whether any axis is set.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A complete outline-local affine transform in canonical Q16.16 form.
///
/// ```text
/// | a  c  tx |
/// | b  d  ty |
/// ```
///
/// Translation (`tx`, `ty`) is part of the outline-local transform — a
/// 2×2-only transform is insufficient for complete outline identity. This is
/// **not** scene/layout placement, which must never enter a cache key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Affine2Fixed {
    pub a: Fixed16_16,
    pub b: Fixed16_16,
    pub c: Fixed16_16,
    pub d: Fixed16_16,
    pub tx: Fixed16_16,
    pub ty: Fixed16_16,
}

impl Affine2Fixed {
    /// The true Q16.16 identity `(1, 0, 0, 1, 0, 0)` — `a == d == 65536`,
    /// everything else `0`. This is distinct from the all-zero matrix.
    pub const IDENTITY: Self = Affine2Fixed {
        a: Fixed16_16::ONE,
        b: Fixed16_16::ZERO,
        c: Fixed16_16::ZERO,
        d: Fixed16_16::ONE,
        tx: Fixed16_16::ZERO,
        ty: Fixed16_16::ZERO,
    };

    /// The all-zero matrix — a degenerate transform, kept distinct from
    /// [`IDENTITY`](Self::IDENTITY).
    pub const ZERO: Self = Affine2Fixed {
        a: Fixed16_16::ZERO,
        b: Fixed16_16::ZERO,
        c: Fixed16_16::ZERO,
        d: Fixed16_16::ZERO,
        tx: Fixed16_16::ZERO,
        ty: Fixed16_16::ZERO,
    };

    /// Whether this is the true Q16.16 identity.
    pub fn is_identity(self) -> bool {
        self == Self::IDENTITY
    }
}

impl Default for Affine2Fixed {
    fn default() -> Self {
        Self::IDENTITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn axis(tag: &[u8; 4], raw: i32) -> VariationAxis {
        VariationAxis {
            tag: OpenTypeTag::new(*tag),
            value: Fixed16_16(raw),
        }
    }

    #[test]
    fn variation_settings_sort_by_tag() {
        let settings = VariationSettings::new([
            axis(b"wght", 400 << 16),
            axis(b"ital", 0),
            axis(b"wdth", 100 << 16),
        ])
        .unwrap();
        let tags: Vec<_> = settings.axes().iter().map(|a| a.tag).collect();
        assert_eq!(
            tags,
            vec![
                OpenTypeTag::new(*b"ital"),
                OpenTypeTag::new(*b"wdth"),
                OpenTypeTag::new(*b"wght"),
            ]
        );
    }

    #[test]
    fn variation_settings_reject_duplicate_tags() {
        let err = VariationSettings::new([axis(b"wght", 400 << 16), axis(b"wght", 700 << 16)]);
        assert_eq!(
            err,
            Err(TextError::DuplicateVariationAxis(OpenTypeTag::new(
                *b"wght"
            )))
        );
    }

    #[test]
    fn variation_settings_canonical_regardless_of_input_order() {
        let a =
            VariationSettings::new([axis(b"wght", 400 << 16), axis(b"wdth", 100 << 16)]).unwrap();
        let b =
            VariationSettings::new([axis(b"wdth", 100 << 16), axis(b"wght", 400 << 16)]).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn fixed_rejects_non_finite() {
        assert_eq!(
            Fixed16_16::from_f32(f32::NAN),
            Err(TextError::InvalidFixedPoint)
        );
        assert_eq!(
            Fixed16_16::from_f32(f32::INFINITY),
            Err(TextError::InvalidFixedPoint)
        );
        assert_eq!(
            Fixed16_16::from_f32(f32::NEG_INFINITY),
            Err(TextError::InvalidFixedPoint)
        );
    }

    #[test]
    fn fixed_rejects_out_of_range() {
        // 2^31 scaled by 2^16 vastly overflows i32.
        assert_eq!(Fixed16_16::from_f32(1e9), Err(TextError::InvalidFixedPoint));
        assert_eq!(
            Fixed16_16::from_f32(-1e9),
            Err(TextError::InvalidFixedPoint)
        );
    }

    #[test]
    fn fixed_rounds_ties_away_from_zero() {
        // 1.0 -> exactly 65536.
        assert_eq!(Fixed16_16::from_f32(1.0).unwrap(), Fixed16_16::ONE);
        // A half-LSB above zero rounds to 1 raw unit; below zero to -1.
        let half = 0.5 / Fixed16_16::RAW_ONE as f32;
        assert_eq!(Fixed16_16::from_f32(half).unwrap().raw(), 1);
        assert_eq!(Fixed16_16::from_f32(-half).unwrap().raw(), -1);
    }

    #[test]
    fn fixed_hashes_and_compares_raw() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(Fixed16_16(65536));
        assert!(set.contains(&Fixed16_16::ONE));
        assert_ne!(Fixed16_16(0), Fixed16_16(1));
    }

    #[test]
    fn identity_is_real_q16_16_not_zero() {
        assert_eq!(Affine2Fixed::IDENTITY.a, Fixed16_16::ONE);
        assert_eq!(Affine2Fixed::IDENTITY.a.raw(), 65536);
        assert!(Affine2Fixed::IDENTITY.is_identity());
        // The crucial inequality: identity must NOT equal the zero matrix.
        assert_ne!(Affine2Fixed::IDENTITY, Affine2Fixed::ZERO);
        assert!(!Affine2Fixed::ZERO.is_identity());
    }

    #[test]
    fn affine_translation_changes_identity() {
        let translated = Affine2Fixed {
            tx: Fixed16_16::ONE,
            ..Affine2Fixed::IDENTITY
        };
        // Translation is part of outline identity: a translated transform is
        // not equal to the identity.
        assert_ne!(translated, Affine2Fixed::IDENTITY);
        assert!(!translated.is_identity());
    }
}
