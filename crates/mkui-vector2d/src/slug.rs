//! Deterministic CPU Slug glyph encoder: outlines → curve + band records.
//!
//! This module turns a backend-neutral [`VectorPath`] of line/quadratic glyph
//! geometry into a [`SlugGlyph`] blob: a flat list of quadratic [`SlugCurve`]
//! records plus horizontal and vertical [`BandRange`] tables that index into
//! curve-index streams. It is the size-independent *outline* cache — distinct
//! from `mkui-text`'s size-dependent bitmap raster cache (see crate docs).
//!
//! # Coordinate convention
//!
//! All coordinates stay in **font units, y-up**, exactly as supplied by the
//! resolved outline. Screen-space y-down conversion and GPU packing are #66's
//! responsibility; this encoder never sees a pixel size or a WGPU type.
//!
//! # Attribution (Sprint 7 plan §3.4)
//!
//! The curve/band encoding reproduces the contract of the **Slug** algorithm
//! by Eric Lengyel (<https://github.com/EricLengyel/Slug>). Lengyel published
//! the Slug reference encoding into the public domain. This is a from-scratch
//! CPU encoder that honours the documented record/band invariants — it copies
//! no Slug source.
//!
//! # Record layout (versioned via [`SlugConfig::revision`])
//!
//! - [`SlugGlyph::curves`]: every segment as a quadratic `(p0, p1, p2)`. A
//!   straight line uses the Slug **duplicated-endpoint sentinel** `p1 == p2`
//!   (never the midpoint), so the shader can branch on `p1 == p2` to take its
//!   line path instead of evaluating a quadratic.
//! - [`SlugGlyph::horizontal_bands`]: the glyph's y-range split into
//!   `SlugConfig::horizontal_bands` equal rows. A row lists every curve whose
//!   y-extent overlaps it **and** that is not axis-parallel to the scan
//!   direction — segments with `|dy| < ε` (horizontal/near-horizontal) are
//!   excluded so they cannot corrupt the horizontal-ray winding count. Members
//!   are referenced via `[first_curve, first_curve + curve_count)` into
//!   [`SlugGlyph::horizontal_curve_indices`].
//! - [`SlugGlyph::vertical_bands`] / [`SlugGlyph::vertical_curve_indices`]: the
//!   same, for the x-range split into columns, excluding `|dx| < ε`
//!   (vertical/near-vertical) segments.
//!
//! Within each band the members are ordered by the **cross-axis curve
//! extremum**, descending, as Slug's scanline rasterizer requires: horizontal
//! rows sort by descending maximum **x** (rightmost-extending first), vertical
//! columns by descending maximum **y** (topmost-extending first). The extremum
//! is the true curve maximum over `t ∈ [0, 1]` — for a quadratic that includes
//! the interior critical point, not just the endpoints. Exact ties break by
//! ascending curve index for determinism. The encoding is a pure function of
//! `(path, config)`, so re-encoding is byte-identical.
//!
//! # What #66 may and may not do
//!
//! #66 may serialize these records (see [`SlugGlyph::to_le_bytes`]) and pack
//! them into GPU buffers. It may **not** reimplement or reinterpret the band
//! algorithm — the band membership and ordering produced here are the contract.

use std::collections::HashMap;
use std::sync::Arc;

use crate::path::{PathCommand, Vec2, VectorPath};
use mkui_text::{Affine2Fixed, FontId, LayoutRun, VariationSettings};

/// Immutable encoder configuration. Held by the owning [`SlugBlobCache`]; it is
/// **not** part of [`SlugGlyphKey`] so two caches with different configs form
/// separate namespaces and cannot alias each other's blobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlugConfig {
    horizontal_bands: u16,
    vertical_bands: u16,
    revision: u32,
    /// `units_per_em`'s bit pattern (`f32::to_bits`), so the config keeps
    /// deriving `Eq`/`Hash` (`f32` implements neither) — no downstream
    /// map/set usage of `SlugConfig` loses those impls.
    units_per_em_bits: u32,
}

impl SlugConfig {
    /// Construct a config. Band counts are clamped to at least 1 (a glyph
    /// always needs one row and one column). `units_per_em` defaults to `1.0`
    /// (the path's own coordinate units carry the em) — callers encoding real
    /// font outlines should set the face's actual units-per-em via
    /// [`Self::with_units_per_em`] so the band overlap epsilon (#157 Phase 2
    /// step 5) normalizes correctly to that font's design-unit scale.
    pub fn new(horizontal_bands: u16, vertical_bands: u16, revision: u32) -> Self {
        Self {
            horizontal_bands: horizontal_bands.max(1),
            vertical_bands: vertical_bands.max(1),
            revision,
            units_per_em_bits: 1.0_f32.to_bits(),
        }
    }

    /// Set the font's units-per-em (or the path's own em scale, for non-font
    /// geometry). Clamped to a small positive floor so the derived band
    /// overlap epsilon is never zero or negative.
    pub fn with_units_per_em(mut self, units_per_em: f32) -> Self {
        self.units_per_em_bits = units_per_em.max(f32::EPSILON).to_bits();
        self
    }

    pub fn horizontal_bands(self) -> u16 {
        self.horizontal_bands
    }

    pub fn vertical_bands(self) -> u16 {
        self.vertical_bands
    }

    /// Encoder revision stamped into every [`SlugGlyph`]. Bump when the record
    /// layout or band algorithm changes so #66 can reject stale blobs.
    pub fn revision(self) -> u32 {
        self.revision
    }

    /// The em scale band overlap is normalized against. Defaults to `1.0`.
    pub fn units_per_em(self) -> f32 {
        f32::from_bits(self.units_per_em_bits)
    }
}

/// Collision-free outline-cache key built from canonical full values.
///
/// Every field is a real value, never a pre-hashed surrogate, so equality and
/// hashing are exact. Pixel size, hinting, subpixel position, scene/layout
/// placement, and output format are deliberately **excluded** — the encoded
/// outline blob is size-independent. `outline_transform` carries only
/// outline-local synthesis/normalization, never the glyph quad position.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SlugGlyphKey {
    pub font_id: FontId,
    pub font_generation: u32,
    pub glyph_id: u32,
    pub variation_axes: VariationSettings,
    pub synthesis_flags: u32,
    pub outline_transform: Affine2Fixed,
}

impl SlugGlyphKey {
    /// Propagate a Slug run's canonical identity into an outline-cache key.
    ///
    /// This is #67's #65-side propagation seam: a [`TextRenderClass::Slug`](mkui_text::TextRenderClass)
    /// [`LayoutRun`] carries the registry-owned global [`FontId`], generation,
    /// and canonical variation coordinate; combined with a `glyph_id` and the
    /// outline-local `outline_transform` (canonical, **not** scene placement)
    /// they form the size-independent [`SlugGlyphKey`]. Pixel size, hinting, and
    /// layout position are deliberately excluded — one blob serves every size.
    pub fn from_run(run: &LayoutRun, glyph_id: u32, outline_transform: Affine2Fixed) -> Self {
        Self {
            font_id: run.font_id,
            font_generation: run.font_generation,
            glyph_id,
            variation_axes: run.variations.clone(),
            synthesis_flags: run.synthesis_flags,
            outline_transform,
        }
    }
}

/// A single quadratic curve record `(p0, p1, p2)` in font units, y-up.
///
/// A straight line segment is stored with the Slug **duplicated-endpoint
/// sentinel**: `p1 == p2`. The Slug shader keys on `p1 == p2` to take its line
/// branch instead of evaluating a quadratic, so the control point must be the
/// terminal endpoint — never the midpoint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlugCurve {
    pub p0: Vec2,
    pub p1: Vec2,
    pub p2: Vec2,
}

impl SlugCurve {
    pub(crate) fn line(p0: Vec2, p2: Vec2) -> Self {
        // Duplicated-endpoint sentinel (control == terminal endpoint): the Slug
        // shader distinguishes a line from a real quadratic by `p1 == p2`.
        Self { p0, p1: p2, p2 }
    }

    pub(crate) fn quad(p0: Vec2, p1: Vec2, p2: Vec2) -> Self {
        Self { p0, p1, p2 }
    }

    pub(crate) fn y_extent(&self) -> (f32, f32) {
        let lo = self.p0.y.min(self.p1.y).min(self.p2.y);
        let hi = self.p0.y.max(self.p1.y).max(self.p2.y);
        (lo, hi)
    }

    pub(crate) fn x_extent(&self) -> (f32, f32) {
        let lo = self.p0.x.min(self.p1.x).min(self.p2.x);
        let hi = self.p0.x.max(self.p1.x).max(self.p2.x);
        (lo, hi)
    }

    /// True maximum x-coordinate of the curve over `t ∈ [0, 1]` (the larger
    /// endpoint plus the interior critical point if one lies inside the span).
    /// This is the horizontal-band sort key the Slug contract requires.
    pub(crate) fn curve_extrema_max_x(&self) -> f32 {
        quadratic_extremum_max(self.p0.x, self.p1.x, self.p2.x)
    }

    /// True maximum y-coordinate of the curve over `t ∈ [0, 1]` — the
    /// vertical-band sort key the Slug contract requires.
    pub(crate) fn curve_extrema_max_y(&self) -> f32 {
        quadratic_extremum_max(self.p0.y, self.p1.y, self.p2.y)
    }
}

/// Maximum of one component of a quadratic Bézier over `t ∈ [0, 1]`.
///
/// `B(t) = (1-t)²·a0 + 2(1-t)t·a1 + t²·a2`. The derivative vanishes at
/// `t* = (a0 - a1) / (a0 - 2·a1 + a2)`; when `t*` falls strictly inside
/// `(0, 1)` the interior value `B(t*)` is a candidate extremum, otherwise the
/// maximum is at an endpoint. For a degenerate (line) component the denominator
/// is ~0 and only the endpoints matter. We never sample at a fixed `t`.
fn quadratic_extremum_max(a0: f32, a1: f32, a2: f32) -> f32 {
    let mut max = a0.max(a2);
    let denom = a0 - 2.0 * a1 + a2;
    if denom.abs() > f32::EPSILON {
        let t = (a0 - a1) / denom;
        if t > 0.0 && t < 1.0 {
            let mt = 1.0 - t;
            let b = mt * mt * a0 + 2.0 * mt * t * a1 + t * t * a2;
            max = max.max(b);
        }
    }
    max
}

/// Glyph ink bounds carried alongside the records, font units y-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlyphBounds {
    pub x_min: f32,
    pub y_min: f32,
    pub x_max: f32,
    pub y_max: f32,
}

/// One band's coordinate range plus its slice into a curve-index stream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandRange {
    /// Inclusive lower edge of the band along its axis (y for horizontal bands,
    /// x for vertical bands).
    pub lower: f32,
    /// Upper edge of the band along its axis.
    pub upper: f32,
    /// Offset of this band's first curve index in the stream.
    pub first_curve: u32,
    /// Number of curve indices belonging to this band.
    pub curve_count: u32,
}

/// The encoded, backend-neutral Slug glyph blob.
#[derive(Debug, Clone, PartialEq)]
pub struct SlugGlyph {
    /// Encoder revision that produced this blob (from [`SlugConfig::revision`]).
    pub revision: u32,
    pub bounds: GlyphBounds,
    pub curves: Vec<SlugCurve>,
    pub horizontal_bands: Vec<BandRange>,
    pub horizontal_curve_indices: Vec<u32>,
    pub vertical_bands: Vec<BandRange>,
    pub vertical_curve_indices: Vec<u32>,
}

impl SlugGlyph {
    /// Deterministic little-endian serialization of the records, provided so
    /// #66 can persist/pack blobs and so callers can assert byte-identical
    /// re-encoding. The byte layout is an encoding detail of this contract, not
    /// a GPU buffer format.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.revision.to_le_bytes());
        for v in [
            self.bounds.x_min,
            self.bounds.y_min,
            self.bounds.x_max,
            self.bounds.y_max,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(self.curves.len() as u32).to_le_bytes());
        for c in &self.curves {
            for v in [c.p0.x, c.p0.y, c.p1.x, c.p1.y, c.p2.x, c.p2.y] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        push_band_table(
            &mut out,
            &self.horizontal_bands,
            &self.horizontal_curve_indices,
        );
        push_band_table(&mut out, &self.vertical_bands, &self.vertical_curve_indices);
        out
    }
}

fn push_band_table(out: &mut Vec<u8>, bands: &[BandRange], indices: &[u32]) {
    out.extend_from_slice(&(bands.len() as u32).to_le_bytes());
    for b in bands {
        out.extend_from_slice(&b.lower.to_le_bytes());
        out.extend_from_slice(&b.upper.to_le_bytes());
        out.extend_from_slice(&b.first_curve.to_le_bytes());
        out.extend_from_slice(&b.curve_count.to_le_bytes());
    }
    out.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    for i in indices {
        out.extend_from_slice(&i.to_le_bytes());
    }
}

/// Errors from Slug glyph encoding. These are typed rejections — the cache
/// never stores a blob for an errored encode, so a bad outline cannot poison
/// the cache namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SlugEncodeError {
    /// A cubic Bézier segment was present. Cubics are representable by the path
    /// model but unsupported by the Sprint 7 glyph encoder.
    #[error("cubic segments are not supported by the Slug glyph encoder")]
    UnsupportedSegment,
    /// The outline produced no drawable curves.
    #[error("outline is empty: no drawable curves")]
    EmptyOutline,
}

/// Encode a resolved glyph path into a [`SlugGlyph`]. Pure function of
/// `(path, config)` — same inputs always yield byte-identical records.
pub fn encode_slug_glyph(
    path: &VectorPath,
    config: &SlugConfig,
) -> Result<SlugGlyph, SlugEncodeError> {
    let curves = flatten_curves(path)?;
    if curves.is_empty() {
        return Err(SlugEncodeError::EmptyOutline);
    }

    let bounds = GlyphBounds {
        x_min: path.bounds.min.x,
        y_min: path.bounds.min.y,
        x_max: path.bounds.max.x,
        y_max: path.bounds.max.y,
    };

    // #157 Phase 2 step 5: overlap adjacent bands by a small epsilon,
    // normalized to the config's em scale (upstream README's recommended
    // `1/1024` em), so a curve sitting almost exactly on a band boundary
    // (a floating-point sliver from a real font's rounding) still lands in
    // both neighbouring bands rather than falling through the gap.
    let band_epsilon = config.units_per_em() / 1024.0;
    let (horizontal_bands, horizontal_curve_indices) = build_bands(
        &curves,
        bounds.y_min,
        bounds.y_max,
        config.horizontal_bands(),
        band_epsilon,
        SlugCurve::y_extent,
        SlugCurve::curve_extrema_max_x,
    );
    let (vertical_bands, vertical_curve_indices) = build_bands(
        &curves,
        bounds.x_min,
        bounds.x_max,
        config.vertical_bands(),
        band_epsilon,
        SlugCurve::x_extent,
        SlugCurve::curve_extrema_max_y,
    );

    Ok(SlugGlyph {
        revision: config.revision(),
        bounds,
        curves,
        horizontal_bands,
        horizontal_curve_indices,
        vertical_bands,
        vertical_curve_indices,
    })
}

/// Walk the path commands into a flat curve list, rejecting cubics.
fn flatten_curves(path: &VectorPath) -> Result<Vec<SlugCurve>, SlugEncodeError> {
    let mut curves = Vec::new();
    let mut start = Vec2::ZERO;
    let mut cur = Vec2::ZERO;
    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                start = p;
                cur = p;
            }
            PathCommand::LineTo(p) => {
                if p != cur {
                    curves.push(SlugCurve::line(cur, p));
                }
                cur = p;
            }
            PathCommand::QuadTo { control, to } => {
                if !(to == cur && control == cur) {
                    curves.push(SlugCurve::quad(cur, control, to));
                }
                cur = to;
            }
            PathCommand::CubicTo { .. } => {
                return Err(SlugEncodeError::UnsupportedSegment);
            }
            PathCommand::Close => {
                if cur != start {
                    curves.push(SlugCurve::line(cur, start));
                }
                cur = start;
            }
        }
    }
    Ok(curves)
}

/// Segments whose extent along the scan axis is below this threshold are
/// axis-parallel and excluded from that axis's bands — including them would
/// corrupt the Slug winding count for rays cast along the axis.
const AXIS_PARALLEL_EPSILON: f32 = 1e-6;

/// Partition `[lo, hi]` into `count` equal bands. A curve joins a band when its
/// scan-axis extent overlaps the band, widened by `epsilon` on both sides
/// (#157 Phase 2 step 5 — closes floating-point gaps at a shared band
/// boundary; upstream's README recommends `1/1024` em, normalized by the
/// caller against the config's `units_per_em`), **and** the curve is not
/// axis-parallel (`extent_span >= ε`). Members are ordered by descending
/// `sort_key` — the cross-axis curve extremum (max-x for horizontal rows,
/// max-y for vertical columns) — with ascending curve index breaking ties,
/// per the Slug scanline-rasterization invariant.
pub(crate) fn build_bands(
    curves: &[SlugCurve],
    lo: f32,
    hi: f32,
    count: u16,
    epsilon: f32,
    extent: impl Fn(&SlugCurve) -> (f32, f32),
    sort_key: impl Fn(&SlugCurve) -> f32,
) -> (Vec<BandRange>, Vec<u32>) {
    let count = count.max(1) as usize;
    // A zero or negative span still yields `count` zero-width bands so the
    // table shape is config-stable; every non-axis-parallel curve then overlaps
    // every band.
    let span = (hi - lo).max(0.0);
    let step = span / count as f32;

    let mut bands = Vec::with_capacity(count);
    let mut stream = Vec::new();
    for i in 0..count {
        let lower = lo + step * i as f32;
        let upper = if i + 1 == count {
            hi
        } else {
            lo + step * (i + 1) as f32
        };
        let first_curve = stream.len() as u32;

        let mut members: Vec<u32> = Vec::new();
        for (idx, curve) in curves.iter().enumerate() {
            let (c_lo, c_hi) = extent(curve);
            // Directional exclusion: axis-parallel segments do not cross a ray
            // cast along this axis, so they never enter the band.
            if c_hi - c_lo < AXIS_PARALLEL_EPSILON {
                continue;
            }
            // Inclusive overlap, widened by the band epsilon on both sides so
            // a curve landing a hair outside `[lower, upper]` due to
            // floating-point rounding still joins the band instead of
            // leaving a thin uncovered gap at the boundary.
            if c_hi >= lower - epsilon && c_lo <= upper + epsilon {
                members.push(idx as u32);
            }
        }
        // Descending cross-axis curve extremum (max-x for rows, max-y for
        // columns); ascending index breaks exact ties so the output stays
        // deterministic and byte-stable.
        members.sort_by(|&a, &b| {
            let ka = sort_key(&curves[a as usize]);
            let kb = sort_key(&curves[b as usize]);
            kb.partial_cmp(&ka)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.cmp(&b))
        });

        let curve_count = members.len() as u32;
        stream.extend_from_slice(&members);
        bands.push(BandRange {
            lower,
            upper,
            first_curve,
            curve_count,
        });
    }
    (bands, stream)
}

/// Size-independent outline-blob cache. Owns one immutable [`SlugConfig`]; all
/// blobs in a given cache share that config's namespace. A different config
/// requires a different cache instance — configs never mix within one cache.
#[derive(Debug)]
pub struct SlugBlobCache {
    config: SlugConfig,
    blobs: HashMap<SlugGlyphKey, Arc<SlugGlyph>>,
    hits: u64,
    misses: u64,
}

impl SlugBlobCache {
    pub fn new(config: SlugConfig) -> Self {
        Self {
            config,
            blobs: HashMap::new(),
            hits: 0,
            misses: 0,
        }
    }

    /// The immutable config that defines this cache's namespace.
    pub fn config(&self) -> SlugConfig {
        self.config
    }

    /// Encode `path` under `key`, returning a shared blob. On a cache hit the
    /// stored blob is reused and the hit counter increments. On a miss the blob
    /// is encoded once, stored, and the miss counter increments. On an encode
    /// error nothing is stored (the cache is never poisoned) and no counter
    /// moves. Uses the cache's own `units_per_em` (default `1.0`) for the band
    /// overlap epsilon; callers encoding real font outlines should use
    /// [`Self::encode_with_units_per_em`] instead.
    pub fn encode(
        &mut self,
        key: SlugGlyphKey,
        path: &VectorPath,
    ) -> Result<Arc<SlugGlyph>, SlugEncodeError> {
        self.encode_with_units_per_em(key, path, self.config.units_per_em())
    }

    /// Encode `path` under `key` as [`Self::encode`] does, but normalize the
    /// band overlap epsilon (#157 Phase 2 step 5) against `units_per_em`
    /// instead of the cache's own config for this call only. Lets one cache
    /// serve glyphs from fonts with different units-per-em: `key` (via
    /// [`SlugGlyphKey::font_id`]) already disambiguates by font identity, so
    /// per-call `units_per_em` cannot alias two fonts' blobs together — only
    /// the derived epsilon differs, never correctness.
    pub fn encode_with_units_per_em(
        &mut self,
        key: SlugGlyphKey,
        path: &VectorPath,
        units_per_em: f32,
    ) -> Result<Arc<SlugGlyph>, SlugEncodeError> {
        if let Some(blob) = self.blobs.get(&key) {
            self.hits += 1;
            return Ok(Arc::clone(blob));
        }
        let config = self.config.with_units_per_em(units_per_em);
        let glyph = encode_slug_glyph(path, &config)?;
        let blob = Arc::new(glyph);
        self.blobs.insert(key, Arc::clone(&blob));
        self.misses += 1;
        Ok(blob)
    }

    /// Look up an already-encoded blob without encoding or touching counters.
    pub fn get(&self, key: &SlugGlyphKey) -> Option<Arc<SlugGlyph>> {
        self.blobs.get(key).map(Arc::clone)
    }

    /// Number of blobs served from cache.
    pub fn hits(&self) -> u64 {
        self.hits
    }

    /// Number of fresh encodes stored.
    pub fn misses(&self) -> u64 {
        self.misses
    }

    /// Number of distinct blobs currently retained.
    pub fn len(&self) -> usize {
        self.blobs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.blobs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::{Bounds, FillRule};
    use mkui_text::{Fixed16_16, FontIdAllocator, OpenTypeTag, VariationAxis};

    fn tag(t: &[u8; 4]) -> OpenTypeTag {
        OpenTypeTag::new(*t)
    }

    /// Mint a fresh, process-unique `FontId` through the public allocator —
    /// `FontId` is opaque (#61), so tests cannot forge raw values.
    fn font_id() -> FontId {
        FontIdAllocator::new().allocate().unwrap()
    }

    /// A closed triangle with one quadratic, sized so 2×2 bands fall on exact
    /// half-coordinates. Hand-computed records below.
    fn golden_path() -> VectorPath {
        VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(100.0, 0.0)),
                PathCommand::QuadTo {
                    control: Vec2::new(100.0, 50.0),
                    to: Vec2::new(0.0, 100.0),
                },
                PathCommand::Close,
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
        )
    }

    fn golden_config() -> SlugConfig {
        SlugConfig::new(2, 2, 1)
    }

    fn golden_key(font_id: FontId) -> SlugGlyphKey {
        SlugGlyphKey {
            font_id,
            font_generation: 0,
            glyph_id: 42,
            variation_axes: VariationSettings::empty(),
            synthesis_flags: 0,
            outline_transform: Affine2Fixed::IDENTITY,
        }
    }

    /// True iff `curve_index` is listed within horizontal band `band_index`'s
    /// own slice of the index stream (not just anywhere in the flat stream —
    /// a curve deep inside an adjacent band's un-widened range would also
    /// appear there regardless of the epsilon under test).
    fn horizontal_band_contains(glyph: &SlugGlyph, band_index: usize, curve_index: u32) -> bool {
        let band = glyph.horizontal_bands[band_index];
        let start = band.first_curve as usize;
        let end = start + band.curve_count as usize;
        glyph.horizontal_curve_indices[start..end].contains(&curve_index)
    }

    /// A single non-axis-parallel line whose y-extent (`50.0005..50.0015`)
    /// sits just above the lower/upper band boundary at `y = 50` (bounds
    /// `[0, 100]`, 2 horizontal bands). Exact (unwidened) overlap excludes it
    /// from the lower `[0, 50]` band; a wide-enough epsilon pulls it back in.
    fn boundary_straddling_path() -> VectorPath {
        VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(10.0, 50.0005)),
                PathCommand::LineTo(Vec2::new(20.0, 50.0015)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
        )
    }

    #[test]
    fn band_overlap_epsilon_normalizes_to_units_per_em() {
        // #157 Phase 2 step 5: this exercises `build_bands`'s widened overlap
        // test through the real encoder — not a pre-encoded fixture — so
        // shrinking the epsilon back to zero (or a units_per_em too small to
        // matter) would fail this test, unlike a GPU test that only replays
        // already-encoded band tables.
        let path = boundary_straddling_path();

        // Default units_per_em = 1.0 -> epsilon = 1/1024 ~= 0.000977, wide
        // enough that the curve's 0.0005 offset past the boundary still
        // pulls it into the lower [0, 50] band.
        let wide = encode_slug_glyph(&path, &SlugConfig::new(2, 1, 1)).unwrap();
        assert!(
            horizontal_band_contains(&wide, 0, 0),
            "units_per_em=1.0's epsilon (1/1024) must pull the near-boundary \
             curve into the lower band"
        );

        // A tighter units_per_em (as if a font's design units were a fraction
        // of an em) shrinks the epsilon below the curve's 0.0005 offset,
        // excluding it from the lower band again.
        let narrow_config = SlugConfig::new(2, 1, 1).with_units_per_em(0.1);
        let narrow = encode_slug_glyph(&path, &narrow_config).unwrap();
        assert!(
            !horizontal_band_contains(&narrow, 0, 0),
            "a tighter units_per_em must shrink the epsilon back below the \
             boundary offset"
        );
    }

    #[test]
    fn cache_encode_with_units_per_em_overrides_the_cache_default_per_call() {
        // Proves the exact plumbing `mkui-wgpu`'s `slug_text::place_slug_run`
        // relies on: a shared cache (one fixed `SlugConfig`, default
        // units_per_em = 1.0) still applies a *different*, real font's
        // units-per-em when the caller passes one per glyph. `SlugGlyphKey`'s
        // `font_id` already disambiguates the two calls below, so this cannot
        // alias the two fonts' blobs together — only the derived epsilon
        // (and thus band membership) differs.
        let path = boundary_straddling_path();
        let mut cache = SlugBlobCache::new(SlugConfig::new(2, 1, 1));

        let wide = cache
            .encode_with_units_per_em(golden_key(font_id()), &path, 1.0)
            .unwrap();
        assert!(horizontal_band_contains(&wide, 0, 0));

        let narrow = cache
            .encode_with_units_per_em(golden_key(font_id()), &path, 0.1)
            .unwrap();
        assert!(!horizontal_band_contains(&narrow, 0, 0));

        // Plain `encode` (what `mkui-text`'s space-glyph / non-SFNT callers
        // use) falls back to the cache's own units_per_em (1.0 here) — same
        // outcome as `wide`.
        let via_plain_encode = cache.encode(golden_key(font_id()), &path).unwrap();
        assert!(horizontal_band_contains(&via_plain_encode, 0, 0));
    }

    #[test]
    fn golden_quadratic_outline_records() {
        let glyph = encode_slug_glyph(&golden_path(), &golden_config()).unwrap();

        assert_eq!(glyph.revision, 1);
        assert_eq!(
            glyph.bounds,
            GlyphBounds {
                x_min: 0.0,
                y_min: 0.0,
                x_max: 100.0,
                y_max: 100.0
            }
        );

        // curve0: bottom line (0,0)->(100,0), duplicated-endpoint sentinel.
        // curve1: quadratic (100,0),(100,50),(0,100).
        // curve2: closing line (0,100)->(0,0), duplicated-endpoint sentinel.
        assert_eq!(
            glyph.curves,
            vec![
                SlugCurve {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(100.0, 0.0),
                    p2: Vec2::new(100.0, 0.0),
                },
                SlugCurve {
                    p0: Vec2::new(100.0, 0.0),
                    p1: Vec2::new(100.0, 50.0),
                    p2: Vec2::new(0.0, 100.0),
                },
                SlugCurve {
                    p0: Vec2::new(0.0, 100.0),
                    p1: Vec2::new(0.0, 0.0),
                    p2: Vec2::new(0.0, 0.0),
                },
            ]
        );

        // Horizontal bands (y): curve0 is horizontal (|dy|=0) → excluded. Both
        // remaining curves overlap both rows; they share a top-endpoint y of
        // 100, so ascending index breaks the tie → [1, 2].
        assert_eq!(
            glyph.horizontal_bands,
            vec![
                BandRange {
                    lower: 0.0,
                    upper: 50.0,
                    first_curve: 0,
                    curve_count: 2
                },
                BandRange {
                    lower: 50.0,
                    upper: 100.0,
                    first_curve: 2,
                    curve_count: 2
                },
            ]
        );
        // Horizontal rows sort by descending max-x: curve1 (max-x 100) precedes
        // curve2 (max-x 0).
        assert_eq!(glyph.horizontal_curve_indices, vec![1, 2, 1, 2]);

        // Vertical bands (x): curve2 is vertical (|dx|=0) → excluded. Columns
        // sort by descending max-y: curve1 (max-y 100) precedes curve0 (max-y 0).
        assert_eq!(
            glyph.vertical_bands,
            vec![
                BandRange {
                    lower: 0.0,
                    upper: 50.0,
                    first_curve: 0,
                    curve_count: 2
                },
                BandRange {
                    lower: 50.0,
                    upper: 100.0,
                    first_curve: 2,
                    curve_count: 2
                },
            ]
        );
        assert_eq!(glyph.vertical_curve_indices, vec![1, 0, 1, 0]);
    }

    #[test]
    fn line_sentinel_and_axis_parallel_exclusion() {
        // One horizontal line, one diagonal line — open path so each command
        // maps to exactly one curve index.
        let path = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                // curve0: horizontal line (|dy| == 0) — excluded from rows.
                PathCommand::LineTo(Vec2::new(100.0, 0.0)),
                // curve1: diagonal line — changes x and y.
                PathCommand::LineTo(Vec2::new(0.0, 100.0)),
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(100.0, 100.0)),
        );
        let glyph = encode_slug_glyph(&path, &SlugConfig::new(2, 2, 1)).unwrap();

        // Both lines carry the duplicated-endpoint sentinel (p1 == p2).
        assert_eq!(glyph.curves[0].p1, glyph.curves[0].p2);
        let diagonal = glyph.curves[1];
        assert_eq!(diagonal.p1, diagonal.p2);
        assert_eq!(diagonal.p1, Vec2::new(0.0, 100.0));

        // The horizontal segment (curve 0) is in no horizontal band; the
        // diagonal (curve 1, which is not axis-parallel) is.
        assert!(!glyph.horizontal_curve_indices.contains(&0));
        assert!(glyph.horizontal_curve_indices.contains(&1));
    }

    /// Three diagonal line segments laid out as separate contours so their
    /// max-x and max-y disambiguate the sort axis. Each is non-axis-parallel,
    /// so all three land in a single full-height/full-width band.
    ///
    /// | curve | max-x | max-y |
    /// |-------|-------|-------|
    /// | 0     | 2.0   | 10.0  |
    /// | 1     | 5.0   | 1.0   |
    /// | 2     | 3.0   | 5.0   |
    fn disambiguating_path() -> VectorPath {
        VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(2.0, 10.0)), // curve0
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(5.0, 1.0)), // curve1
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(3.0, 5.0)), // curve2
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(5.0, 10.0)),
        )
    }

    #[test]
    fn horizontal_band_sorts_by_descending_max_x() {
        // Single full-height row so all three curves share one band.
        let glyph = encode_slug_glyph(&disambiguating_path(), &SlugConfig::new(1, 1, 1)).unwrap();
        // max-x descending: curve1 (5) > curve2 (3) > curve0 (2).
        assert_eq!(glyph.horizontal_curve_indices, vec![1, 2, 0]);
    }

    #[test]
    fn vertical_band_sorts_by_descending_max_y() {
        let glyph = encode_slug_glyph(&disambiguating_path(), &SlugConfig::new(1, 1, 1)).unwrap();
        // max-y descending: curve0 (10) > curve2 (5) > curve1 (1).
        assert_eq!(glyph.vertical_curve_indices, vec![0, 2, 1]);
    }

    #[test]
    fn band_sort_axes_are_not_swapped() {
        // The two orientations must produce *different* orderings; if the sort
        // axes were swapped or applied globally these would coincide.
        let glyph = encode_slug_glyph(&disambiguating_path(), &SlugConfig::new(1, 1, 1)).unwrap();
        assert_ne!(
            glyph.horizontal_curve_indices, glyph.vertical_curve_indices,
            "horizontal (max-x) and vertical (max-y) sorts must differ"
        );
    }

    #[test]
    fn quadratic_extremum_uses_interior_critical_point() {
        // Control point well above the endpoints: the true y-maximum is the
        // interior critical point at t=0.5, not the endpoint max of 0.
        let arch = SlugCurve {
            p0: Vec2::new(0.0, 0.0),
            p1: Vec2::new(50.0, 200.0),
            p2: Vec2::new(100.0, 0.0),
        };
        assert_eq!(arch.curve_extrema_max_y(), 100.0);
        // x is monotone across the endpoints, so max-x is the endpoint value.
        assert_eq!(arch.curve_extrema_max_x(), 100.0);
    }

    #[test]
    fn re_encoding_is_byte_identical() {
        let a = encode_slug_glyph(&golden_path(), &golden_config()).unwrap();
        let b = encode_slug_glyph(&golden_path(), &golden_config()).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.to_le_bytes(), b.to_le_bytes());
    }

    #[test]
    fn same_key_twice_hits_and_reuses_one_blob() {
        let mut cache = SlugBlobCache::new(golden_config());
        let path = golden_path();
        let key = golden_key(font_id());
        let first = cache.encode(key.clone(), &path).unwrap();
        let second = cache.encode(key, &path).unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1);
        assert!(
            Arc::ptr_eq(&first, &second),
            "blob is reused, not re-encoded"
        );
    }

    #[test]
    fn identity_differences_each_miss() {
        let path = golden_path();
        let base = golden_key(font_id());

        // Each variant differs from `base` in exactly one identity field.
        let variants = vec![
            // A different global FontId (freshly minted) must miss.
            SlugGlyphKey {
                font_id: font_id(),
                ..base.clone()
            },
            SlugGlyphKey {
                font_generation: 1,
                ..base.clone()
            },
            SlugGlyphKey {
                glyph_id: 7,
                ..base.clone()
            },
            SlugGlyphKey {
                variation_axes: VariationSettings::new([VariationAxis {
                    tag: tag(b"wght"),
                    value: Fixed16_16::from_f32(700.0).unwrap(),
                }])
                .unwrap(),
                ..base.clone()
            },
            SlugGlyphKey {
                synthesis_flags: 0b1,
                ..base.clone()
            },
            // Translation-only affine difference must still miss.
            SlugGlyphKey {
                outline_transform: Affine2Fixed {
                    tx: Fixed16_16::from_f32(1.0).unwrap(),
                    ..Affine2Fixed::IDENTITY
                },
                ..base.clone()
            },
            // A linear-component affine difference must also miss.
            SlugGlyphKey {
                outline_transform: Affine2Fixed {
                    a: Fixed16_16::from_f32(2.0).unwrap(),
                    ..Affine2Fixed::IDENTITY
                },
                ..base.clone()
            },
        ];

        let mut cache = SlugBlobCache::new(golden_config());
        cache.encode(base, &path).unwrap();
        let mut expected_len = 1;
        for v in variants {
            cache.encode(v, &path).unwrap();
            expected_len += 1;
            assert_eq!(cache.len(), expected_len);
        }
        assert_eq!(cache.hits(), 0, "every distinct key is a fresh miss");
        assert_eq!(cache.misses(), expected_len as u64);
    }

    #[test]
    fn reordered_axes_compare_equal_and_share_a_blob() {
        let wght = VariationAxis {
            tag: tag(b"wght"),
            value: Fixed16_16::from_f32(700.0).unwrap(),
        };
        let ital = VariationAxis {
            tag: tag(b"ital"),
            value: Fixed16_16::ONE,
        };
        // One shared base key so the two variants differ *only* in axis order.
        let base = golden_key(font_id());
        let key_ab = SlugGlyphKey {
            variation_axes: VariationSettings::new([wght, ital]).unwrap(),
            ..base.clone()
        };
        let key_ba = SlugGlyphKey {
            variation_axes: VariationSettings::new([ital, wght]).unwrap(),
            ..base
        };

        let mut cache = SlugBlobCache::new(golden_config());
        let path = golden_path();
        cache.encode(key_ab, &path).unwrap();
        cache.encode(key_ba, &path).unwrap();
        assert_eq!(cache.len(), 1, "canonical axis order makes the keys equal");
        assert_eq!(cache.hits(), 1);
    }

    #[test]
    fn cubic_outline_errors_without_poisoning_cache() {
        let cubic = VectorPath::new(
            vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::CubicTo {
                    control1: Vec2::new(10.0, 20.0),
                    control2: Vec2::new(30.0, 40.0),
                    to: Vec2::new(50.0, 0.0),
                },
                PathCommand::Close,
            ],
            FillRule::NonZero,
            Bounds::new(Vec2::new(0.0, 0.0), Vec2::new(50.0, 40.0)),
        );
        let mut cache = SlugBlobCache::new(golden_config());
        let err = cache.encode(golden_key(font_id()), &cubic).unwrap_err();
        assert_eq!(err, SlugEncodeError::UnsupportedSegment);
        assert!(cache.is_empty(), "errored encode does not poison the cache");
        assert_eq!(cache.misses(), 0);
    }

    #[test]
    fn empty_outline_errors_without_poisoning_cache() {
        let empty = VectorPath::new(
            vec![PathCommand::MoveTo(Vec2::ZERO), PathCommand::Close],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let mut cache = SlugBlobCache::new(golden_config());
        let err = cache.encode(golden_key(font_id()), &empty).unwrap_err();
        assert_eq!(err, SlugEncodeError::EmptyOutline);
        assert!(cache.is_empty());
    }

    #[test]
    fn different_configs_are_separate_namespaces() {
        let path = golden_path();
        let mut cache_a = SlugBlobCache::new(SlugConfig::new(2, 2, 1));
        let mut cache_b = SlugBlobCache::new(SlugConfig::new(4, 4, 1));
        let key = golden_key(font_id());
        let a = cache_a.encode(key.clone(), &path).unwrap();
        let b = cache_b.encode(key, &path).unwrap();
        // Same key, but each cache owns its own config + blob.
        assert_ne!(a.horizontal_bands.len(), b.horizontal_bands.len());
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
