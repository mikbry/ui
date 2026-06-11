//! Font registry + backend-neutral composite/router [`TextSystem`].
//!
//! [`CompositeTextSystem`] lets bitmap and Slug (and, from #67, outline) text
//! be live in the same scene without renderer-global conditionals or
//! provider-local [`FontId`] collisions. It owns:
//!
//! - a registry-owned [`FontIdAllocator`] (the #61 shared allocator handle),
//!   which is the **only** thing that mints public [`FontId`] values, and
//! - a registry holding two maps kept in lockstep:
//!     - `(ProviderId, LocalFontId) -> FontId` (forward), and
//!     - `FontId -> FaceRecord` (reverse: provider, local id, generation,
//!       render class, source).
//!
//! ## The router never forges a `FontId`
//!
//! [`FontId`] is opaque (#61): only [`FontIdAllocator::allocate`] or the
//! reserved [`FontId::BITMAP`] can produce one. The registry mints every
//! non-bitmap id through its allocator at **registration** time and records
//! the `(ProviderId, LocalFontId)` it maps to. At **dispatch** time the router
//! reverse-looks-up the public `FontId` to recover `(ProviderId, LocalFontId)`
//! and hands the provider only its private [`LocalFontId`] — it never
//! reconstructs a `FontId` from a provider-local value. Provider-local face
//! IDs ([`LocalFontId`]) are crate-private and never leave this boundary, so
//! two providers may both use `LocalFontId(0)` without colliding.
//!
//! Provider-produced [`LayoutRun`]s are rewritten so the public run carries the
//! global `FontId`, the registry generation, and the selected
//! [`TextRenderClass`].
//!
//! ## Generic-only boundary (#62)
//!
//! This module ships the registry/router and the crate-private provider
//! contract only. No concrete SFNT parser, TTF byte handling, Slug encoder, or
//! WGPU type lands here — the outline provider (`SfntTextSystem`) registers in
//! #67.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::bitmap::{BitmapTextSystem, BITMAP_FAMILY};
use crate::font_id::{FontId, FontIdAllocator};
use crate::outline::{GlyphOutline, OutlineKey};
use crate::system::{
    FontQuery, GlyphCacheKey, GlyphImage, LayoutRun, LayoutSpec, TextError, TextRenderClass,
    TextSystem,
};

/// Where a registered face's data originates — modeled **separately** from
/// [`TextRenderClass`].
///
/// Source answers "where did the bytes come from"; render class answers
/// "which pipeline draws it". The two axes are orthogonal: an `Outline`-class
/// face and a `Slug`-class face can both be [`FontSource::Bytes`], and the
/// built-in bitmap is the only [`FontSource::Builtin`] face.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FontSource {
    /// Compiled-in face owned by the implementation (the 5×7 bitmap).
    #[default]
    Builtin,
    /// Face registered from an external byte blob. The bytes themselves stay
    /// private to the owning provider / registry; this variant only records
    /// that the face came from outside the binary.
    Bytes,
}

/// Index of a registered provider inside a [`CompositeTextSystem`].
///
/// Crate-private: callers only ever see the public [`FontId`]. `ProviderId(0)`
/// is always the built-in bitmap adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct ProviderId(pub(crate) usize);

/// A provider-local face identifier.
///
/// Never leaves the registry / provider-adapter boundary — two providers may
/// both use `LocalFontId(0)` without colliding because the registry maps each
/// `(ProviderId, LocalFontId)` pair to a distinct allocator-minted [`FontId`].
/// Crucially, the router translates `FontId -> LocalFontId` (reverse lookup)
/// and never the other way: a `LocalFontId` can never be turned back into a
/// public `FontId` by reconstruction, only by the stored mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct LocalFontId(pub(crate) u64);

/// Errors from the crate-internal registry API (registration + reload).
///
/// Distinct from [`TextError`] (the public `TextSystem` surface) because these
/// are build-time registry-management failures, not per-call text failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub(crate) enum RegistryError {
    /// Reload/lookup referenced a `FontId` not in the registry.
    #[error("unknown font: {0:?}")]
    UnknownFont(FontId),
    /// The shared [`FontIdAllocator`] has exhausted the `1..=u64::MAX` space.
    #[error("font id space exhausted")]
    FontIdOverflow,
    /// A face reload would overflow `font_generation`. The generation counter
    /// never wraps into an old cache namespace — on exhaustion the existing
    /// mapping is left intact and this error is returned.
    #[error("font generation exhausted for {0:?}")]
    GenerationOverflow(FontId),
}

/// Crate-private adapter every backend implements so the composite can route
/// to it. Distinct from the public [`TextSystem`] trait: its methods are keyed
/// by the provider's own [`LocalFontId`], and the composite is the only caller.
///
/// "The provider-local adapter may be crate-private" (#62) — this trait, plus
/// [`LocalFontId`] / [`ProviderId`], are exactly that boundary. Public APIs
/// continue to expose only [`FontId`]; no `GlobalFaceId` type is introduced,
/// and a provider is never handed a public `FontId` to act on — only its own
/// `LocalFontId`.
pub(crate) trait FontProvider: Send + Sync + 'static {
    /// The render lane every face this provider exposes draws through.
    fn render_class(&self) -> TextRenderClass;

    /// Human-readable family names this provider exposes.
    fn families(&self) -> Vec<String>;

    /// Resolve a query to one of this provider's local faces, or `None` to
    /// decline (so a sibling provider can claim the query).
    fn resolve_local(&self, query: &FontQuery) -> Option<LocalFontId>;

    /// Lay out `text` for provider-local face `local`. The provider keys on
    /// `local`, **not** on `spec.font_id` (which still holds the public id);
    /// the composite rewrites the returned runs to the global id + render
    /// class + registry generation before they reach the caller.
    fn layout_local(
        &self,
        local: LocalFontId,
        text: &str,
        spec: &LayoutSpec,
        max_width_px: Option<f32>,
    ) -> Vec<LayoutRun>;

    /// Rasterize `key` against provider-local face `local`. The provider keys
    /// on `local`; `key.font_id` still holds the public id (the composite does
    /// not rewrite it, because it never forges a local one).
    fn rasterize_local(
        &self,
        local: LocalFontId,
        key: &GlyphCacheKey,
    ) -> Result<GlyphImage, TextError>;

    /// Resolve a glyph outline for provider-local face `local`.
    ///
    /// The default returns [`TextError::UnsupportedOutline`] — most providers
    /// (bitmap, Slug) have no CPU outline. The composite reaches an
    /// **override** for a registered outline face by routing through the
    /// reverse map; it must never silently fall back to this default for such a
    /// face.
    fn outline_local(
        &self,
        _local: LocalFontId,
        _key: &OutlineKey,
    ) -> Result<GlyphOutline, TextError> {
        Err(TextError::UnsupportedOutline)
    }
}

/// Reverse-map entry: everything the router needs to dispatch a public
/// [`FontId`] to its owning provider and rewrite outputs.
#[derive(Debug, Clone)]
struct FaceRecord {
    provider_id: ProviderId,
    local: LocalFontId,
    generation: u32,
    render_class: TextRenderClass,
    source: FontSource,
}

/// Forward + reverse maps, plus a family index for `resolve_font`.
#[derive(Debug, Default)]
struct FontRegistry {
    forward: HashMap<(ProviderId, LocalFontId), FontId>,
    reverse: HashMap<FontId, FaceRecord>,
    by_family: HashMap<String, FontId>,
}

/// Checked `font_generation` bump.
///
/// Never wraps: at `u32::MAX` it returns [`RegistryError::GenerationOverflow`]
/// so the caller can leave the existing mapping intact rather than aliasing a
/// fresh generation onto a stale cache namespace.
fn next_generation(current: u32, font_id: FontId) -> Result<u32, RegistryError> {
    current
        .checked_add(1)
        .ok_or(RegistryError::GenerationOverflow(font_id))
}

/// Backend-neutral composite/router [`TextSystem`].
///
/// Seeded with the built-in 5×7 bitmap face as [`FontId::BITMAP`] /
/// `ProviderId(0)` / [`TextRenderClass::Bitmap`]. Additional providers + faces
/// register through the crate-internal registry API; every public request
/// routes by reverse lookup, and the router never constructs a [`FontId`].
pub struct CompositeTextSystem {
    providers: Vec<Box<dyn FontProvider>>,
    registry: FontRegistry,
    /// The shared #61 allocator handle. The registry owns it and is the only
    /// thing that mints public [`FontId`] values (the bitmap face uses the
    /// reserved [`FontId::BITMAP`], not the allocator).
    allocator: FontIdAllocator,
}

impl std::fmt::Debug for CompositeTextSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeTextSystem")
            .field("providers", &self.providers.len())
            .field("faces", &self.registry.reverse.len())
            .finish()
    }
}

impl Default for CompositeTextSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeTextSystem {
    /// Build a composite seeded with the built-in bitmap face:
    /// [`FontId::BITMAP`], `ProviderId(0)`, [`TextRenderClass::Bitmap`],
    /// [`FontSource::Builtin`], selectable by [`BITMAP_FAMILY`].
    pub fn new() -> Self {
        let mut system = Self {
            providers: Vec::new(),
            registry: FontRegistry::default(),
            allocator: FontIdAllocator::new(),
        };

        // Provider 0 is always the bitmap adapter; its single face is the
        // reserved FontId::BITMAP, recorded directly (the allocator only mints
        // the 1..=u64::MAX range).
        let provider_id = system.push_provider(Box::new(BitmapProvider::new()));
        debug_assert_eq!(provider_id, ProviderId(0));
        system
            .registry
            .forward
            .insert((provider_id, LocalFontId(0)), FontId::BITMAP);
        system.registry.reverse.insert(
            FontId::BITMAP,
            FaceRecord {
                provider_id,
                local: LocalFontId(0),
                generation: 0,
                render_class: TextRenderClass::Bitmap,
                source: FontSource::Builtin,
            },
        );
        system
            .registry
            .by_family
            .insert(BITMAP_FAMILY.to_string(), FontId::BITMAP);

        system
    }

    fn push_provider(&mut self, provider: Box<dyn FontProvider>) -> ProviderId {
        let id = ProviderId(self.providers.len());
        self.providers.push(provider);
        id
    }

    /// Register an additional provider, returning its [`ProviderId`] for use
    /// with [`register_face`](Self::register_face).
    ///
    /// Crate-internal registration API: consumed by the in-crate outline
    /// provider in #67, and exercised by the mock-provider tests here. No
    /// shipping caller exists in #62 (generic-only boundary).
    #[allow(dead_code)]
    pub(crate) fn add_provider(&mut self, provider: Box<dyn FontProvider>) -> ProviderId {
        self.push_provider(provider)
    }

    /// Register the face that `provider_id` exposes at provider-local `local`,
    /// **minting a fresh public [`FontId`] from the registry-owned allocator**
    /// and indexing `families`.
    ///
    /// Forward + reverse maps are updated **atomically**: the allocator is
    /// drawn first, so on [`RegistryError::FontIdOverflow`] no map is touched
    /// and no id is leaked. Re-registering an already-mapped `(provider, local)`
    /// pair is idempotent — it returns the existing [`FontId`] without a second
    /// allocation. The router obtains the `FontId` *only* here; it never
    /// reconstructs one from `local`.
    #[allow(dead_code)]
    pub(crate) fn register_face(
        &mut self,
        provider_id: ProviderId,
        local: LocalFontId,
        source: FontSource,
        families: Vec<String>,
    ) -> Result<FontId, RegistryError> {
        if let Some(&existing) = self.registry.forward.get(&(provider_id, local)) {
            return Ok(existing);
        }

        let render_class = self.providers[provider_id.0].render_class();
        // Mint first: if the id space is exhausted we return here, before
        // mutating either map, so the registry is left exactly as it was.
        let font_id = self
            .allocator
            .allocate()
            .map_err(|_| RegistryError::FontIdOverflow)?;

        self.registry.forward.insert((provider_id, local), font_id);
        self.registry.reverse.insert(
            font_id,
            FaceRecord {
                provider_id,
                local,
                generation: 0,
                render_class,
                source,
            },
        );
        for family in families {
            self.registry.by_family.insert(family, font_id);
        }
        Ok(font_id)
    }

    /// Reload/replace the bytes backing `font_id`, bumping `font_generation`
    /// with checked arithmetic so downstream caches cannot alias stale
    /// outlines. The public [`FontId`] identity is **preserved**.
    ///
    /// On generation exhaustion the old mapping is left intact and
    /// [`RegistryError::GenerationOverflow`] is returned. Returns the new
    /// generation on success.
    #[allow(dead_code)]
    pub(crate) fn reload_face(
        &mut self,
        font_id: FontId,
        source: FontSource,
    ) -> Result<u32, RegistryError> {
        let record = self
            .registry
            .reverse
            .get_mut(&font_id)
            .ok_or(RegistryError::UnknownFont(font_id))?;
        // Compute the bump before mutating: on exhaustion the record is left
        // untouched (old generation + source intact).
        let next = next_generation(record.generation, font_id)?;
        record.generation = next;
        record.source = source;
        Ok(next)
    }

    /// Render lane selected for a registered face, or `None` if `font_id` is
    /// unknown. Render class is a property of font/style resolution — not a
    /// renderer-global switch.
    pub fn render_class(&self, font_id: FontId) -> Option<TextRenderClass> {
        self.registry.reverse.get(&font_id).map(|r| r.render_class)
    }

    /// Origin of a registered face's data, modeled separately from its render
    /// class. `None` if `font_id` is unknown.
    pub fn font_source(&self, font_id: FontId) -> Option<FontSource> {
        self.registry.reverse.get(&font_id).map(|r| r.source)
    }

    /// Current `font_generation` for a registered face — available to
    /// downstream cache keys. Bumped by [`reload_face`](Self::reload_face).
    /// `None` if `font_id` is unknown.
    pub fn font_generation(&self, font_id: FontId) -> Option<u32> {
        self.registry.reverse.get(&font_id).map(|r| r.generation)
    }
}

impl TextSystem for CompositeTextSystem {
    fn resolve_font(&self, query: &FontQuery) -> Option<FontId> {
        // Family index first: registered faces resolve directly by family.
        if let Some(family) = &query.family {
            if let Some(&id) = self.registry.by_family.get(family) {
                return Some(id);
            }
        }
        // Otherwise ask each provider in registration order; the first to claim
        // the query wins, mapped back to its public id via the forward map.
        // This exercises the per-provider `(ProviderId, LocalFontId)` isolation
        // directly — the public id comes from the stored mapping, never from
        // reconstructing one out of the local id.
        for (index, provider) in self.providers.iter().enumerate() {
            if let Some(local) = provider.resolve_local(query) {
                if let Some(&id) = self.registry.forward.get(&(ProviderId(index), local)) {
                    return Some(id);
                }
            }
        }
        // Default: the built-in bitmap face.
        Some(FontId::BITMAP)
    }

    fn register_font_bytes(&self, _bytes: Arc<[u8]>, _index: u32) -> Result<FontId, TextError> {
        // No byte-parsing provider ships in #62 (generic-only boundary): the
        // SFNT byte path lands with the outline provider in #67. Faces register
        // through the registry API at build time, not through this
        // infallible-bytes entry point.
        Err(TextError::InvalidFontBytes)
    }

    fn layout(&self, text: &str, spec: &LayoutSpec, max_width_px: Option<f32>) -> Vec<LayoutRun> {
        // Reverse-map the public FontId to its owning provider + local face.
        // layout is infallible: an unknown id — which the FontId opacity (#61)
        // makes callers unable to forge — degrades to the bitmap face rather
        // than panicking, and the rewritten run reports FontId::BITMAP.
        let (public_id, provider_id, local, generation, render_class) =
            match self.registry.reverse.get(&spec.font_id) {
                Some(rec) => (
                    spec.font_id,
                    rec.provider_id,
                    rec.local,
                    rec.generation,
                    rec.render_class,
                ),
                None => (
                    FontId::BITMAP,
                    ProviderId(0),
                    LocalFontId(0),
                    0,
                    TextRenderClass::Bitmap,
                ),
            };

        // Hand the provider its private local id; the spec still carries the
        // public id, which the provider ignores for identity.
        let mut runs = self.providers[provider_id.0].layout_local(local, text, spec, max_width_px);

        // Rewrite every provider-local run so the public output carries the
        // global FontId, the registry generation, and the selected lane.
        for run in &mut runs {
            run.font_id = public_id;
            run.font_generation = generation;
            run.render_class = render_class;
        }
        runs
    }

    fn rasterize(&self, key: GlyphCacheKey) -> Result<GlyphImage, TextError> {
        // Fallible: an unknown id is rejected with TextError, never routed to a
        // provider fallback by accident.
        let record = self
            .registry
            .reverse
            .get(&key.font_id)
            .ok_or(TextError::UnknownFont(key.font_id))?;
        // Dispatch by the stored local id; the public key is passed through
        // unchanged (the router does not reconstruct a local FontId).
        self.providers[record.provider_id.0].rasterize_local(record.local, &key)
    }

    fn glyph_outline(&self, key: &OutlineKey) -> Result<GlyphOutline, TextError> {
        // Fallible: route through the reverse map to the owning provider. An
        // unknown id is rejected with TextError::UnknownFont, distinct from a
        // routed provider's TextError::UnsupportedOutline.
        let record = self
            .registry
            .reverse
            .get(&key.font_id)
            .ok_or(TextError::UnknownFont(key.font_id))?;
        self.providers[record.provider_id.0].outline_local(record.local, key)
    }

    fn families(&self) -> Vec<String> {
        // De-duplicated union over providers, registration order preserved.
        let mut seen = HashSet::new();
        let mut names = Vec::new();
        for provider in &self.providers {
            for family in provider.families() {
                if seen.insert(family.clone()) {
                    names.push(family);
                }
            }
        }
        names
    }
}

/// Crate-private provider adapter wrapping the built-in [`BitmapTextSystem`]
/// behind the [`FontProvider`] contract. Exposes exactly one face,
/// `LocalFontId(0)`, in the [`TextRenderClass::Bitmap`] lane.
struct BitmapProvider {
    inner: BitmapTextSystem,
}

impl BitmapProvider {
    fn new() -> Self {
        Self {
            inner: BitmapTextSystem::new(),
        }
    }
}

impl FontProvider for BitmapProvider {
    fn render_class(&self) -> TextRenderClass {
        TextRenderClass::Bitmap
    }

    fn families(&self) -> Vec<String> {
        self.inner.families()
    }

    fn resolve_local(&self, query: &FontQuery) -> Option<LocalFontId> {
        // Claim an unspecified family or our own; decline other families so
        // sibling providers can claim them.
        match &query.family {
            None => Some(LocalFontId(0)),
            Some(family) if family.as_str() == BITMAP_FAMILY => Some(LocalFontId(0)),
            Some(_) => None,
        }
    }

    fn layout_local(
        &self,
        _local: LocalFontId,
        text: &str,
        spec: &LayoutSpec,
        max_width_px: Option<f32>,
    ) -> Vec<LayoutRun> {
        self.inner.layout(text, spec, max_width_px)
    }

    fn rasterize_local(
        &self,
        _local: LocalFontId,
        key: &GlyphCacheKey,
    ) -> Result<GlyphImage, TextError> {
        self.inner.rasterize(key.clone())
    }

    // outline_local uses the trait default — the bitmap face has no CPU
    // outline (TextError::UnsupportedOutline).
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::cache_key_for;
    use crate::canonical::{Affine2Fixed, VariationSettings};
    use crate::outline::{OutlineBounds, OutlineCommand};
    use crate::system::{GlyphFormat, LayoutGlyph};

    /// Configurable in-crate mock provider. Each instance exposes one local
    /// face (`LocalFontId(0)`); a distinct `tag` byte proves which provider a
    /// rasterize request actually reached.
    struct MockProvider {
        render_class: TextRenderClass,
        family: String,
        tag: u8,
        canned_outline: Option<GlyphOutline>,
    }

    impl FontProvider for MockProvider {
        fn render_class(&self) -> TextRenderClass {
            self.render_class
        }

        fn families(&self) -> Vec<String> {
            vec![self.family.clone()]
        }

        fn resolve_local(&self, query: &FontQuery) -> Option<LocalFontId> {
            match &query.family {
                Some(f) if f.as_str() == self.family => Some(LocalFontId(0)),
                _ => None,
            }
        }

        fn layout_local(
            &self,
            _local: LocalFontId,
            text: &str,
            spec: &LayoutSpec,
            _max_width_px: Option<f32>,
        ) -> Vec<LayoutRun> {
            // Emit one run carrying the public font id from `spec` and a
            // deliberately wrong render class; the composite must overwrite
            // both (and the generation).
            vec![LayoutRun {
                font_id: spec.font_id,
                render_class: TextRenderClass::Bitmap,
                font_generation: spec.font_generation,
                font_size_px: spec.font_size_px,
                variations: spec.variations.clone(),
                synthesis_flags: spec.synthesis_flags,
                hinting: spec.hinting,
                origin_x_px: 0.0,
                origin_y_px: 0.0,
                line_y_baseline_px: 0.0,
                line_ascent_px: 0.0,
                line_descent_px: 0.0,
                glyphs: vec![LayoutGlyph {
                    glyph_id: text.chars().next().map_or(0, |c| c as u32),
                    x_px: 0.0,
                    y_px: 0.0,
                    x_advance_px: 0.0,
                    x_offset_px: 0.0,
                    y_offset_px: 0.0,
                    cluster: 0,
                    subpixel_variant: 0,
                    format: GlyphFormat::Alpha,
                }],
            }]
        }

        fn rasterize_local(
            &self,
            _local: LocalFontId,
            _key: &GlyphCacheKey,
        ) -> Result<GlyphImage, TextError> {
            // 1×1 image tagged with this provider's byte, so a test can assert
            // the request routed to THIS provider and no other.
            Ok(GlyphImage {
                width_px: 1,
                height_px: 1,
                left_px: 0,
                top_px: 0,
                format: GlyphFormat::Alpha,
                data: Arc::from(vec![self.tag].into_boxed_slice()),
            })
        }

        fn outline_local(
            &self,
            _local: LocalFontId,
            key: &OutlineKey,
        ) -> Result<GlyphOutline, TextError> {
            match &self.canned_outline {
                Some(o) => Ok(o.clone()),
                None => Err(TextError::UnknownGlyph(key.glyph_id)),
            }
        }
    }

    fn mock(render_class: TextRenderClass, family: &str, tag: u8) -> Box<MockProvider> {
        Box::new(MockProvider {
            render_class,
            family: family.to_string(),
            tag,
            canned_outline: None,
        })
    }

    /// Build a cache key for the given public `FontId` (canonical
    /// `GlyphCacheKey` is not `Copy`, so spread a fresh `cache_key_for`).
    fn key_for(font_id: FontId) -> GlyphCacheKey {
        GlyphCacheKey {
            font_id,
            ..cache_key_for('A', 10.0)
        }
    }

    // ---- Collision isolation + routing -----------------------------------

    #[test]
    fn two_providers_local_zero_get_distinct_ids_and_route_correctly() {
        let mut sys = CompositeTextSystem::new();
        let pa = sys.add_provider(mock(TextRenderClass::Outline, "MockA", 11));
        let pb = sys.add_provider(mock(TextRenderClass::Slug, "MockB", 22));

        // Both providers use LocalFontId(0); the registry must mint distinct
        // public ids, neither colliding with the reserved bitmap face.
        let id_a = sys
            .register_face(pa, LocalFontId(0), FontSource::Bytes, vec!["MockA".into()])
            .unwrap();
        let id_b = sys
            .register_face(pb, LocalFontId(0), FontSource::Bytes, vec!["MockB".into()])
            .unwrap();
        assert_ne!(id_a, id_b);
        assert_ne!(id_a, FontId::BITMAP);
        assert_ne!(id_b, FontId::BITMAP);

        // resolve-by-family lands on the right global id.
        let qa = FontQuery {
            family: Some("MockA".into()),
            ..Default::default()
        };
        let qb = FontQuery {
            family: Some("MockB".into()),
            ..Default::default()
        };
        assert_eq!(sys.resolve_font(&qa), Some(id_a));
        assert_eq!(sys.resolve_font(&qb), Some(id_b));

        // rasterize routes by reverse map to the correct provider — the tag
        // byte proves which provider produced the image.
        assert_eq!(sys.rasterize(key_for(id_a)).unwrap().data[0], 11);
        assert_eq!(sys.rasterize(key_for(id_b)).unwrap().data[0], 22);
    }

    // ---- Register/resolve by family + render class -----------------------

    #[test]
    fn register_and_resolve_by_family_and_render_class() {
        let mut sys = CompositeTextSystem::new();
        let p_slug = sys.add_provider(mock(TextRenderClass::Slug, "Slugger", 1));
        let id_slug = sys
            .register_face(
                p_slug,
                LocalFontId(0),
                FontSource::Bytes,
                vec!["Slugger".into()],
            )
            .unwrap();

        let q_bitmap = FontQuery {
            family: Some(BITMAP_FAMILY.into()),
            ..Default::default()
        };
        let q_slug = FontQuery {
            family: Some("Slugger".into()),
            ..Default::default()
        };
        assert_eq!(sys.resolve_font(&q_bitmap), Some(FontId::BITMAP));
        assert_eq!(sys.resolve_font(&q_slug), Some(id_slug));
        assert_eq!(
            sys.render_class(FontId::BITMAP),
            Some(TextRenderClass::Bitmap)
        );
        assert_eq!(sys.render_class(id_slug), Some(TextRenderClass::Slug));
    }

    // ---- Unknown id rejected by fallible ops -----------------------------

    #[test]
    fn unknown_id_rejected_by_fallible_raster_and_outline() {
        let sys = CompositeTextSystem::new();
        // Fabricate an unknown id by minting one from a throwaway allocator:
        // global-backed, so it is guaranteed distinct + non-bitmap, and it is
        // never registered into `sys`.
        let bogus = FontIdAllocator::new().allocate().unwrap();
        // Ensure `bogus` is genuinely unregistered: it came from a fresh
        // allocator draw and was never inserted into this registry.
        assert!(sys.render_class(bogus).is_none());

        // GlyphImage is not PartialEq, so assert on the error directly.
        assert_eq!(
            sys.rasterize(key_for(bogus)).unwrap_err(),
            TextError::UnknownFont(bogus)
        );

        let okey = OutlineKey {
            font_id: bogus,
            font_generation: 0,
            glyph_id: 1,
            variations: VariationSettings::empty(),
            synthesis_flags: 0,
            transform: Affine2Fixed::IDENTITY,
        };
        assert_eq!(
            sys.glyph_outline(&okey).unwrap_err(),
            TextError::UnknownFont(bogus)
        );

        // The infallible layout API degrades to the bitmap face rather than
        // panicking, and reports FontId::BITMAP — not the unknown id.
        let spec = LayoutSpec {
            font_id: bogus,
            ..Default::default()
        };
        let runs = sys.layout("A", &spec, None);
        assert!(!runs.is_empty());
        assert_eq!(runs[0].font_id, FontId::BITMAP);
        assert_eq!(runs[0].render_class, TextRenderClass::Bitmap);
    }

    // ---- Layout run rewrite ----------------------------------------------

    #[test]
    fn layout_run_rewritten_to_global_id_and_render_class() {
        let mut sys = CompositeTextSystem::new();
        let p = sys.add_provider(mock(TextRenderClass::Slug, "Slugger", 7));
        let id = sys
            .register_face(p, LocalFontId(0), FontSource::Bytes, vec!["Slugger".into()])
            .unwrap();

        let spec = LayoutSpec {
            font_id: id,
            ..Default::default()
        };
        let runs = sys.layout("Q", &spec, None);
        assert_eq!(runs.len(), 1);
        // The provider emitted a Bitmap lane; the composite rewrote it to the
        // global id and the Slug lane.
        assert_eq!(runs[0].font_id, id);
        assert_ne!(runs[0].font_id, FontId::BITMAP);
        assert_eq!(runs[0].render_class, TextRenderClass::Slug);
    }

    // ---- Outline routing, no trait-default fallback ----------------------

    #[test]
    fn outline_routed_through_reverse_map_not_trait_default() {
        let mut sys = CompositeTextSystem::new();
        let canned = GlyphOutline {
            units_per_em: 1000,
            ink_bounds: OutlineBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 100.0,
                max_y: 0.0,
            },
            commands: vec![
                OutlineCommand::MoveTo { x: 0.0, y: 0.0 },
                OutlineCommand::LineTo { x: 100.0, y: 0.0 },
                OutlineCommand::Close,
            ],
        };
        let p = sys.add_provider(Box::new(MockProvider {
            render_class: TextRenderClass::Outline,
            family: "Outliner".into(),
            tag: 1,
            canned_outline: Some(canned.clone()),
        }));
        let id = sys
            .register_face(
                p,
                LocalFontId(0),
                FontSource::Bytes,
                vec!["Outliner".into()],
            )
            .unwrap();

        let key = OutlineKey {
            font_id: id,
            font_generation: 0,
            glyph_id: 42,
            variations: VariationSettings::empty(),
            synthesis_flags: 0,
            transform: Affine2Fixed::IDENTITY,
        };
        // Returns the canned outline (provider override), proving the composite
        // reached the override and did NOT fall back to the
        // UnsupportedOutline default.
        assert_eq!(sys.glyph_outline(&key), Ok(canned));
    }

    // ---- Generation / reload ---------------------------------------------

    #[test]
    fn next_generation_is_checked_and_never_wraps() {
        assert_eq!(next_generation(0, FontId::BITMAP), Ok(1));
        assert_eq!(next_generation(41, FontId::BITMAP), Ok(42));
        assert_eq!(
            next_generation(u32::MAX, FontId::BITMAP),
            Err(RegistryError::GenerationOverflow(FontId::BITMAP))
        );
    }

    #[test]
    fn reload_bumps_generation_preserving_identity() {
        let mut sys = CompositeTextSystem::new();
        let p = sys.add_provider(mock(TextRenderClass::Outline, "Reloadable", 1));
        let id = sys
            .register_face(
                p,
                LocalFontId(0),
                FontSource::Bytes,
                vec!["Reloadable".into()],
            )
            .unwrap();
        assert_eq!(sys.font_generation(id), Some(0));

        assert_eq!(sys.reload_face(id, FontSource::Bytes), Ok(1));
        assert_eq!(sys.font_generation(id), Some(1));
        assert_eq!(sys.reload_face(id, FontSource::Bytes), Ok(2));
        assert_eq!(sys.font_generation(id), Some(2));

        // Public identity + render class preserved across reloads.
        assert_eq!(sys.render_class(id), Some(TextRenderClass::Outline));
        let q = FontQuery {
            family: Some("Reloadable".into()),
            ..Default::default()
        };
        assert_eq!(sys.resolve_font(&q), Some(id));
    }

    #[test]
    fn reload_at_ceiling_errors_and_leaves_mapping_intact() {
        let mut sys = CompositeTextSystem::new();
        let p = sys.add_provider(mock(TextRenderClass::Outline, "Ceiling", 1));
        let id = sys
            .register_face(p, LocalFontId(0), FontSource::Bytes, vec!["Ceiling".into()])
            .unwrap();

        // Drive the generation to the ceiling via the private registry (a
        // descendant module can reach the parent's private fields).
        sys.registry.reverse.get_mut(&id).unwrap().generation = u32::MAX;

        assert_eq!(
            sys.reload_face(id, FontSource::Builtin),
            Err(RegistryError::GenerationOverflow(id))
        );
        // Old mapping intact: generation unchanged, source not replaced.
        assert_eq!(sys.font_generation(id), Some(u32::MAX));
        assert_eq!(sys.font_source(id), Some(FontSource::Bytes));
    }

    // ---- Source vs render class are orthogonal ---------------------------

    #[test]
    fn font_source_modeled_separately_from_render_class() {
        let mut sys = CompositeTextSystem::new();
        let p = sys.add_provider(mock(TextRenderClass::Slug, "Sep", 1));
        let id = sys
            .register_face(p, LocalFontId(0), FontSource::Bytes, vec!["Sep".into()])
            .unwrap();

        // Slug render class, Bytes source — independent axes.
        assert_eq!(sys.render_class(id), Some(TextRenderClass::Slug));
        assert_eq!(sys.font_source(id), Some(FontSource::Bytes));
        // Built-in bitmap: Bitmap class + Builtin source.
        assert_eq!(
            sys.render_class(FontId::BITMAP),
            Some(TextRenderClass::Bitmap)
        );
        assert_eq!(sys.font_source(FontId::BITMAP), Some(FontSource::Builtin));
    }

    // ---- Bitmap is a first-class face ------------------------------------

    #[test]
    fn bitmap_is_font_zero_and_selectable() {
        let sys = CompositeTextSystem::new();

        assert_eq!(FontId::BITMAP.raw(), 0);
        assert_eq!(
            sys.render_class(FontId::BITMAP),
            Some(TextRenderClass::Bitmap)
        );
        assert_eq!(sys.font_source(FontId::BITMAP), Some(FontSource::Builtin));

        // Selectable by family, and the default query also lands on bitmap.
        let q = FontQuery {
            family: Some(BITMAP_FAMILY.into()),
            ..Default::default()
        };
        assert_eq!(sys.resolve_font(&q), Some(FontId::BITMAP));
        assert_eq!(
            sys.resolve_font(&FontQuery::default()),
            Some(FontId::BITMAP)
        );

        // Bitmap layout + raster still flow end-to-end through the composite.
        let spec = LayoutSpec::default();
        let runs = sys.layout("A", &spec, None);
        assert_eq!(runs[0].font_id, FontId::BITMAP);
        assert_eq!(runs[0].render_class, TextRenderClass::Bitmap);
        let img = sys.rasterize(cache_key_for('A', 10.0)).unwrap();
        assert_eq!(img.width_px, 5);
        assert_eq!(img.height_px, 7);
    }

    #[test]
    fn families_union_includes_bitmap_and_registered() {
        let mut sys = CompositeTextSystem::new();
        let p = sys.add_provider(mock(TextRenderClass::Slug, "Extra", 1));
        sys.register_face(p, LocalFontId(0), FontSource::Bytes, vec!["Extra".into()])
            .unwrap();

        let families = sys.families();
        assert!(families.contains(&BITMAP_FAMILY.to_string()));
        assert!(families.contains(&"Extra".to_string()));
    }
}
