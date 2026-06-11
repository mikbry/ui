//! Globally-unique font identity + the monotonic allocator that mints it.
//!
//! [`FontId`] is the **only** public face identity threaded through
//! [`TextSystem`](crate::TextSystem), [`LayoutSpec`](crate::LayoutSpec),
//! [`LayoutRun`](crate::LayoutRun), and [`GlyphCacheKey`](crate::GlyphCacheKey).
//! There is deliberately no parallel `GlobalFaceId`.
//!
//! ## Identity domain
//!
//! - Raw value `0` ([`FontId::BITMAP`]) is the permanently reserved built-in
//!   bitmap face shipped by [`BitmapTextSystem`](crate::BitmapTextSystem).
//! - Raw values `1..=u64::MAX` are minted by a shared [`FontIdAllocator`].
//!   Allocated IDs are monotonic, never reused during the process lifetime,
//!   and never wrap — exhaustion surfaces as [`TextError::FontIdOverflow`].
//!
//! The type is **opaque**: callers and providers cannot forge an
//! allocator-owned identity through a public tuple field or unchecked
//! constructor. Only [`FontId::BITMAP`] is publicly constructible; every other
//! value must come from an allocator. The raw value is readable through
//! [`FontId::raw`] purely for diagnostics and serialization.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::system::TextError;

/// Opaque, globally-unique font handle.
///
/// Construct one of two ways:
/// - [`FontId::BITMAP`] — the reserved built-in bitmap face (raw `0`).
/// - [`FontIdAllocator::allocate`] — a fresh monotonic id (raw `>= 1`).
///
/// There is intentionally no public constructor that accepts an arbitrary raw
/// value: that would let a provider mint an identity the allocator believes it
/// still owns, aliasing two distinct faces onto one cache namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct FontId(u64);

impl FontId {
    /// The permanently reserved built-in bitmap face (raw value `0`).
    pub const BITMAP: FontId = FontId(0);

    /// Read-only access to the raw 64-bit value, for diagnostics and
    /// serialization only. Round-tripping this through a public constructor is
    /// intentionally impossible — deserializers must re-resolve through a
    /// registry rather than forge the identity.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// Crate-internal constructor used by [`FontIdAllocator`] (and the bitmap
    /// face). Not public: see the type-level docs.
    pub(crate) const fn from_raw(raw: u64) -> Self {
        FontId(raw)
    }
}

/// Shared monotonic allocator for [`FontId`]s in the range `1..=u64::MAX`.
///
/// One allocator is registered per process (see #62); concrete providers
/// receive the registered allocator and mint their faces through it, so
/// provider-local IDs never escape into public layout/cache data. The
/// allocator is `Send + Sync` and lock-free.
///
/// Guarantees:
/// - **Monotonic** — each [`allocate`](Self::allocate) returns a strictly
///   larger raw value than the previous successful call.
/// - **Never reused** — a handed-out id is never returned again for the
///   lifetime of the allocator.
/// - **Never wraps** — after `u64::MAX` is handed out, every subsequent call
///   returns [`TextError::FontIdOverflow`] rather than wrapping back onto
///   live (or reserved) ids.
#[derive(Debug)]
pub struct FontIdAllocator {
    /// Next raw value to hand out. `0` is the exhausted sentinel: it is never
    /// a valid hand-out (raw `0` is the reserved bitmap face) and is only
    /// reached after `u64::MAX` has been allocated.
    next: AtomicU64,
}

impl FontIdAllocator {
    /// Create an allocator whose first id will be raw `1`.
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Mint the next monotonic [`FontId`], or [`TextError::FontIdOverflow`] if
    /// the `1..=u64::MAX` space is exhausted.
    pub fn allocate(&self) -> Result<FontId, TextError> {
        let mut current = self.next.load(Ordering::Relaxed);
        loop {
            // `0` is the exhausted sentinel — set only after `u64::MAX` was
            // handed out. We never hand out `0` (the reserved bitmap face).
            if current == 0 {
                return Err(TextError::FontIdOverflow);
            }
            // After handing out `u64::MAX` the next value would wrap to `0`;
            // store the `0` sentinel instead of a wrapped, reusable value.
            let next = current.checked_add(1).unwrap_or(0);
            match self.next.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(FontId::from_raw(current)),
                Err(observed) => current = observed,
            }
        }
    }
}

impl Default for FontIdAllocator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmap_is_raw_zero() {
        assert_eq!(FontId::BITMAP.raw(), 0);
        assert_eq!(FontId::default(), FontId::BITMAP);
    }

    #[test]
    fn allocator_starts_at_one_and_is_monotonic() {
        let alloc = FontIdAllocator::new();
        let a = alloc.allocate().unwrap();
        let b = alloc.allocate().unwrap();
        let c = alloc.allocate().unwrap();
        assert_eq!(a.raw(), 1);
        assert_eq!(b.raw(), 2);
        assert_eq!(c.raw(), 3);
        assert!(a.raw() < b.raw() && b.raw() < c.raw());
    }

    #[test]
    fn allocated_ids_never_collide_with_bitmap() {
        let alloc = FontIdAllocator::new();
        for _ in 0..1000 {
            assert_ne!(alloc.allocate().unwrap(), FontId::BITMAP);
        }
    }

    #[test]
    fn allocated_ids_are_unique() {
        use std::collections::HashSet;
        let alloc = FontIdAllocator::new();
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            assert!(
                seen.insert(alloc.allocate().unwrap()),
                "duplicate id minted"
            );
        }
    }

    #[test]
    fn overflow_hands_out_max_then_errors_without_wrapping() {
        // Drive the allocator to the very top of the space.
        let alloc = FontIdAllocator {
            next: AtomicU64::new(u64::MAX),
        };
        // The last legal id is u64::MAX itself.
        assert_eq!(alloc.allocate().unwrap().raw(), u64::MAX);
        // Every subsequent call errors — no wraparound onto 0/1/...
        assert_eq!(alloc.allocate(), Err(TextError::FontIdOverflow));
        assert_eq!(alloc.allocate(), Err(TextError::FontIdOverflow));
    }
}
