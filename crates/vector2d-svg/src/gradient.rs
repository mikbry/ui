//! Linear/radial gradients with stops (#201).
//!
//! `gradientUnits`/`gradientTransform` are **not** part of the accepted
//! attribute subset (see [`crate::document`] for the per-element allowlist),
//! so every gradient uses the SVG spec default implicitly:
//! `gradientUnits="objectBoundingBox"` — coordinates are fractions of the
//! painted shape's own bounding box, not a free-standing scope decision to
//! skip `gradientUnits` support. A radial gradient on a non-square bbox is
//! therefore a true ellipse (semi-axes `r * bbox.width`, `r * bbox.height`),
//! not an approximation.

use crate::color::Color;

/// One color stop. `offset` is already resolved to a `0.0..=1.0` fraction
/// (from either a bare number or a `%` value) and clamped to be
/// non-decreasing relative to the previous stop (SVG spec: "each gradient
/// offset value is required to be equal to or greater than the previous
/// gradient stop's offset", enforced by clamping rather than rejecting —
/// resvg and browsers do the same rather than failing the whole gradient).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stop {
    pub offset: f32,
    pub color: Color,
}

/// A gradient's geometry, already resolved from `objectBoundingBox`
/// fractions into the painted shape's own local coordinate space (the same
/// pre-[`mkui_vector2d::Affine2`] space as that shape's [`mkui_vector2d::VectorPath::commands`],
/// y-flipped identically — see [`crate::document`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientKind {
    Linear { x1: f32, y1: f32, x2: f32, y2: f32 },
    Radial { cx: f32, cy: f32, rx: f32, ry: f32 },
}

/// A fully-resolved gradient: geometry plus stops, sorted and clamped to a
/// non-decreasing offset sequence.
#[derive(Debug, Clone, PartialEq)]
pub struct Gradient {
    pub kind: GradientKind,
    pub stops: Vec<Stop>,
}

/// Sort stops by authored order (SVG does not allow reordering) and clamp
/// each offset to be `>=` the previous one and within `[0, 1]`.
pub fn normalize_stops(mut stops: Vec<Stop>) -> Vec<Stop> {
    let mut prev = 0.0f32;
    for stop in &mut stops {
        let clamped = stop.offset.clamp(0.0, 1.0).max(prev);
        stop.offset = clamped;
        prev = clamped;
    }
    stops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(offset: f32) -> Stop {
        Stop {
            offset,
            color: Color::BLACK,
        }
    }

    #[test]
    fn decreasing_offsets_are_clamped_to_non_decreasing() {
        let stops = normalize_stops(vec![stop(0.5), stop(0.2), stop(0.9)]);
        assert_eq!(stops[0].offset, 0.5);
        assert_eq!(stops[1].offset, 0.5);
        assert_eq!(stops[2].offset, 0.9);
    }

    #[test]
    fn out_of_range_offsets_are_clamped() {
        let stops = normalize_stops(vec![stop(-1.0), stop(2.0)]);
        assert_eq!(stops[0].offset, 0.0);
        assert_eq!(stops[1].offset, 1.0);
    }
}
