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
//! and hands the provider only its private `LocalFontId` — it never
//! reconstructs a `FontId` from a provider-local value. Provider-local face
//! IDs (`LocalFontId`) are crate-private and never leave this boundary, so
//! two providers may both use `LocalFontId(0)` without colliding.
//!
//! Provider-produced [`LayoutRun`]s carry a `RoutedRun` discriminator: a
//! `Primary` run adopts the selected face's `FontId` / registry generation /
//! [`TextRenderClass`], while a `Fallback` run keeps its provider-validated
//! fallback target. This preserves mixed SFNT/bitmap layout (the #67 contract)
//! instead of flattening every run onto the selected face.
//!
//! ## Generic-only boundary (#62)
//!
//! This module ships the registry/router and the crate-private provider
//! contract only. No concrete SFNT parser, TTF byte handling, Slug encoder, or
//! WGPU type lands here — the outline provider (`SfntTextSystem`) registers in
//! #67.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::bitmap::{
    bitmap_scale, normalize_bitmap_char, BitmapTextSystem, BITMAP_FAMILY, GLYPH_ADVANCE_PX,
    GLYPH_CELL_HEIGHT_PX,
};
use crate::font_id::{FontId, FontIdAllocator};
use crate::outline::{GlyphOutline, OutlineKey};
use crate::sfnt::{apply_transform, SfntError, SfntFace};
use crate::system::{
    FontQuery, GlyphCacheKey, GlyphFormat, GlyphImage, LayoutGlyph, LayoutRun, LayoutSpec,
    TextError, TextRenderClass, TextSystem,
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

/// Non-forgeable proof that a fallback target is a registered face.
///
/// The registry is the single source of truth for every `(FontId,
/// TextRenderClass)` binding, so a fallback route must not let a provider name
/// an unregistered face or claim a render class that contradicts the registry.
/// This module enforces both with the type system: [`ValidatedFallback`]'s
/// field is private, so the **only** way to obtain one is [`validate`] — whose
/// only caller is the registry ([`CompositeTextSystem::validated_fallback`]).
/// The token carries just the target id; render class and generation are
/// derived from the target's `FaceRecord` at dispatch, never supplied by the
/// provider.
mod fallback {
    use super::RegistryError;
    use crate::font_id::FontId;

    /// A fallback target proven registered — mintable only via [`validate`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct ValidatedFallback(FontId);

    impl ValidatedFallback {
        /// The validated target id — always a registered face.
        pub(crate) fn target(self) -> FontId {
            self.0
        }
    }

    /// Validate `target` against `is_registered` and mint a
    /// [`ValidatedFallback`], or reject an unknown target with
    /// [`RegistryError::UnknownFont`]. The private field makes this the only
    /// construction path; providers and tests cannot bypass it.
    #[allow(dead_code)]
    pub(crate) fn validate(
        target: FontId,
        is_registered: impl FnOnce(FontId) -> bool,
    ) -> Result<ValidatedFallback, RegistryError> {
        if is_registered(target) {
            Ok(ValidatedFallback(target))
        } else {
            Err(RegistryError::UnknownFont(target))
        }
    }
}

pub(crate) use fallback::ValidatedFallback;

/// How the router should label one provider-emitted [`LayoutRun`].
///
/// A provider that performs **layout-time fallback** — e.g. #67's SFNT face
/// emitting bitmap runs for glyphs it lacks (emoji, CJK supplementary, missing
/// ligatures) — classifies each run at the provider boundary and tags it so
/// the router preserves the fallback identity instead of relabeling every run
/// with the primary selected face. Crate-private: never part of the public
/// surface.
///
/// `Fallback` has no shipping constructor in #62 (the only bundled provider is
/// the single-lane bitmap adapter, which is always `Primary`); the outline
/// provider in #67 constructs it, and the mixed-fallback tests here exercise
/// it. The router already honors it — see `CompositeTextSystem::layout`.
#[allow(dead_code)]
pub(crate) enum RoutedRun {
    /// Render via the provider's primary (selected) face. The router stamps the
    /// selected `FontId`, its render class, and its registry generation.
    Primary,
    /// Render via a registry-validated cross-provider fallback. The target is a
    /// [`ValidatedFallback`] (mintable only by the registry), so the route is
    /// guaranteed valid by construction. The router derives the run's render
    /// class and generation from the target's `FaceRecord` — the registry is
    /// authoritative; the provider supplies no render class here and so can
    /// never contradict it.
    Fallback(ValidatedFallback),
}

/// A provider-emitted run plus its [`RoutedRun`] discriminator. The composite
/// consumes these and returns plain public [`LayoutRun`]s with the routing
/// applied. Crate-private wrapper so the public `LayoutRun` (owned by #61's
/// `system.rs`) is not touched.
pub(crate) struct RoutedLayoutRun {
    pub(crate) routing: RoutedRun,
    pub(crate) run: LayoutRun,
}

/// Crate-private adapter every backend implements so the composite can route
/// to it. Distinct from the public [`TextSystem`] trait: its methods are keyed
/// by the provider's own `LocalFontId`, and the composite is the only caller.
///
/// "The provider-local adapter may be crate-private" (#62) — this trait, plus
/// `LocalFontId` / `ProviderId`, are exactly that boundary. Public APIs
/// continue to expose only [`FontId`]; no `GlobalFaceId` type is introduced. A
/// provider dispatches only on its own `LocalFontId`; the one public `FontId` a
/// provider ever names is a [`RoutedRun::Fallback`] target — a routing hint the
/// router re-validates, not an id the provider dispatches on.
pub(crate) trait FontProvider: Send + Sync + 'static {
    /// The render lane every face this provider exposes draws through.
    fn render_class(&self) -> TextRenderClass;

    /// Human-readable family names this provider exposes.
    fn families(&self) -> Vec<String>;

    /// Resolve a query to one of this provider's local faces, or `None` to
    /// decline (so a sibling provider can claim the query).
    fn resolve_local(&self, query: &FontQuery) -> Option<LocalFontId>;

    /// Lay out `text` for provider-local face `local`, returning each run
    /// tagged with a [`RoutedRun`] discriminator. The provider keys on `local`,
    /// **not** on `spec.font_id` (which still holds the public id).
    ///
    /// A run tagged [`RoutedRun::Primary`] is relabeled by the router to the
    /// selected face; a run tagged [`RoutedRun::Fallback`] keeps its
    /// provider-validated fallback identity. This is what makes #67's
    /// mixed SFNT/bitmap-fallback layout work: the provider — not the router —
    /// decides per run which face renders it.
    fn layout_local(
        &self,
        local: LocalFontId,
        text: &str,
        spec: &LayoutSpec,
        max_width_px: Option<f32>,
    ) -> Vec<RoutedLayoutRun>;

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

    /// Borrow this provider's decoded [`SfntFace`], if it is the SFNT provider.
    /// The default is `None` (bitmap / mock providers have no SFNT face); the
    /// SFNT provider overrides it so the registry can expose the decoded face
    /// for inspection without re-parsing.
    fn as_sfnt_face(&self) -> Option<&SfntFace> {
        None
    }
}

/// Public error from [`CompositeTextSystem::register_sfnt_face`].
///
/// Keeps the crate-private [`RegistryError`] off the public surface: the only
/// registry failure reachable during SFNT registration is allocator exhaustion
/// (the bitmap fallback target is always registered and generation is never
/// bumped here), surfaced as [`FontIdExhausted`](Self::FontIdExhausted).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SfntRegisterError {
    /// The narrow SFNT decoder rejected the bytes with a typed reason.
    #[error("SFNT decode failed: {0}")]
    Decode(#[from] SfntError),
    /// The shared `1..=u64::MAX` [`FontId`] space is exhausted.
    #[error("font id space exhausted while registering SFNT face")]
    FontIdExhausted,
}

/// Map a narrow-decoder [`SfntError`] onto the public [`TextError`] surface for
/// the `glyph_outline` / `rasterize` trait paths.
fn sfnt_to_text_error(err: SfntError) -> TextError {
    match err {
        SfntError::GlyphOutOfRange(gid) => TextError::UnknownGlyph(gid as u32),
        // Composite / unsupported / malformed glyph data: this face cannot
        // produce that outline. UnknownFont/Unsupported* are setup-time concerns
        // already handled at registration, so the per-glyph surface collapses to
        // "no outline available".
        _ => TextError::UnsupportedOutline,
    }
}

/// Crate-private provider adapter wrapping a decoded [`SfntFace`] behind the
/// [`FontProvider`] contract, in the [`TextRenderClass::Slug`] lane.
///
/// Exposes one local face, `LocalFontId(0)`. Its [`layout_local`](FontProvider::layout_local)
/// performs **layout-time fallback segmentation**: consecutive characters the
/// face maps stay in a `Primary` Slug run; consecutive characters it lacks are
/// emitted as an adjacent `Fallback` run targeting the registry-validated
/// bitmap face, with cluster order, advances, and absolute positions preserved.
struct SfntProvider {
    face: Arc<SfntFace>,
    fallback: ValidatedFallback,
}

impl SfntProvider {
    fn new(face: Arc<SfntFace>, fallback: ValidatedFallback) -> Self {
        Self { face, fallback }
    }
}

/// One run-in-progress while segmenting a line into Slug + fallback runs.
struct PendingRun {
    routing: RoutedRun,
    render_class: TextRenderClass,
    origin_x_px: f32,
    line_ascent_px: f32,
    line_descent_px: f32,
    line_y_baseline_px: f32,
    glyphs: Vec<LayoutGlyph>,
}

impl FontProvider for SfntProvider {
    fn render_class(&self) -> TextRenderClass {
        TextRenderClass::Slug
    }

    fn families(&self) -> Vec<String> {
        self.face
            .family_name()
            .map(|n| vec![n.to_string()])
            .unwrap_or_default()
    }

    fn resolve_local(&self, query: &FontQuery) -> Option<LocalFontId> {
        match (&query.family, self.face.family_name()) {
            (Some(requested), Some(face_family)) if requested == face_family => {
                Some(LocalFontId(0))
            }
            _ => None,
        }
    }

    fn layout_local(
        &self,
        _local: LocalFontId,
        text: &str,
        spec: &LayoutSpec,
        _max_width_px: Option<f32>,
    ) -> Vec<RoutedLayoutRun> {
        let upem = self.face.units_per_em() as f32;
        let sfnt_scale = spec.font_size_px / upem;
        let metrics = self.face.metrics();
        let sfnt_ascent = metrics.ascender as f32 * sfnt_scale;
        let sfnt_descent = -(metrics.descender as f32) * sfnt_scale;
        // The shared baseline both lanes sit on, measured from the line top.
        let baseline = sfnt_ascent;

        let bmp_scale = bitmap_scale(spec.font_size_px);
        let bmp_advance = GLYPH_ADVANCE_PX * bmp_scale;
        let bmp_height = GLYPH_CELL_HEIGHT_PX * bmp_scale;

        let mut runs: Vec<PendingRun> = Vec::new();
        let mut current: Option<PendingRun> = None;
        let mut pen_x = 0.0f32;

        for (cluster, ch) in text.chars().enumerate() {
            // Classify the character: mapped → Slug lane, unmapped → bitmap
            // fallback. The boundary is decided here, at layout time, never in
            // the renderer.
            let (is_slug, glyph_id, advance) = match self.face.glyph_index(ch) {
                Some(gid) => (
                    true,
                    gid as u32,
                    self.face.advance_width(gid) as f32 * sfnt_scale,
                ),
                None => (false, normalize_bitmap_char(ch) as u32, bmp_advance),
            };

            let want_class = if is_slug {
                TextRenderClass::Slug
            } else {
                TextRenderClass::Bitmap
            };

            // Start a new run at every supported/unsupported boundary.
            let needs_new = current
                .as_ref()
                .is_none_or(|r| r.render_class != want_class);
            if needs_new {
                if let Some(done) = current.take() {
                    runs.push(done);
                }
                let (line_ascent_px, line_descent_px) = if is_slug {
                    (sfnt_ascent, sfnt_descent)
                } else {
                    (bmp_height, 0.0)
                };
                current = Some(PendingRun {
                    routing: if is_slug {
                        RoutedRun::Primary
                    } else {
                        RoutedRun::Fallback(self.fallback)
                    },
                    render_class: want_class,
                    origin_x_px: pen_x,
                    line_ascent_px,
                    line_descent_px,
                    line_y_baseline_px: baseline,
                    glyphs: Vec::new(),
                });
            }

            let run = current.as_mut().expect("a run was just ensured");
            run.glyphs.push(LayoutGlyph {
                glyph_id,
                // Absolute x within the parent run = pen − run origin.
                x_px: pen_x - run.origin_x_px,
                y_px: 0.0,
                x_advance_px: advance,
                x_offset_px: 0.0,
                y_offset_px: 0.0,
                cluster: cluster as u32,
                subpixel_variant: 0,
                format: GlyphFormat::Alpha,
            });
            pen_x += advance;
        }
        if let Some(done) = current.take() {
            runs.push(done);
        }

        // Materialize each pending run into a public LayoutRun. The composite
        // overwrites font_id / render_class / generation per the routing
        // discriminator, so the values stamped here for those three fields are
        // placeholders.
        runs.into_iter()
            .map(|pending| {
                let bitmap_top_origin = (baseline - pending.line_ascent_px).max(0.0);
                RoutedLayoutRun {
                    routing: pending.routing,
                    run: LayoutRun {
                        font_id: spec.font_id,
                        render_class: pending.render_class,
                        font_generation: spec.font_generation,
                        font_size_px: spec.font_size_px,
                        variations: spec.variations.clone(),
                        synthesis_flags: spec.synthesis_flags,
                        hinting: spec.hinting,
                        origin_x_px: pending.origin_x_px,
                        // Slug runs are baseline-relative (origin at top = 0);
                        // bitmap fallback runs draw from a top origin placed so
                        // their cell baseline lands on the shared baseline.
                        origin_y_px: if pending.render_class == TextRenderClass::Bitmap {
                            bitmap_top_origin
                        } else {
                            0.0
                        },
                        line_y_baseline_px: pending.line_y_baseline_px,
                        line_ascent_px: pending.line_ascent_px,
                        line_descent_px: pending.line_descent_px,
                        glyphs: pending.glyphs,
                    },
                }
            })
            .collect()
    }

    fn rasterize_local(
        &self,
        _local: LocalFontId,
        _key: &GlyphCacheKey,
    ) -> Result<GlyphImage, TextError> {
        // A Slug/outline face has no CPU raster; its glyphs flow through
        // `outline_local` + the Slug encoder. Fallback glyphs are bitmap-face
        // glyphs and never route here.
        Err(TextError::UnsupportedRaster)
    }

    fn outline_local(
        &self,
        _local: LocalFontId,
        key: &OutlineKey,
    ) -> Result<GlyphOutline, TextError> {
        let gid = u16::try_from(key.glyph_id).map_err(|_| TextError::UnknownGlyph(key.glyph_id))?;
        let mut outline = self.face.glyph_outline(gid).map_err(sfnt_to_text_error)?;
        // The provider resolves the outline-local affine exactly once, so the
        // returned ink bounds always match the returned points (#61 contract).
        apply_transform(&mut outline, key.transform);
        Ok(outline)
    }

    fn as_sfnt_face(&self) -> Option<&SfntFace> {
        Some(&self.face)
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

    /// Validate that `target` is a registered face and mint a
    /// [`ValidatedFallback`] a provider can emit as a [`RoutedRun::Fallback`].
    ///
    /// This is the **setup-time** validation boundary: an unknown target is
    /// rejected here with [`RegistryError::UnknownFont`], loudly, rather than
    /// being silently coerced to bitmap at dispatch. Because the token is the
    /// only way to construct a `Fallback` and it cannot be forged, an
    /// unregistered fallback route is unrepresentable downstream.
    #[allow(dead_code)]
    pub(crate) fn validated_fallback(
        &self,
        target: FontId,
    ) -> Result<ValidatedFallback, RegistryError> {
        fallback::validate(target, |fid| self.registry.reverse.contains_key(&fid))
    }

    /// Decode an SFNT/TrueType face from `bytes` (face `index`) and register it
    /// as a [`TextRenderClass::Slug`] provider beside the built-in bitmap face,
    /// minting a fresh public [`FontId`] from the shared allocator.
    ///
    /// This is #67's concrete provider entry point: it owns the narrow
    /// from-scratch [`SfntFace`] decode, wires the provider to a
    /// registry-validated bitmap fallback (so glyphs the face lacks split into
    /// `FontId::BITMAP` runs at layout time), and indexes the decoded family
    /// name. The returned [`FontId`] is the only identity that escapes — the
    /// provider-local face id never leaves the registry boundary.
    pub fn register_sfnt_face(
        &mut self,
        bytes: Arc<[u8]>,
        index: u32,
    ) -> Result<FontId, SfntRegisterError> {
        let face = SfntFace::parse(bytes, index)?;
        let families = face
            .family_name()
            .map(|name| vec![name.to_string()])
            .unwrap_or_default();

        // The reserved bitmap face is always registered, so this fallback token
        // can never be rejected — but route it through the same validation
        // boundary every provider uses, so the registry stays authoritative.
        let fallback = self
            .validated_fallback(FontId::BITMAP)
            .map_err(|_| SfntRegisterError::FontIdExhausted)?;

        let provider = SfntProvider::new(Arc::new(face), fallback);
        let provider_id = self.add_provider(Box::new(provider));
        self.register_face(provider_id, LocalFontId(0), FontSource::Bytes, families)
            .map_err(|_| SfntRegisterError::FontIdExhausted)
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
    /// downstream cache keys. Bumped by `reload_face` on reload. `None` if
    /// `font_id` is unknown.
    pub fn font_generation(&self, font_id: FontId) -> Option<u32> {
        self.registry.reverse.get(&font_id).map(|r| r.generation)
    }

    /// Generation of the reserved bitmap face, used when an unknown id degrades
    /// to bitmap in the infallible layout path. `0` if somehow unregistered
    /// (the seed always registers it).
    fn bitmap_generation(&self) -> u32 {
        self.registry
            .reverse
            .get(&FontId::BITMAP)
            .map_or(0, |r| r.generation)
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
        let (selected_id, provider_id, local, selected_generation, selected_class) =
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
                    self.bitmap_generation(),
                    TextRenderClass::Bitmap,
                ),
            };

        // Hand the provider its private local id; the spec still carries the
        // public id, which the provider ignores for identity.
        let routed = self.providers[provider_id.0].layout_local(local, text, spec, max_width_px);

        // Apply each run's routing discriminator. `Primary` runs adopt the
        // selected face; `Fallback` runs KEEP their target so a mixed
        // SFNT/bitmap layout (the #67 contract) is not flattened onto a single
        // face. For BOTH arms every published `(FontId, TextRenderClass,
        // generation)` tuple is derived from the registry's `FaceRecord` — the
        // single source of truth — never from provider-supplied values. The
        // router never invents a FontId.
        routed
            .into_iter()
            .map(|RoutedLayoutRun { routing, mut run }| {
                let (font_id, render_class, font_generation) = match routing {
                    RoutedRun::Primary => (selected_id, selected_class, selected_generation),
                    RoutedRun::Fallback(target) => {
                        // The token is non-forgeable proof of registration, so
                        // the lookup cannot miss (the registry has no unregister
                        // path). Render class + generation come from the
                        // FaceRecord — authoritative — not from the provider.
                        let target_id = target.target();
                        let rec = self
                            .registry
                            .reverse
                            .get(&target_id)
                            .expect("ValidatedFallback guarantees the target is registered");
                        (target_id, rec.render_class, rec.generation)
                    }
                };
                run.font_id = font_id;
                run.render_class = render_class;
                run.font_generation = font_generation;
                run
            })
            .collect()
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

    /// Resolved face metrics + decoded glyph count for a registered SFNT face,
    /// or `None` if `font_id` is unknown or not an SFNT provider. Lets a caller
    /// inspect the decoded face (units-per-em, glyph count, family) without
    /// re-parsing the bytes.
    fn sfnt_face(&self, font_id: FontId) -> Option<&SfntFace> {
        let record = self.registry.reverse.get(&font_id)?;
        self.providers[record.provider_id.0].as_sfnt_face()
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
    ) -> Vec<RoutedLayoutRun> {
        // The bitmap face has exactly one lane, so every run is Primary.
        self.inner
            .layout(text, spec, max_width_px)
            .into_iter()
            .map(|run| RoutedLayoutRun {
                routing: RoutedRun::Primary,
                run,
            })
            .collect()
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

    /// Build a single-glyph [`LayoutRun`] for `glyph_id` under `spec`. Shared by
    /// the mock providers; the composite overwrites identity fields per routing.
    fn run_for(glyph_id: u32, spec: &LayoutSpec) -> LayoutRun {
        LayoutRun {
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
                glyph_id,
                x_px: 0.0,
                y_px: 0.0,
                x_advance_px: 0.0,
                x_offset_px: 0.0,
                y_offset_px: 0.0,
                cluster: 0,
                subpixel_variant: 0,
                format: GlyphFormat::Alpha,
            }],
        }
    }

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
        ) -> Vec<RoutedLayoutRun> {
            // Emit one Primary run with a deliberately wrong render class; the
            // composite must overwrite the identity fields to the selected face.
            let glyph_id = text.chars().next().map_or(0, |c| c as u32);
            vec![RoutedLayoutRun {
                routing: RoutedRun::Primary,
                run: run_for(glyph_id, spec),
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

    /// A provider that performs **layout-time fallback**, like #67's SFNT face:
    /// it renders most glyphs through its own (primary) lane but emits a
    /// cross-provider fallback run for `fallback_char`. It holds a
    /// registry-issued [`ValidatedFallback`] token — it cannot name an
    /// unregistered face and supplies no render class of its own.
    struct MixedProvider {
        primary_class: TextRenderClass,
        family: String,
        fallback_char: char,
        fallback: ValidatedFallback,
    }

    impl FontProvider for MixedProvider {
        fn render_class(&self) -> TextRenderClass {
            self.primary_class
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
        ) -> Vec<RoutedLayoutRun> {
            // One run per char: the unsupported `fallback_char` routes to the
            // validated fallback token; every other char stays Primary.
            text.chars()
                .map(|ch| {
                    let routing = if ch == self.fallback_char {
                        RoutedRun::Fallback(self.fallback)
                    } else {
                        RoutedRun::Primary
                    };
                    RoutedLayoutRun {
                        routing,
                        run: run_for(ch as u32, spec),
                    }
                })
                .collect()
        }

        fn rasterize_local(
            &self,
            _local: LocalFontId,
            _key: &GlyphCacheKey,
        ) -> Result<GlyphImage, TextError> {
            Err(TextError::UnknownGlyph(0))
        }
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

    // ---- Mixed-fallback runs preserved (the #67 contract) ----------------

    #[test]
    fn mixed_fallback_runs_are_preserved_not_relabeled() {
        // Provider B owns the bitmap-class fallback face; provider A (Outline)
        // performs layout-time fallback to it for the glyph it lacks ('🌟').
        let mut sys = CompositeTextSystem::new();

        let pb = sys.add_provider(mock(TextRenderClass::Bitmap, "FallbackFace", 9));
        let font_b = sys
            .register_face(
                pb,
                LocalFontId(0),
                FontSource::Bytes,
                vec!["FallbackFace".into()],
            )
            .unwrap();

        // Provider A is configured with a registry-issued, validated fallback
        // token — it cannot forge or name an unregistered face.
        let token = sys.validated_fallback(font_b).unwrap();
        let pa = sys.add_provider(Box::new(MixedProvider {
            primary_class: TextRenderClass::Outline,
            family: "Primary".into(),
            fallback_char: '🌟',
            fallback: token,
        }));
        let font_a = sys
            .register_face(
                pa,
                LocalFontId(0),
                FontSource::Bytes,
                vec!["Primary".into()],
            )
            .unwrap();

        let spec = LayoutSpec {
            font_id: font_a,
            ..Default::default()
        };
        let runs = sys.layout("A🌟C", &spec, None);
        assert_eq!(runs.len(), 3);

        // Primary runs adopt the selected (Outline) face...
        assert_eq!(runs[0].font_id, font_a);
        assert_eq!(runs[0].render_class, TextRenderClass::Outline);
        assert_eq!(runs[2].font_id, font_a);
        assert_eq!(runs[2].render_class, TextRenderClass::Outline);

        // ...but the fallback run KEEPS the validated bitmap target, instead of
        // being relabeled to font_a/Outline (which would break #67's contract).
        assert_eq!(runs[1].font_id, font_b);
        assert_eq!(runs[1].render_class, TextRenderClass::Bitmap);
        assert_ne!(runs[1].font_id, font_a);
    }

    #[test]
    fn fallback_to_unregistered_target_is_rejected_at_setup() {
        // The registry refuses to mint a fallback token for an unregistered
        // FontId — loudly, at setup — rather than silently coercing to bitmap
        // later. Because the token is the only way to build a Fallback route,
        // an unregistered fallback is unrepresentable downstream.
        let sys = CompositeTextSystem::new();
        let bogus = FontIdAllocator::new().allocate().unwrap();
        assert_eq!(
            sys.validated_fallback(bogus),
            Err(RegistryError::UnknownFont(bogus))
        );
    }

    #[test]
    fn fallback_render_class_is_derived_from_registry_not_provider() {
        // The fallback face is registered as Msdf; the primary is Outline. The
        // provider carries only a validated token — it cannot supply a render
        // class — so the fallback run's class is read from the registry's
        // FaceRecord (Msdf), proving the registry is authoritative and the
        // router applies no bitmap default.
        let mut sys = CompositeTextSystem::new();
        let pb = sys.add_provider(mock(TextRenderClass::Msdf, "Fb", 5));
        let font_b = sys
            .register_face(pb, LocalFontId(0), FontSource::Bytes, vec!["Fb".into()])
            .unwrap();
        let token = sys.validated_fallback(font_b).unwrap();
        let pa = sys.add_provider(Box::new(MixedProvider {
            primary_class: TextRenderClass::Outline,
            family: "Pri".into(),
            fallback_char: '*',
            fallback: token,
        }));
        let font_a = sys
            .register_face(pa, LocalFontId(0), FontSource::Bytes, vec!["Pri".into()])
            .unwrap();

        let spec = LayoutSpec {
            font_id: font_a,
            ..Default::default()
        };
        let runs = sys.layout("A*", &spec, None);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].font_id, font_a);
        assert_eq!(runs[0].render_class, TextRenderClass::Outline);
        // Fallback run: identity AND render class come from font_b's record.
        assert_eq!(runs[1].font_id, font_b);
        assert_eq!(runs[1].render_class, TextRenderClass::Msdf);
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
