//! Deterministic Bézier encoder for **arbitrary** vector paths.
//!
//! Where [`crate::slug::encode_slug_glyph`] encodes a resolved *glyph* outline
//! (line/quadratic only, rejecting cubics), this module encodes a general
//! [`VectorPath`] — icons, analytic primitives, stroked-then-filled outlines —
//! into the same [`SlugGlyph`] curve/band record contract. It differs from the
//! glyph lane in three ways:
//!
//! 1. **Cubics are subdivided, not rejected.** Each [`PathCommand::CubicTo`] is
//!    lowered to a chain of quadratics by [`crate::bezier`] using the hard-coded
//!    1° angular tolerance (Slug rasterizes quadratics only).
//! 2. **Bounds are computed, not trusted.** A general caller has no
//!    provider-resolved ink box, so the band extents are derived from the
//!    flattened geometry's own control hull — of the *transformed* geometry,
//!    see below.
//! 3. **The cache key is content-derived.** There is no font identity to key on,
//!    so [`VectorPathKey`] is a canonical byte serialization of the path
//!    geometry (plus its fill rule and transform, #304) — deterministic and
//!    byte-identical across threads.
//!
//! # Fill rule and transform (#304)
//!
//! Unlike the glyph lane, this encoder honours [`VectorPath::fill`] and
//! [`VectorPath::transform`] rather than hard-coding non-zero winding and
//! ignoring the transform:
//!
//! - **Fill rule.** The source [`FillRule`] is carried verbatim into the
//!   returned [`SlugGlyph::fill_rule`]. The curve/band records themselves are
//!   rule-agnostic (#304 doesn't change how a band is built); a downstream
//!   rasterizer (`mkui-vector2d-wgpu`'s Slug shader) reads `fill_rule` to pick
//!   non-zero vs. even-odd coverage. Two same-winding nested contours
//!   therefore still accumulate winding ≥ 2 at their overlap — correctly
//!   *filled* under non-zero and a *hole* under even-odd, decided entirely at
//!   the rasterizer, not here.
//! - **Transform order.** [`VectorPath::transform`] is applied to every
//!   authored point — including cubic control points, *before* subdivision
//!   (a skew or non-uniform scale does not preserve angles, so the 1°
//!   tolerance only holds in the space subdivision actually happens in) —
//!   before flattening, before bounds derivation, and before band
//!   partitioning. The full pipeline order is: authored point (path-local,
//!   y-up) → [`Affine2`] → `origin_px`/`scale_px_per_unit` GPU placement
//!   (`mkui-vector2d-wgpu`) → the adapter's y-flip into screen space. A
//!   stroke centreline's transform is applied identically by
//!   [`crate::stroke_expand::stroke_to_fill`] — see that module for how
//!   stroke width scales (and, under shear/non-uniform scale, skews) with it.
//! - **Typed rejections.** A non-finite transform field, a transformed point
//!   that overflows to infinity, or a transform whose linear part has a zero
//!   determinant (collapses the path to zero area, no inverse) are each
//!   rejected before any geometry is produced — see [`VectorPathEncodeError`].
//!
//! No GPU type appears here; #138 owns the wgpu adapter.

use std::collections::HashMap;
use std::sync::Arc;

use crate::bezier::{subdivide_cubic, SubdivisionError};
use crate::path::{Affine2, FillRule, PathCommand, Vec2, VectorPath};
use crate::slug::{build_bands, GlyphBounds, SlugConfig, SlugCurve, SlugGlyph};

/// Errors from encoding an arbitrary vector path. Typed rejections — a cache
/// never stores a blob for an errored encode, so bad input cannot poison it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum VectorPathEncodeError {
    /// A coordinate was NaN or infinite. Rejected at the input boundary before
    /// any geometry is produced (Sprint 7 §3.4 canonicalization rationale).
    /// Covers three moments (#304): a non-finite `transform` field (checked
    /// before any point is transformed), a non-finite authored coordinate,
    /// and a transformed point that overflows to infinity (an extreme scale
    /// applied to a large but finite coordinate) — checked right after that
    /// point is mapped.
    #[error("path contains a non-finite (NaN or infinite) coordinate")]
    NonFinite,
    /// The path produced no drawable curves (no on-curve geometry).
    #[error("path is empty: no drawable curves")]
    EmptyPath,
    /// A `CubicTo` segment could not be lowered to a certified quadratic chain
    /// (see [`SubdivisionError`]): the geometry needs pieces below `f32`
    /// resolution, or a cubic has no tangent direction at all. Emitting an
    /// uncertified approximation would silently break the 1° lowering
    /// contract, so the encode is rejected instead.
    #[error("cubic subdivision failed: {0}")]
    Subdivision(#[from] SubdivisionError),
    /// The path's `transform` has a zero-determinant linear part (#304): it
    /// collapses every point onto a line or a point, which has no interior
    /// under either fill rule and no inverse for later placement math. Not
    /// representable; rejected rather than silently producing a degenerate
    /// (zero-area) blob.
    #[error("path transform is singular (zero-area linear mapping)")]
    SingularTransform,
}

/// Encode an arbitrary [`VectorPath`] into a [`SlugGlyph`] curve/band blob.
///
/// Pure function of `(path, config)`: the same logical input always yields
/// byte-identical records. Cubics are subdivided to quadratics (in the
/// already-transformed space, see the module docs' transform-order note);
/// NaN/Inf inputs (authored or transformed) and a singular transform are
/// rejected up front; `path.fill` is carried verbatim into the returned
/// [`SlugGlyph::fill_rule`].
pub fn encode_vector_path(
    path: &VectorPath,
    config: &SlugConfig,
) -> Result<SlugGlyph, VectorPathEncodeError> {
    reject_non_finite(path)?;
    reject_singular_transform(path)?;

    let curves = flatten_path_curves(path)?;
    if curves.is_empty() {
        return Err(VectorPathEncodeError::EmptyPath);
    }

    // General callers carry no resolved ink box, so derive band extents from the
    // flattened control hull. It is a conservative superset of the true ink
    // bounds — every curve still lands in the right bands.
    let bounds = control_hull_bounds(&curves);

    // #157 Phase 2 step 5 — see `slug::encode_slug_glyph`'s identical epsilon.
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
        fill_rule: path.fill,
        bounds,
        curves,
        horizontal_bands,
        horizontal_curve_indices,
        vertical_bands,
        vertical_curve_indices,
    })
}

/// Reject a transform whose linear part has zero determinant (#304): it
/// collapses the path to zero area, which neither fill rule can draw and
/// which has no inverse for later placement math.
fn reject_singular_transform(path: &VectorPath) -> Result<(), VectorPathEncodeError> {
    if path.transform.determinant() == 0.0 {
        return Err(VectorPathEncodeError::SingularTransform);
    }
    Ok(())
}

/// Reject NaN/Inf at the input boundary, before any geometry is produced.
/// Checks the raw `transform` fields themselves (#304) in addition to the
/// existing authored-coordinate check — a transformed *point* going
/// non-finite (e.g. an extreme but individually-finite scale applied to a
/// large coordinate) is instead caught where it happens, in
/// [`flatten_path_curves`], since that failure depends on both the transform
/// and the specific point together.
fn reject_non_finite(path: &VectorPath) -> Result<(), VectorPathEncodeError> {
    let Affine2 { a, b, c, d, tx, ty } = path.transform;
    if ![a, b, c, d, tx, ty].into_iter().all(f32::is_finite) {
        return Err(VectorPathEncodeError::NonFinite);
    }
    let finite = |p: Vec2| p.x.is_finite() && p.y.is_finite();
    for cmd in &path.commands {
        let ok = match *cmd {
            PathCommand::MoveTo(p) | PathCommand::LineTo(p) => finite(p),
            PathCommand::QuadTo { control, to } => finite(control) && finite(to),
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => finite(control1) && finite(control2) && finite(to),
            PathCommand::Close => true,
        };
        if !ok {
            return Err(VectorPathEncodeError::NonFinite);
        }
    }
    Ok(())
}

/// Walk the path into a flat quadratic-curve list, subdividing cubics. Every
/// authored point is mapped through `path.transform` as it is read (#304) —
/// including a cubic's control points, before subdivision, so the 1°
/// subdivision tolerance holds in the space it is actually measured in rather
/// than being invalidated by a later skew/non-uniform scale. Lines use the
/// Slug duplicated-endpoint sentinel; cubics become one or more quadratics. A
/// no-op cubic (all points at the pen position, post-transform) is skipped
/// like a no-op quad; any other subdivision failure aborts the encode with a
/// typed error.
fn flatten_path_curves(path: &VectorPath) -> Result<Vec<SlugCurve>, VectorPathEncodeError> {
    // Maps a point through `path.transform` and rejects it if that mapping
    // overflowed to infinity (the transform's own fields were already
    // checked finite by `reject_non_finite`).
    let xf = |p: Vec2| -> Result<Vec2, VectorPathEncodeError> {
        let q = path.transform.transform_point(p);
        if q.x.is_finite() && q.y.is_finite() {
            Ok(q)
        } else {
            Err(VectorPathEncodeError::NonFinite)
        }
    };

    let mut curves = Vec::new();
    let mut start = Vec2::ZERO;
    let mut cur = Vec2::ZERO;
    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                let p = xf(p)?;
                start = p;
                cur = p;
            }
            PathCommand::LineTo(p) => {
                let p = xf(p)?;
                if p != cur {
                    curves.push(SlugCurve::line(cur, p));
                }
                cur = p;
            }
            PathCommand::QuadTo { control, to } => {
                let control = xf(control)?;
                let to = xf(to)?;
                if !(to == cur && control == cur) {
                    curves.push(SlugCurve::quad(cur, control, to));
                }
                cur = to;
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                let control1 = xf(control1)?;
                let control2 = xf(control2)?;
                let to = xf(to)?;
                // A no-op cubic (everything at the pen position) draws nothing,
                // exactly like the no-op quad above — skip it rather than
                // surface `DegenerateInput` for a segment with no geometry.
                if !(control1 == cur && control2 == cur && to == cur) {
                    let mut piece_start = cur;
                    for (control, end) in subdivide_cubic(cur, control1, control2, to)? {
                        // Skip a fully-degenerate sub-piece (a zero-length
                        // quadratic contributes no geometry and no band
                        // membership).
                        if !(end == piece_start && control == piece_start) {
                            curves.push(SlugCurve::quad(piece_start, control, end));
                        }
                        piece_start = end;
                    }
                }
                cur = to;
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

/// Axis-aligned bounds of the flattened curves' control points — a conservative
/// superset of the true ink bounds, sufficient for band partitioning.
fn control_hull_bounds(curves: &[SlugCurve]) -> GlyphBounds {
    let mut x_min = f32::INFINITY;
    let mut y_min = f32::INFINITY;
    let mut x_max = f32::NEG_INFINITY;
    let mut y_max = f32::NEG_INFINITY;
    for c in curves {
        for p in [c.p0, c.p1, c.p2] {
            x_min = x_min.min(p.x);
            y_min = y_min.min(p.y);
            x_max = x_max.max(p.x);
            y_max = y_max.max(p.y);
        }
    }
    GlyphBounds {
        x_min,
        y_min,
        x_max,
        y_max,
    }
}

/// Canonical, content-derived cache key for an arbitrary vector path.
///
/// With no font identity to key on (unlike [`crate::slug::SlugGlyphKey`]), the
/// key is a deterministic byte serialization of the path's fill rule,
/// transform, and drawing commands (#304) — all three now determine the
/// encoded records, so all three must be part of the key or two
/// geometrically-identical paths differing only in rule or transform would
/// wrongly collide in a cache. Two logically-identical paths — built on
/// different threads, in any order — serialize to byte-identical keys, so
/// equality and hashing are exact and collision-free.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VectorPathKey {
    canonical: Vec<u8>,
}

impl VectorPathKey {
    /// Derive a canonical key from a path. Fails on NaN/Inf so a non-finite path
    /// can never produce a key (its bits are not canonical).
    pub fn from_path(path: &VectorPath) -> Result<Self, VectorPathEncodeError> {
        reject_non_finite(path)?;
        Ok(Self {
            canonical: canonical_bytes(path),
        })
    }

    /// The canonical byte serialization backing this key.
    pub fn as_bytes(&self) -> &[u8] {
        &self.canonical
    }
}

/// Command tag bytes for the canonical serialization. Stable — appending new
/// variants is fine, reusing a value is a breaking key change.
const TAG_MOVE: u8 = 0;
const TAG_LINE: u8 = 1;
const TAG_QUAD: u8 = 2;
const TAG_CUBIC: u8 = 3;
const TAG_CLOSE: u8 = 4;

/// Serialization format version, so a future layout change is distinguishable.
/// Bumped to 2 (#304) when the fill rule and transform joined the key.
const KEY_FORMAT_VERSION: u8 = 2;

/// Fill-rule tag byte for the canonical serialization — stable, like the
/// command tags above.
const FILL_RULE_NONZERO: u8 = 0;
const FILL_RULE_EVEN_ODD: u8 = 1;

fn canonical_bytes(path: &VectorPath) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(KEY_FORMAT_VERSION);
    out.push(match path.fill {
        FillRule::NonZero => FILL_RULE_NONZERO,
        FillRule::EvenOdd => FILL_RULE_EVEN_ODD,
    });
    let Affine2 { a, b, c, d, tx, ty } = path.transform;
    for v in [a, b, c, d, tx, ty] {
        out.extend_from_slice(&canonical_f32_bits(v).to_le_bytes());
    }
    out.extend_from_slice(&(path.commands.len() as u32).to_le_bytes());
    for cmd in &path.commands {
        match *cmd {
            PathCommand::MoveTo(p) => {
                out.push(TAG_MOVE);
                push_point(&mut out, p);
            }
            PathCommand::LineTo(p) => {
                out.push(TAG_LINE);
                push_point(&mut out, p);
            }
            PathCommand::QuadTo { control, to } => {
                out.push(TAG_QUAD);
                push_point(&mut out, control);
                push_point(&mut out, to);
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => {
                out.push(TAG_CUBIC);
                push_point(&mut out, control1);
                push_point(&mut out, control2);
                push_point(&mut out, to);
            }
            PathCommand::Close => out.push(TAG_CLOSE),
        }
    }
    out
}

fn push_point(out: &mut Vec<u8>, p: Vec2) {
    out.extend_from_slice(&canonical_f32_bits(p.x).to_le_bytes());
    out.extend_from_slice(&canonical_f32_bits(p.y).to_le_bytes());
}

/// Bit pattern of an `f32` with `-0.0` folded to `+0.0` so numerically-equal
/// zeros never split the cache. NaN/Inf are already rejected upstream.
fn canonical_f32_bits(v: f32) -> u32 {
    if v == 0.0 {
        0
    } else {
        v.to_bits()
    }
}

/// Size-independent blob cache for arbitrary vector paths, mirroring
/// [`crate::slug::SlugBlobCache`]. Owns one immutable [`SlugConfig`]; the key is
/// derived from the path content, so callers need not supply one.
#[derive(Debug)]
pub struct VectorPathBlobCache {
    config: SlugConfig,
    blobs: HashMap<VectorPathKey, Arc<SlugGlyph>>,
    hits: u64,
    misses: u64,
}

impl VectorPathBlobCache {
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

    /// Encode `path`, keyed by its canonical content. On a hit the stored blob
    /// is reused (hit counter up); on a miss it is encoded, stored, and the miss
    /// counter increments. An errored encode stores nothing and moves no
    /// counter — the cache is never poisoned.
    pub fn encode(&mut self, path: &VectorPath) -> Result<Arc<SlugGlyph>, VectorPathEncodeError> {
        let key = VectorPathKey::from_path(path)?;
        if let Some(blob) = self.blobs.get(&key) {
            self.hits += 1;
            return Ok(Arc::clone(blob));
        }
        let glyph = encode_vector_path(path, &self.config)?;
        let blob = Arc::new(glyph);
        self.blobs.insert(key, Arc::clone(&blob));
        self.misses += 1;
        Ok(blob)
    }

    /// Look up an already-encoded blob without encoding or touching counters.
    pub fn get(&self, key: &VectorPathKey) -> Option<Arc<SlugGlyph>> {
        self.blobs.get(key).map(Arc::clone)
    }

    pub fn hits(&self) -> u64 {
        self.hits
    }

    pub fn misses(&self) -> u64 {
        self.misses
    }

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
    use crate::path::{Affine2, Bounds, FillRule};

    fn path(commands: Vec<PathCommand>) -> VectorPath {
        // Bounds are intentionally bogus — the arbitrary-path encoder derives its
        // own from the flattened geometry and must not depend on this field.
        VectorPath::new(
            commands,
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        )
    }

    fn config() -> SlugConfig {
        SlugConfig::new(2, 2, 1)
    }

    #[test]
    fn moveto_lineto_close_semantics() {
        // A triangle: two explicit lines plus the implicit closing line.
        let glyph = encode_vector_path(
            &path(vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::LineTo(Vec2::new(100.0, 0.0)),
                PathCommand::LineTo(Vec2::new(0.0, 100.0)),
                PathCommand::Close,
            ]),
            &config(),
        )
        .unwrap();
        assert_eq!(glyph.curves.len(), 3);
        // Every line carries the duplicated-endpoint sentinel.
        for c in &glyph.curves {
            assert_eq!(c.p1, c.p2);
        }
        // Bounds derive from geometry, not the (zeroed) VectorPath.bounds.
        assert_eq!(glyph.bounds.x_max, 100.0);
        assert_eq!(glyph.bounds.y_max, 100.0);
    }

    #[test]
    fn quadto_is_preserved_as_a_real_quadratic() {
        let glyph = encode_vector_path(
            &path(vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::QuadTo {
                    control: Vec2::new(50.0, 100.0),
                    to: Vec2::new(100.0, 0.0),
                },
            ]),
            &config(),
        )
        .unwrap();
        assert_eq!(glyph.curves.len(), 1);
        let c = glyph.curves[0];
        assert_eq!(c.p1, Vec2::new(50.0, 100.0));
        assert_ne!(
            c.p1, c.p2,
            "a real quadratic keeps a distinct control point"
        );
    }

    #[test]
    fn cubicto_is_subdivided_into_quadratics() {
        // The glyph encoder rejects this exact input; the general encoder must
        // accept it and lower the cubic to one-or-more quadratics.
        let glyph = encode_vector_path(
            &path(vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::CubicTo {
                    control1: Vec2::new(0.0, 100.0),
                    control2: Vec2::new(100.0, 100.0),
                    to: Vec2::new(100.0, 0.0),
                },
            ]),
            &config(),
        )
        .unwrap();
        assert!(
            !glyph.curves.is_empty(),
            "cubic lowered to at least one quadratic"
        );
        // The chain starts at the move point and ends at the cubic endpoint.
        assert_eq!(glyph.curves.first().unwrap().p0, Vec2::new(0.0, 0.0));
        assert_eq!(glyph.curves.last().unwrap().p2, Vec2::new(100.0, 0.0));
    }

    #[test]
    fn empty_path_is_rejected() {
        let err = encode_vector_path(
            &path(vec![PathCommand::MoveTo(Vec2::ZERO), PathCommand::Close]),
            &config(),
        )
        .unwrap_err();
        assert_eq!(err, VectorPathEncodeError::EmptyPath);
    }

    #[test]
    fn nan_and_inf_are_rejected_at_the_boundary() {
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let err = encode_vector_path(
                &path(vec![
                    PathCommand::MoveTo(Vec2::ZERO),
                    PathCommand::LineTo(Vec2::new(bad, 10.0)),
                ]),
                &config(),
            )
            .unwrap_err();
            assert_eq!(err, VectorPathEncodeError::NonFinite);
        }
    }

    #[test]
    fn re_encoding_is_byte_identical() {
        let p = path(vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::CubicTo {
                control1: Vec2::new(0.0, 100.0),
                control2: Vec2::new(100.0, 100.0),
                to: Vec2::new(100.0, 0.0),
            },
            PathCommand::Close,
        ]);
        let a = encode_vector_path(&p, &config()).unwrap();
        let b = encode_vector_path(&p, &config()).unwrap();
        assert_eq!(a.to_le_bytes(), b.to_le_bytes());
    }

    #[test]
    fn key_is_byte_identical_across_threads() {
        let build = || {
            path(vec![
                PathCommand::MoveTo(Vec2::new(1.5, -2.0)),
                PathCommand::QuadTo {
                    control: Vec2::new(3.0, 4.0),
                    to: Vec2::new(5.0, 6.0),
                },
                PathCommand::CubicTo {
                    control1: Vec2::new(7.0, 8.0),
                    control2: Vec2::new(9.0, 10.0),
                    to: Vec2::new(11.0, 12.0),
                },
                PathCommand::Close,
            ])
        };
        let a = std::thread::spawn(move || VectorPathKey::from_path(&build()).unwrap());
        let b = std::thread::spawn(move || VectorPathKey::from_path(&build()).unwrap());
        let ka = a.join().unwrap();
        let kb = b.join().unwrap();
        assert_eq!(ka, kb);
        assert_eq!(ka.as_bytes(), kb.as_bytes());
    }

    #[test]
    fn distinct_geometry_produces_distinct_keys() {
        let a = VectorPathKey::from_path(&path(vec![
            PathCommand::MoveTo(Vec2::ZERO),
            PathCommand::LineTo(Vec2::new(1.0, 0.0)),
        ]))
        .unwrap();
        let b = VectorPathKey::from_path(&path(vec![
            PathCommand::MoveTo(Vec2::ZERO),
            PathCommand::LineTo(Vec2::new(2.0, 0.0)),
        ]))
        .unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn negative_zero_does_not_split_the_key() {
        let pos = VectorPathKey::from_path(&path(vec![PathCommand::MoveTo(Vec2::new(0.0, 0.0))]))
            .unwrap();
        let neg = VectorPathKey::from_path(&path(vec![PathCommand::MoveTo(Vec2::new(-0.0, -0.0))]))
            .unwrap();
        assert_eq!(pos, neg, "-0.0 and +0.0 are the same logical point");
    }

    // ---- Fill rule + transform (#304) --------------------------------------

    #[test]
    fn fill_rule_is_carried_through_to_the_encoded_blob() {
        let triangle = vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::LineTo(Vec2::new(10.0, 0.0)),
            PathCommand::LineTo(Vec2::new(0.0, 10.0)),
            PathCommand::Close,
        ];
        let nz = VectorPath::new(
            triangle.clone(),
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        let eo = VectorPath::new(
            triangle,
            FillRule::EvenOdd,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        assert_eq!(
            encode_vector_path(&nz, &config()).unwrap().fill_rule,
            FillRule::NonZero
        );
        assert_eq!(
            encode_vector_path(&eo, &config()).unwrap().fill_rule,
            FillRule::EvenOdd
        );
    }

    /// The real even-odd-vs-non-zero *rasterization* decision (does a
    /// same-winding nested contour paint a hole or stay filled) happens on
    /// the GPU — this encoder only carries `fill_rule` through to the blob
    /// (see the test above) and leaves the curve/band records themselves
    /// rule-agnostic. The pixel-level regression test for nested contours
    /// lives in `mkui-wgpu`'s `gpu-tests,slug`-gated Lavapipe suite
    /// (`render::vector_path_fill_rule_gpu`), which this test cross-references
    /// so the two don't drift apart.
    #[test]
    fn nested_same_winding_contours_still_encode_as_two_full_contours() {
        // Two same-winding (both CCW) nested squares: outer 0..100, inner
        // 25..75. Neither the outer nor inner contour is dropped or merged —
        // both the non-zero *and* even-odd cases need every edge present;
        // only the GPU rasterizer's winding test differs.
        let nested = vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::LineTo(Vec2::new(100.0, 0.0)),
            PathCommand::LineTo(Vec2::new(100.0, 100.0)),
            PathCommand::LineTo(Vec2::new(0.0, 100.0)),
            PathCommand::Close,
            PathCommand::MoveTo(Vec2::new(25.0, 25.0)),
            PathCommand::LineTo(Vec2::new(75.0, 25.0)),
            PathCommand::LineTo(Vec2::new(75.0, 75.0)),
            PathCommand::LineTo(Vec2::new(25.0, 75.0)),
            PathCommand::Close,
        ];
        let eo = path_with_fill(nested, FillRule::EvenOdd);
        let glyph = encode_vector_path(&eo, &config()).unwrap();
        // 4 edges per square (3 explicit `LineTo`s plus the 4th edge `Close`
        // draws back to the contour's start) = 8 curves total.
        assert_eq!(glyph.curves.len(), 8);
        assert_eq!(glyph.fill_rule, FillRule::EvenOdd);
    }

    fn path_with_fill(commands: Vec<PathCommand>, fill: FillRule) -> VectorPath {
        VectorPath::new(commands, fill, Bounds::new(Vec2::ZERO, Vec2::ZERO))
    }

    fn with_transform(mut p: VectorPath, transform: Affine2) -> VectorPath {
        p.transform = transform;
        p
    }

    #[test]
    fn transform_translates_every_point_before_bounds_are_derived() {
        let triangle = vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::LineTo(Vec2::new(10.0, 0.0)),
            PathCommand::LineTo(Vec2::new(0.0, 10.0)),
            PathCommand::Close,
        ];
        let translated = with_transform(
            path(triangle),
            Affine2 {
                tx: 100.0,
                ty: 200.0,
                ..Affine2::IDENTITY
            },
        );
        let glyph = encode_vector_path(&translated, &config()).unwrap();
        assert_eq!(glyph.bounds.x_min, 100.0);
        assert_eq!(glyph.bounds.y_min, 200.0);
        assert_eq!(glyph.bounds.x_max, 110.0);
        assert_eq!(glyph.bounds.y_max, 210.0);
    }

    #[test]
    fn transform_scales_cubic_control_points_before_subdivision() {
        // A 2x uniform scale applied to a cubic: the transformed endpoint
        // must land at (200, 0), not (100, 0) — proving the transform is
        // applied to cubic control points too, before `subdivide_cubic` ever
        // sees them (not re-scaled after flattening).
        let cubic = with_transform(
            path(vec![
                PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
                PathCommand::CubicTo {
                    control1: Vec2::new(0.0, 50.0),
                    control2: Vec2::new(100.0, 50.0),
                    to: Vec2::new(100.0, 0.0),
                },
            ]),
            Affine2 {
                a: 2.0,
                d: 2.0,
                ..Affine2::IDENTITY
            },
        );
        let glyph = encode_vector_path(&cubic, &config()).unwrap();
        assert_eq!(glyph.curves.first().unwrap().p0, Vec2::new(0.0, 0.0));
        assert_eq!(glyph.curves.last().unwrap().p2, Vec2::new(200.0, 0.0));
    }

    #[test]
    fn non_finite_transform_field_is_rejected() {
        let bad = with_transform(
            path(vec![
                PathCommand::MoveTo(Vec2::ZERO),
                PathCommand::LineTo(Vec2::new(1.0, 0.0)),
            ]),
            Affine2 {
                a: f32::NAN,
                ..Affine2::IDENTITY
            },
        );
        assert_eq!(
            encode_vector_path(&bad, &config()).unwrap_err(),
            VectorPathEncodeError::NonFinite
        );
    }

    #[test]
    fn a_transform_that_overflows_a_point_to_infinity_is_rejected() {
        let overflowing = with_transform(
            path(vec![
                PathCommand::MoveTo(Vec2::ZERO),
                PathCommand::LineTo(Vec2::new(f32::MAX, 0.0)),
            ]),
            Affine2 {
                a: 2.0,
                ..Affine2::IDENTITY
            },
        );
        assert_eq!(
            encode_vector_path(&overflowing, &config()).unwrap_err(),
            VectorPathEncodeError::NonFinite
        );
    }

    #[test]
    fn singular_transform_is_rejected() {
        // Rank-1 linear part: every point collapses onto the line y = 2x.
        let singular = with_transform(
            path(vec![
                PathCommand::MoveTo(Vec2::ZERO),
                PathCommand::LineTo(Vec2::new(1.0, 0.0)),
            ]),
            Affine2 {
                a: 1.0,
                b: 2.0,
                c: 2.0,
                d: 4.0,
                tx: 0.0,
                ty: 0.0,
            },
        );
        assert_eq!(
            encode_vector_path(&singular, &config()).unwrap_err(),
            VectorPathEncodeError::SingularTransform
        );
    }

    #[test]
    fn distinct_fill_rules_produce_distinct_keys() {
        let cmds = vec![
            PathCommand::MoveTo(Vec2::ZERO),
            PathCommand::LineTo(Vec2::new(1.0, 0.0)),
        ];
        let nz =
            VectorPathKey::from_path(&path_with_fill(cmds.clone(), FillRule::NonZero)).unwrap();
        let eo = VectorPathKey::from_path(&path_with_fill(cmds, FillRule::EvenOdd)).unwrap();
        assert_ne!(nz, eo);
    }

    #[test]
    fn distinct_transforms_produce_distinct_keys() {
        let cmds = vec![
            PathCommand::MoveTo(Vec2::ZERO),
            PathCommand::LineTo(Vec2::new(1.0, 0.0)),
        ];
        let identity = VectorPathKey::from_path(&path(cmds.clone())).unwrap();
        let translated = VectorPathKey::from_path(&with_transform(
            path(cmds),
            Affine2 {
                tx: 5.0,
                ..Affine2::IDENTITY
            },
        ))
        .unwrap();
        assert_ne!(identity, translated);
    }

    #[test]
    fn cache_hits_reuse_one_blob() {
        let mut cache = VectorPathBlobCache::new(config());
        let p = path(vec![
            PathCommand::MoveTo(Vec2::new(0.0, 0.0)),
            PathCommand::LineTo(Vec2::new(10.0, 0.0)),
            PathCommand::LineTo(Vec2::new(0.0, 10.0)),
            PathCommand::Close,
        ]);
        let first = cache.encode(&p).unwrap();
        let second = cache.encode(&p).unwrap();
        assert_eq!(cache.hits(), 1);
        assert_eq!(cache.misses(), 1);
        assert_eq!(cache.len(), 1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn errored_encode_does_not_poison_cache() {
        let mut cache = VectorPathBlobCache::new(config());
        let bad = path(vec![
            PathCommand::MoveTo(Vec2::ZERO),
            PathCommand::LineTo(Vec2::new(f32::NAN, 0.0)),
        ]);
        assert_eq!(
            cache.encode(&bad).unwrap_err(),
            VectorPathEncodeError::NonFinite
        );
        assert!(cache.is_empty());
        assert_eq!(cache.misses(), 0);
    }
}
