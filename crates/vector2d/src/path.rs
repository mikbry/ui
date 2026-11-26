//! Backend-neutral 2D path model.
//!
//! A [`VectorPath`] is a renderer-independent description of filled vector
//! geometry: a sequence of [`PathCommand`]s plus a [`FillRule`], a path-local
//! [`Affine2`] transform, and a [`Bounds`] rectangle. The glyph lane (see
//! [`crate::outline`], [`crate::slug`]) always carries [`FillRule::NonZero`]
//! and [`Affine2::IDENTITY`] — a resolved outline arrives pre-transformed and
//! is never re-transformed there. The general path lane (see [`crate::encode`])
//! honours both fields: [`FillRule::EvenOdd`] actually punches a hole in
//! same-winding nested contours, and a non-identity `transform` moves, scales,
//! rotates, or skews the encoded geometry (#304). No GPU type ever appears in
//! this module.
//!
//! Coordinates are `f32` in the source geometry's own units. For glyph
//! outlines that is *font units, y-up* (see [`crate::GlyphOutline`]).

/// A 2D point/vector in path space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Fill winding rule for a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum FillRule {
    /// Non-zero winding (the TrueType/PostScript glyph convention).
    #[default]
    NonZero,
    /// Even-odd winding.
    EvenOdd,
}

/// One drawing command in a [`VectorPath`].
///
/// Cubic segments are *representable* so the model can carry general curves,
/// but the Sprint 7 Slug glyph encoder rejects them with an explicit
/// unsupported-segment error (see [`crate::slug`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum PathCommand {
    /// Begin a new contour at `to`.
    MoveTo(Vec2),
    /// Straight line from the current point to `to`.
    LineTo(Vec2),
    /// Quadratic Bézier with one off-curve `control` to `to`.
    QuadTo { control: Vec2, to: Vec2 },
    /// Cubic Bézier with two off-curve controls to `to`. Representable but not
    /// encodable by the Sprint 7 glyph lane.
    CubicTo {
        control1: Vec2,
        control2: Vec2,
        to: Vec2,
    },
    /// Close the current contour back to its start point.
    Close,
}

/// An axis-aligned bounding rectangle in path space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min: Vec2,
    pub max: Vec2,
}

impl Bounds {
    pub const fn new(min: Vec2, max: Vec2) -> Self {
        Self { min, max }
    }

    pub fn width(&self) -> f32 {
        self.max.x - self.min.x
    }

    pub fn height(&self) -> f32 {
        self.max.y - self.min.y
    }

    /// Whether the rectangle has zero or negative area in either axis.
    pub fn is_degenerate(&self) -> bool {
        self.width() <= 0.0 || self.height() <= 0.0
    }
}

/// A path-local 2×3 affine transform in `f32` (distinct from the canonical
/// fixed-point [`mkui_text::Affine2Fixed`] used in cache keys). The glyph lane
/// (see [`crate::slug::encode_slug_glyph`]) requires this to stay
/// [`Affine2::IDENTITY`] — a resolved outline arrives pre-transformed and is
/// never re-transformed there, and that encoder rejects anything else with a
/// typed error. The general path lane
/// ([`crate::encode::encode_vector_path`]/[`crate::stroke_expand::stroke_to_fill`],
/// #304) applies this transform to the authored points, in the path's own
/// *local*, y-up coordinate space, before any GPU placement: authored point →
/// `Affine2` (this struct) → `origin_px` + `scale_px_per_unit` placement
/// (`mkui-vector2d-wgpu`) → the adapter's y-flip into screen space. `a b c d
/// tx ty` match the six free parameters of an SVG `matrix(a b c d e f)`
/// transform algebraically (`e`/`f` are this struct's `tx`/`ty`) — see
/// [`Self::transform_point`] — but SVG's own coordinate space is y-*down*
/// while this one is y-*up*. A caller importing an authored SVG `transform`
/// (#201) must conjugate it with that y-flip (negate `b`, `c`, and `ty`) or a
/// `rotate`/`skewX`/`skewY` comes out mirrored — e.g. SVG's `rotate(θ)` is
/// this struct's `rotate(−θ)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine2 {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine2 {
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// Apply the transform to a point: `(a·x + c·y + tx, b·x + d·y + ty)`,
    /// i.e. SVG's `matrix(a, b, c, d, tx, ty)` convention.
    pub fn transform_point(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }

    /// The determinant of the transform's linear part, `a·d − b·c`. Zero
    /// means the mapping collapses every point onto a line or a point: no
    /// interior survives under either fill rule, and the mapping has no
    /// inverse. `encode_vector_path` rejects a path whose transform has a
    /// zero determinant with a typed error rather than producing degenerate
    /// geometry.
    pub fn determinant(&self) -> f32 {
        self.a * self.d - self.b * self.c
    }
}

impl Default for Affine2 {
    fn default() -> Self {
        Self::IDENTITY
    }
}

/// A backend-neutral filled vector path.
///
/// `bounds` is in the same **authored, pre-transform** space as `commands` —
/// it is never re-derived from `transform` (#304). The glyph lane trusts it
/// verbatim as the provider's resolved ink box (and requires `transform` to
/// stay [`Affine2::IDENTITY`], so the distinction is moot there); the general
/// path lane ([`crate::encode::encode_vector_path`]) ignores this field
/// entirely and derives its own bounds from the *transformed* geometry. A
/// caller that reads `bounds` directly to approximate a screen-space box
/// (rather than going through the encoder) must apply `transform` to it
/// first if `transform` is not the identity.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorPath {
    pub commands: Vec<PathCommand>,
    pub fill: FillRule,
    pub transform: Affine2,
    pub bounds: Bounds,
}

impl VectorPath {
    /// Construct a path from already-resolved geometry, with `transform` at
    /// [`Affine2::IDENTITY`]. `bounds` is taken as authoritative in that same
    /// authored space (the glyph lane copies the provider's resolved ink
    /// bounds) and is **not** recomputed from `commands` or re-derived from a
    /// `transform` set afterward — see the struct docs.
    pub fn new(commands: Vec<PathCommand>, fill: FillRule, bounds: Bounds) -> Self {
        Self {
            commands,
            fill,
            transform: Affine2::IDENTITY,
            bounds,
        }
    }

    /// Whether the path draws nothing (no on-curve geometry).
    pub fn is_empty(&self) -> bool {
        !self.commands.iter().any(|c| {
            matches!(
                c,
                PathCommand::LineTo(_) | PathCommand::QuadTo { .. } | PathCommand::CubicTo { .. }
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_path_detection() {
        let p = VectorPath::new(
            vec![PathCommand::MoveTo(Vec2::ZERO), PathCommand::Close],
            FillRule::NonZero,
            Bounds::new(Vec2::ZERO, Vec2::ZERO),
        );
        assert!(p.is_empty());
    }

    #[test]
    fn affine_identity_is_a_noop() {
        let p = Vec2::new(3.0, -2.5);
        assert_eq!(Affine2::IDENTITY.transform_point(p), p);
    }

    #[test]
    fn determinant_of_identity_is_one() {
        assert_eq!(Affine2::IDENTITY.determinant(), 1.0);
    }

    #[test]
    fn determinant_is_zero_for_a_collapsing_transform() {
        // Every point lands on the line y = 2x: the linear part has rank 1.
        let collapsing = Affine2 {
            a: 1.0,
            b: 2.0,
            c: 2.0,
            d: 4.0,
            tx: 0.0,
            ty: 0.0,
        };
        assert_eq!(collapsing.determinant(), 0.0);
    }
}
