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
//! # Record layout (versioned via [`SlugConfig::revision`])
//!
//! - [`SlugGlyph::curves`]: every segment as a quadratic `(p0, p1, p2)`. Line
//!   segments are encoded as a degenerate quadratic whose control point is the
//!   midpoint of the endpoints, so #66 can treat the whole stream uniformly.
//! - [`SlugGlyph::horizontal_bands`]: the glyph's y-range split into
//!   `SlugConfig::horizontal_bands` equal rows. Band *i* lists every curve
//!   whose y-extent overlaps the row, via `[first_curve, first_curve +
//!   curve_count)` into [`SlugGlyph::horizontal_curve_indices`].
//! - [`SlugGlyph::vertical_bands`] / [`SlugGlyph::vertical_curve_indices`]: the
//!   same, for the x-range split into columns.
//!
//! Within a band, curve indices are sorted ascending. The encoding is a pure
//! function of `(path, config)`, so re-encoding is byte-identical.
//!
//! # What #66 may and may not do
//!
//! #66 may serialize these records (see [`SlugGlyph::to_le_bytes`]) and pack
//! them into GPU buffers. It may **not** reimplement or reinterpret the band
//! algorithm — the band membership and ordering produced here are the contract.

use std::collections::HashMap;
use std::sync::Arc;

use crate::fixed::{Affine2Fixed, VariationSettings};
use crate::path::{PathCommand, Vec2, VectorPath};
use mkui_text::FontId;

/// Immutable encoder configuration. Held by the owning [`SlugBlobCache`]; it is
/// **not** part of [`SlugGlyphKey`] so two caches with different configs form
/// separate namespaces and cannot alias each other's blobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SlugConfig {
    horizontal_bands: u16,
    vertical_bands: u16,
    revision: u32,
}

impl SlugConfig {
    /// Construct a config. Band counts are clamped to at least 1 (a glyph
    /// always needs one row and one column).
    pub fn new(horizontal_bands: u16, vertical_bands: u16, revision: u32) -> Self {
        Self {
            horizontal_bands: horizontal_bands.max(1),
            vertical_bands: vertical_bands.max(1),
            revision,
        }
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

/// A single quadratic curve record `(p0, p1, p2)` in font units, y-up. A line
/// segment is stored as a degenerate quadratic (`p1` == midpoint of `p0`/`p2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SlugCurve {
    pub p0: Vec2,
    pub p1: Vec2,
    pub p2: Vec2,
}

impl SlugCurve {
    fn line(p0: Vec2, p2: Vec2) -> Self {
        Self {
            p0,
            p1: Vec2::new((p0.x + p2.x) * 0.5, (p0.y + p2.y) * 0.5),
            p2,
        }
    }

    fn quad(p0: Vec2, p1: Vec2, p2: Vec2) -> Self {
        Self { p0, p1, p2 }
    }

    fn y_extent(&self) -> (f32, f32) {
        let lo = self.p0.y.min(self.p1.y).min(self.p2.y);
        let hi = self.p0.y.max(self.p1.y).max(self.p2.y);
        (lo, hi)
    }

    fn x_extent(&self) -> (f32, f32) {
        let lo = self.p0.x.min(self.p1.x).min(self.p2.x);
        let hi = self.p0.x.max(self.p1.x).max(self.p2.x);
        (lo, hi)
    }
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

    let (horizontal_bands, horizontal_curve_indices) = build_bands(
        &curves,
        bounds.y_min,
        bounds.y_max,
        config.horizontal_bands(),
        SlugCurve::y_extent,
    );
    let (vertical_bands, vertical_curve_indices) = build_bands(
        &curves,
        bounds.x_min,
        bounds.x_max,
        config.vertical_bands(),
        SlugCurve::x_extent,
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

/// Partition `[lo, hi]` into `count` equal bands and assign each curve to every
/// band whose range overlaps the curve's extent along the chosen axis.
fn build_bands(
    curves: &[SlugCurve],
    lo: f32,
    hi: f32,
    count: u16,
    extent: impl Fn(&SlugCurve) -> (f32, f32),
) -> (Vec<BandRange>, Vec<u32>) {
    let count = count.max(1) as usize;
    // A zero or negative span still yields `count` zero-width bands so the
    // table shape is config-stable; every curve then overlaps every band.
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
        for (idx, curve) in curves.iter().enumerate() {
            let (c_lo, c_hi) = extent(curve);
            // Conservative inclusive overlap: a curve on a shared edge lands in
            // both neighbouring bands.
            if c_hi >= lower && c_lo <= upper {
                stream.push(idx as u32);
            }
        }
        let curve_count = stream.len() as u32 - first_curve;
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
    /// moves.
    pub fn encode(
        &mut self,
        key: SlugGlyphKey,
        path: &VectorPath,
    ) -> Result<Arc<SlugGlyph>, SlugEncodeError> {
        if let Some(blob) = self.blobs.get(&key) {
            self.hits += 1;
            return Ok(Arc::clone(blob));
        }
        let glyph = encode_slug_glyph(path, &self.config)?;
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

    fn tag(t: &[u8; 4]) -> crate::fixed::OpenTypeTag {
        crate::fixed::OpenTypeTag::new(*t)
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

    fn golden_key() -> SlugGlyphKey {
        SlugGlyphKey {
            font_id: FontId(1),
            font_generation: 0,
            glyph_id: 42,
            variation_axes: VariationSettings::empty(),
            synthesis_flags: 0,
            outline_transform: Affine2Fixed::IDENTITY,
        }
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

        // curve0: bottom line (0,0)->(100,0), control = midpoint.
        // curve1: quadratic (100,0),(100,50),(0,100).
        // curve2: closing line (0,100)->(0,0), control = midpoint.
        assert_eq!(
            glyph.curves,
            vec![
                SlugCurve {
                    p0: Vec2::new(0.0, 0.0),
                    p1: Vec2::new(50.0, 0.0),
                    p2: Vec2::new(100.0, 0.0),
                },
                SlugCurve {
                    p0: Vec2::new(100.0, 0.0),
                    p1: Vec2::new(100.0, 50.0),
                    p2: Vec2::new(0.0, 100.0),
                },
                SlugCurve {
                    p0: Vec2::new(0.0, 100.0),
                    p1: Vec2::new(0.0, 50.0),
                    p2: Vec2::new(0.0, 0.0),
                },
            ]
        );

        // Horizontal bands (y): row0 [0,50] = {0,1,2}; row1 [50,100] = {1,2}.
        assert_eq!(
            glyph.horizontal_bands,
            vec![
                BandRange {
                    lower: 0.0,
                    upper: 50.0,
                    first_curve: 0,
                    curve_count: 3
                },
                BandRange {
                    lower: 50.0,
                    upper: 100.0,
                    first_curve: 3,
                    curve_count: 2
                },
            ]
        );
        assert_eq!(glyph.horizontal_curve_indices, vec![0, 1, 2, 1, 2]);

        // Vertical bands (x): col0 [0,50] = {0,1,2}; col1 [50,100] = {0,1}.
        assert_eq!(
            glyph.vertical_bands,
            vec![
                BandRange {
                    lower: 0.0,
                    upper: 50.0,
                    first_curve: 0,
                    curve_count: 3
                },
                BandRange {
                    lower: 50.0,
                    upper: 100.0,
                    first_curve: 3,
                    curve_count: 2
                },
            ]
        );
        assert_eq!(glyph.vertical_curve_indices, vec![0, 1, 2, 0, 1]);
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
        let first = cache.encode(golden_key(), &path).unwrap();
        let second = cache.encode(golden_key(), &path).unwrap();
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
        let base = golden_key();

        // Each variant differs from `base` in exactly one identity field.
        let variants = vec![
            SlugGlyphKey {
                font_id: FontId(2),
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
                variation_axes: VariationSettings::new(vec![crate::fixed::VariationAxis::new(
                    tag(b"wght"),
                    crate::fixed::Fixed16_16::from_f32(700.0).unwrap(),
                )])
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
                    tx: crate::fixed::Fixed16_16::from_f32(1.0).unwrap(),
                    ..Affine2Fixed::IDENTITY
                },
                ..base.clone()
            },
            // A linear-component affine difference must also miss.
            SlugGlyphKey {
                outline_transform: Affine2Fixed {
                    a: crate::fixed::Fixed16_16::from_f32(2.0).unwrap(),
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
        let wght = crate::fixed::VariationAxis::new(
            tag(b"wght"),
            crate::fixed::Fixed16_16::from_f32(700.0).unwrap(),
        );
        let ital = crate::fixed::VariationAxis::new(tag(b"ital"), crate::fixed::Fixed16_16::ONE);
        let key_ab = SlugGlyphKey {
            variation_axes: VariationSettings::new(vec![wght, ital]).unwrap(),
            ..golden_key()
        };
        let key_ba = SlugGlyphKey {
            variation_axes: VariationSettings::new(vec![ital, wght]).unwrap(),
            ..golden_key()
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
        let err = cache.encode(golden_key(), &cubic).unwrap_err();
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
        let err = cache.encode(golden_key(), &empty).unwrap_err();
        assert_eq!(err, SlugEncodeError::EmptyOutline);
        assert!(cache.is_empty());
    }

    #[test]
    fn different_configs_are_separate_namespaces() {
        let path = golden_path();
        let mut cache_a = SlugBlobCache::new(SlugConfig::new(2, 2, 1));
        let mut cache_b = SlugBlobCache::new(SlugConfig::new(4, 4, 1));
        let a = cache_a.encode(golden_key(), &path).unwrap();
        let b = cache_b.encode(golden_key(), &path).unwrap();
        // Same key, but each cache owns its own config + blob.
        assert_ne!(a.horizontal_bands.len(), b.horizontal_bands.len());
        assert!(!Arc::ptr_eq(&a, &b));
    }
}
