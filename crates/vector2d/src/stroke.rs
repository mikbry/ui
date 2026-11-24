//! Backend-neutral stroke descriptor.
//!
//! A [`Stroke`] is a **value-type descriptor** held *alongside* a
//! [`crate::path::VectorPath`] — it describes how the path's outline would be
//! painted (width, end caps, corner joins, dashing) without itself carrying any
//! geometry. Stroke *expansion* (turning a stroked path into a fillable
//! outline) is a Sprint 9+ concern; Wave 1 (#137) only ratifies the descriptor
//! so downstream crates can carry it. No GPU type ever appears here.
//!
//! Following the Sprint 8 §2.1 value-type-first decision this module ships no
//! builder — a [`Stroke`] is a plain struct with public fields.

/// How the two ends of an open stroked subpath are drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LineCap {
    /// Squared off exactly at the endpoint (no extension).
    #[default]
    Butt,
    /// A semicircle of radius `width_px / 2` centred on the endpoint.
    Round,
    /// Squared off, extended past the endpoint by `width_px / 2`.
    Square,
}

/// How two stroked segments are joined at a shared vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LineJoin {
    /// Sharp corner — outer edges extended until they meet.
    #[default]
    Miter,
    /// Rounded corner — a circular arc of radius `width_px / 2`.
    Round,
    /// Flattened corner — the outer edges joined by a straight bevel.
    Bevel,
}

/// A dash pattern: alternating on/off run lengths (in pixels) plus a phase
/// offset. `intervals` is read cyclically starting from `offset`; an empty
/// `intervals` list means "no dashing" (a solid stroke).
#[derive(Debug, Clone, PartialEq)]
pub struct DashPattern {
    /// Alternating on/off lengths in pixels: `[on, off, on, off, …]`.
    pub intervals: Vec<f32>,
    /// Distance into the pattern at which drawing begins, in pixels.
    pub offset: f32,
}

impl DashPattern {
    /// Construct a dash pattern from on/off intervals and a phase offset.
    pub fn new(intervals: Vec<f32>, offset: f32) -> Self {
        Self { intervals, offset }
    }

    /// Whether every length in the pattern is finite (no NaN/Inf).
    pub fn is_finite(&self) -> bool {
        self.offset.is_finite() && self.intervals.iter().all(|v| v.is_finite())
    }
}

/// A stroke descriptor carried alongside a [`crate::path::VectorPath`].
///
/// This is a pure description; it holds no tessellated geometry. Widths are in
/// pixels (device-independent length is the caller's concern), matching the
/// screen-space intent of a stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// Total stroke width in pixels (the pen diameter).
    pub width_px: f32,
    /// End-cap style for open subpaths.
    pub cap: LineCap,
    /// Corner-join style between segments.
    pub join: LineJoin,
    /// Optional dash pattern; `None` is a solid stroke.
    pub dash: Option<DashPattern>,
}

impl Stroke {
    /// A solid stroke of the given width with default (butt) caps and (miter)
    /// joins.
    pub fn new(width_px: f32) -> Self {
        Self {
            width_px,
            cap: LineCap::default(),
            join: LineJoin::default(),
            dash: None,
        }
    }

    /// Whether every numeric field is finite (no NaN/Inf), including the dash
    /// pattern when present.
    pub fn is_finite(&self) -> bool {
        self.width_px.is_finite() && self.dash.as_ref().is_none_or(DashPattern::is_finite)
    }
}

impl Default for Stroke {
    /// A 1-pixel solid butt/miter stroke.
    fn default() -> Self {
        Self::new(1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_butt_miter_solid_one_px() {
        let s = Stroke::default();
        assert_eq!(s.width_px, 1.0);
        assert_eq!(s.cap, LineCap::Butt);
        assert_eq!(s.join, LineJoin::Miter);
        assert!(s.dash.is_none());
        assert!(s.is_finite());
    }

    #[test]
    fn descriptor_carries_caps_joins_and_dash() {
        let s = Stroke {
            width_px: 4.0,
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            dash: Some(DashPattern::new(vec![6.0, 2.0], 1.0)),
        };
        assert_eq!(s.cap, LineCap::Round);
        assert_eq!(s.join, LineJoin::Bevel);
        assert_eq!(s.dash.as_ref().unwrap().intervals, vec![6.0, 2.0]);
        assert_eq!(s.dash.as_ref().unwrap().offset, 1.0);
        assert!(s.is_finite());
    }

    #[test]
    fn non_finite_fields_are_detected() {
        assert!(!Stroke::new(f32::NAN).is_finite());
        assert!(!Stroke::new(f32::INFINITY).is_finite());
        let dashed = Stroke {
            dash: Some(DashPattern::new(vec![1.0, f32::NAN], 0.0)),
            ..Stroke::new(2.0)
        };
        assert!(!dashed.is_finite());
    }
}
