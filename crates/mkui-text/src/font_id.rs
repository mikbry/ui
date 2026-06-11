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
//! - Raw values `1..=u64::MAX` are minted by a [`FontIdAllocator`].
//!   Allocated IDs are monotonic, never reused during the process lifetime,
//!   and never wrap — exhaustion surfaces as [`TextError::FontIdOverflow`].
//!
//! The type is **opaque**: callers and providers cannot forge an
//! allocator-owned identity through a public tuple field or unchecked
//! constructor. Only [`FontId::BITMAP`] is publicly constructible; every other
//! value must come from an allocator. The raw value is readable through
//! [`FontId::raw`] purely for diagnostics and serialization.
//!
//! ## Global uniqueness
//!
//! Identities must be unique **across the whole process**, not merely within
//! one allocator: the #62 registry routes by `FontId`, and the #65/#66/#67
//! caches key on it, so two providers that both mint `FontId(1)` would alias.
//! The default [`FontIdAllocator::new`] therefore draws from a single
//! process-global counter — every default allocator shares the same monotonic
//! space, so independent providers can never collide even if the composition
//! root accidentally constructs more than one allocator handle.
//!
//! The intended **registry owns the allocator handle** pattern (#62): the
//! registry holds one [`FontIdAllocator`] and hands it (by reference / clone)
//! to each provider's registration call. Because the default allocator is
//! global-backed, sharing is automatic and re-allocation is impossible.
//!
//! [`FontIdAllocator::isolated`] exists for tests and self-contained sandboxes
//! that need a private, deterministic counter; an isolated allocator does
//! **not** participate in the global space and must not mint ids that escape
//! into shared registry/cache data.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::system::TextError;

/// Process-global font-id counter backing every default [`FontIdAllocator`].
/// Starts at `1`; raw `0` is the reserved bitmap face and is never handed out.
/// A value of `0` is also the *exhausted* sentinel, only reachable after
/// `u64::MAX` has been minted (see [`FontIdAllocator::allocate`]).
static GLOBAL_FONT_ID_COUNTER: AtomicU64 = AtomicU64::new(1);

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

/// Monotonic allocator for [`FontId`]s in the range `1..=u64::MAX`.
///
/// By default ([`new`](Self::new)) every allocator draws from a single
/// **process-global** counter, so all default allocators share one monotonic
/// space and independent providers can never mint colliding ids. The intended
/// pattern is *registry owns the allocator handle* (#62): one handle is shared
/// across all providers.
///
/// Guarantees (for the shared/global allocator):
/// - **Monotonic** — each [`allocate`](Self::allocate) returns a strictly
///   larger raw value than the previous successful call across the process.
/// - **Globally unique** — no two successful allocations anywhere in the
///   process return the same id.
/// - **Never reused** — a handed-out id is never returned again for the
///   lifetime of the process.
/// - **Never wraps** — after `u64::MAX` is handed out, every subsequent call
///   returns [`TextError::FontIdOverflow`] rather than wrapping back onto
///   live (or reserved) ids.
///
/// [`isolated`](Self::isolated) trades the global guarantee for a private,
/// deterministic counter — for tests and sandboxes only.
#[derive(Debug)]
pub struct FontIdAllocator {
    /// `None` ⇒ draw from [`GLOBAL_FONT_ID_COUNTER`] (the default,
    /// globally-unique path). `Some` ⇒ an isolated, private counter.
    local: Option<AtomicU64>,
}

impl FontIdAllocator {
    /// A globally-unique allocator backed by the process-global counter.
    /// Sharing across providers is automatic; constructing several of these is
    /// safe because they all draw from the same space.
    pub const fn new() -> Self {
        Self { local: None }
    }

    /// An **isolated** allocator with a private counter starting at raw `1`.
    ///
    /// Does **not** participate in the process-global space — two isolated
    /// allocators will each begin at `1` and collide. Use only for tests and
    /// self-contained sandboxes whose ids never reach shared registry/cache
    /// data.
    pub const fn isolated() -> Self {
        Self {
            local: Some(AtomicU64::new(1)),
        }
    }

    /// The counter this allocator draws from — the global one by default, or
    /// its private one when isolated.
    fn counter(&self) -> &AtomicU64 {
        match &self.local {
            Some(local) => local,
            None => &GLOBAL_FONT_ID_COUNTER,
        }
    }

    /// Mint the next monotonic [`FontId`], or [`TextError::FontIdOverflow`] if
    /// the `1..=u64::MAX` space is exhausted.
    pub fn allocate(&self) -> Result<FontId, TextError> {
        let counter = self.counter();
        let mut current = counter.load(Ordering::Relaxed);
        loop {
            // `0` is the exhausted sentinel — set only after `u64::MAX` was
            // handed out. We never hand out `0` (the reserved bitmap face).
            if current == 0 {
                return Err(TextError::FontIdOverflow);
            }
            // After handing out `u64::MAX` the next value would wrap to `0`;
            // store the `0` sentinel instead of a wrapped, reusable value.
            let next = current.checked_add(1).unwrap_or(0);
            match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
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
    fn isolated_allocator_starts_at_one_and_is_monotonic() {
        let alloc = FontIdAllocator::isolated();
        let a = alloc.allocate().unwrap();
        let b = alloc.allocate().unwrap();
        let c = alloc.allocate().unwrap();
        assert_eq!((a.raw(), b.raw(), c.raw()), (1, 2, 3));
        assert!(a.raw() < b.raw() && b.raw() < c.raw());
    }

    #[test]
    fn allocated_ids_never_collide_with_bitmap() {
        let alloc = FontIdAllocator::isolated();
        for _ in 0..1000 {
            assert_ne!(alloc.allocate().unwrap(), FontId::BITMAP);
        }
    }

    #[test]
    fn isolated_allocator_ids_are_unique() {
        use std::collections::HashSet;
        let alloc = FontIdAllocator::isolated();
        let mut seen = HashSet::new();
        for _ in 0..10_000 {
            assert!(
                seen.insert(alloc.allocate().unwrap()),
                "duplicate id minted"
            );
        }
    }

    #[test]
    fn independent_default_allocators_never_collide() {
        // The bug this guards: two providers, each with their *own* allocator,
        // both minting one id. With the global-backed default they must still
        // produce distinct, non-bitmap ids — never both `FontId(1)`.
        let provider_a = FontIdAllocator::new();
        let provider_b = FontIdAllocator::new();
        let id_a = provider_a.allocate().unwrap();
        let id_b = provider_b.allocate().unwrap();
        assert_ne!(id_a, id_b, "independent default allocators collided");
        assert_ne!(id_a, FontId::BITMAP);
        assert_ne!(id_b, FontId::BITMAP);
    }

    #[test]
    fn overflow_hands_out_max_then_errors_without_wrapping() {
        // Drive an isolated allocator to the very top of the space.
        let alloc = FontIdAllocator {
            local: Some(AtomicU64::new(u64::MAX)),
        };
        // The last legal id is u64::MAX itself.
        assert_eq!(alloc.allocate().unwrap().raw(), u64::MAX);
        // Every subsequent call errors — no wraparound onto 0/1/...
        assert_eq!(alloc.allocate(), Err(TextError::FontIdOverflow));
        assert_eq!(alloc.allocate(), Err(TextError::FontIdOverflow));
    }
}
