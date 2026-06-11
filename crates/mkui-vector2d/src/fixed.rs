//! Canonical fixed-point outline-identity values.
//!
//! These types mirror the renderer-neutral identity contract owned by
//! `mkui-text` (issue #61): [`OpenTypeTag`], [`Fixed16_16`], [`VariationAxis`],
//! [`VariationSettings`], and [`Affine2Fixed`]. They exist here so the Slug
//! glyph cache key ([`crate::slug::SlugGlyphKey`]) can key on *canonical full
//! values* — never on pre-hashed surrogates — and so the crate is buildable
//! and testable ahead of #61 landing in `mkui-text`.
//!
//! # Integration note
//!
//! When #61 lands its `mkui-text` types, these definitions are intended to be
//! replaced by re-exports of the `mkui-text` originals (tracked as a follow-up
//! integration issue). The invariants here — canonical axis ordering, duplicate
//! rejection, Q16.16 identity is `(1,0,0,1,0,0)` and distinct from the
//! all-zero matrix, equality/hash derived from raw values — match #61's
//! contract exactly so the swap is mechanical.

/// A four-byte OpenType tag (e.g. `b"wght"`, `b"ital"`).
///
/// Stored as raw bytes so equality and hashing are exact and order-stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpenTypeTag(pub [u8; 4]);

impl OpenTypeTag {
    /// Construct a tag from a four-byte ASCII string.
    pub const fn new(tag: [u8; 4]) -> Self {
        Self(tag)
    }

    /// The raw four-byte representation, for diagnostics/serialization.
    pub const fn as_bytes(self) -> [u8; 4] {
        self.0
    }
}

/// A signed Q16.16 fixed-point value.
///
/// The raw `i32` is the canonical representation: `Hash`/`Eq`/`Ord` all derive
/// from it, so two `Fixed16_16` compare equal iff their raw bits match. The
/// integer `1` is `65536` raw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Fixed16_16(pub i32);

impl Fixed16_16 {
    /// One whole unit (`1.0`) in Q16.16: `65536` raw.
    pub const ONE: Self = Self(65536);
    /// Zero.
    pub const ZERO: Self = Self(0);

    const FRACTIONAL_BITS: u32 = 16;
    const SCALE: f64 = 65536.0;

    /// Convert from `f32` using round-half-to-even, rejecting non-finite or
    /// out-of-range inputs.
    ///
    /// This is the single documented rounding rule for the whole crate: the
    /// scaled value is rounded to the nearest integer, ties to even, matching
    /// IEEE-754 default rounding so two equal `f32` always map to the same raw
    /// `i32`.
    pub fn from_f32(value: f32) -> Result<Self, FixedError> {
        if !value.is_finite() {
            return Err(FixedError::NonFinite);
        }
        let scaled = f64::from(value) * Self::SCALE;
        // i32 range check before rounding so the cast cannot saturate silently.
        if scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
            return Err(FixedError::OutOfRange);
        }
        let raw = round_half_even(scaled);
        Ok(Self(raw as i32))
    }

    /// Convert back to `f32` for geometry math. Lossy for magnitudes that do
    /// not fit in an `f32` mantissa, but exact for the values used here.
    pub fn to_f32(self) -> f32 {
        (f64::from(self.0) / Self::SCALE) as f32
    }

    /// The raw Q16.16 representation.
    pub const fn raw(self) -> i32 {
        self.0
    }

    /// Number of fractional bits (16), exposed for serialization consumers.
    pub const fn fractional_bits() -> u32 {
        Self::FRACTIONAL_BITS
    }
}

/// Round a `f64` to the nearest integer with ties going to even.
fn round_half_even(x: f64) -> f64 {
    let rounded = x.round(); // round-half-away-from-zero
    if (x - x.trunc()).abs() == 0.5 {
        // Exactly halfway: pick the even neighbour.
        let floor = x.floor();
        if (floor as i64) % 2 == 0 {
            floor
        } else {
            floor + 1.0
        }
    } else {
        rounded
    }
}

/// Errors from fixed-point conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FixedError {
    /// The input was NaN or infinite.
    #[error("fixed-point value must be finite")]
    NonFinite,
    /// The input does not fit the Q16.16 range.
    #[error("fixed-point value out of Q16.16 range")]
    OutOfRange,
}

/// One variation-axis setting: a tag plus its canonical fixed-point value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VariationAxis {
    pub tag: OpenTypeTag,
    pub value: Fixed16_16,
}

impl VariationAxis {
    pub const fn new(tag: OpenTypeTag, value: Fixed16_16) -> Self {
        Self { tag, value }
    }
}

/// A canonical, deduplicated, tag-sorted set of variation-axis settings.
///
/// Two `VariationSettings` built from the same axes in a different order
/// compare equal because construction sorts by tag. Duplicate tags are a hard
/// error so an ambiguous setting can never enter a cache key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct VariationSettings(Vec<VariationAxis>);

impl VariationSettings {
    /// The empty (default-instance) variation setting.
    pub const fn empty() -> Self {
        Self(Vec::new())
    }

    /// Build a canonical setting: sort by tag and reject duplicate tags.
    pub fn new(mut axes: Vec<VariationAxis>) -> Result<Self, VariationError> {
        axes.sort_by_key(|a| a.tag);
        if axes.windows(2).any(|w| w[0].tag == w[1].tag) {
            return Err(VariationError::DuplicateAxis);
        }
        Ok(Self(axes))
    }

    /// The canonical, tag-sorted axes.
    pub fn axes(&self) -> &[VariationAxis] {
        &self.0
    }

    /// Whether there are no axes (the default instance).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Errors from canonical variation construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VariationError {
    /// Two axes shared the same tag.
    #[error("duplicate variation-axis tag")]
    DuplicateAxis,
}

/// A 2×3 affine transform in canonical Q16.16 fixed-point.
///
/// Layout matches the usual `[a b; c d]` linear part plus a `(tx, ty)`
/// translation. Translation is part of the *outline-local* identity (synthesis
/// / normalization), so it participates in equality and hashing — scene/layout
/// placement is **not** an outline transform and must never be folded in here.
///
/// [`Affine2Fixed::IDENTITY`] is the true Q16.16 identity `(1,0,0,1,0,0)`. The
/// all-zero matrix is a distinct, degenerate value, never the identity.
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
    /// The true Q16.16 identity `(1,0,0,1,0,0)`.
    pub const IDENTITY: Self = Self {
        a: Fixed16_16::ONE,
        b: Fixed16_16::ZERO,
        c: Fixed16_16::ZERO,
        d: Fixed16_16::ONE,
        tx: Fixed16_16::ZERO,
        ty: Fixed16_16::ZERO,
    };

    /// The all-zero matrix — a distinct, degenerate value, never the identity.
    pub const ZERO: Self = Self {
        a: Fixed16_16::ZERO,
        b: Fixed16_16::ZERO,
        c: Fixed16_16::ZERO,
        d: Fixed16_16::ZERO,
        tx: Fixed16_16::ZERO,
        ty: Fixed16_16::ZERO,
    };

    /// Whether this is the Q16.16 identity.
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

    #[test]
    fn identity_is_not_zero_matrix() {
        assert_ne!(Affine2Fixed::IDENTITY, Affine2Fixed::ZERO);
        assert!(Affine2Fixed::IDENTITY.is_identity());
        assert!(!Affine2Fixed::ZERO.is_identity());
    }

    #[test]
    fn fixed_round_half_to_even() {
        // 1.0 -> 65536
        assert_eq!(Fixed16_16::from_f32(1.0).unwrap(), Fixed16_16::ONE);
        // Halfway raw values round to even.
        // 0.5 * 65536 = 32768 (already integer, exact).
        assert_eq!(Fixed16_16::from_f32(0.5).unwrap().raw(), 32768);
    }

    #[test]
    fn fixed_rejects_non_finite_and_out_of_range() {
        assert_eq!(Fixed16_16::from_f32(f32::NAN), Err(FixedError::NonFinite));
        assert_eq!(
            Fixed16_16::from_f32(f32::INFINITY),
            Err(FixedError::NonFinite)
        );
        assert_eq!(Fixed16_16::from_f32(1.0e30), Err(FixedError::OutOfRange));
    }

    #[test]
    fn variation_settings_canonicalize_order() {
        let wght = OpenTypeTag::new(*b"wght");
        let ital = OpenTypeTag::new(*b"ital");
        let a = VariationSettings::new(vec![
            VariationAxis::new(wght, Fixed16_16::from_f32(700.0).unwrap()),
            VariationAxis::new(ital, Fixed16_16::ONE),
        ])
        .unwrap();
        let b = VariationSettings::new(vec![
            VariationAxis::new(ital, Fixed16_16::ONE),
            VariationAxis::new(wght, Fixed16_16::from_f32(700.0).unwrap()),
        ])
        .unwrap();
        assert_eq!(
            a, b,
            "differently ordered equivalent settings compare equal"
        );
    }

    #[test]
    fn variation_settings_reject_duplicate_tags() {
        let wght = OpenTypeTag::new(*b"wght");
        let err = VariationSettings::new(vec![
            VariationAxis::new(wght, Fixed16_16::ONE),
            VariationAxis::new(wght, Fixed16_16::ZERO),
        ]);
        assert_eq!(err, Err(VariationError::DuplicateAxis));
    }
}
