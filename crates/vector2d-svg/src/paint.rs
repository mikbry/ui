//! What paints a shape's fill or stroke (#201): nothing, a solid color, or a
//! gradient reference.

use crate::color::Color;
use crate::gradient::Gradient;

/// A `fill`/`stroke` attribute value before gradient references are
/// resolved against the document's `<defs>`.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintRef {
    /// `fill="none"` / `stroke="none"`.
    None,
    Solid(Color),
    /// `url(#id)` — resolved against the document's gradients in
    /// [`crate::document`]; an unknown `id` is a typed
    /// [`crate::SvgError::UnknownPaintReference`].
    GradientRef(String),
}

/// A fully-resolved paint, ready to hand to a renderer.
#[derive(Debug, Clone, PartialEq)]
pub enum Paint {
    Solid(Color),
    Gradient(Gradient),
}

/// Parse a `fill`/`stroke` presentation-attribute value: `none`, a color
/// (see [`crate::color::parse_color`]), or `url(#id)`. No fallback color
/// after `url(...)` (`"url(#a) red"`) and no `currentColor` — both outside
/// the subset, so any trailing content after `url(#id)` is rejected rather
/// than ignored.
pub fn parse_paint_ref(s: &str) -> Option<PaintRef> {
    let s = s.trim();
    if s == "none" {
        return Some(PaintRef::None);
    }
    if let Some(rest) = s.strip_prefix("url(") {
        let inner = rest.strip_suffix(')')?;
        let id = inner.strip_prefix('#')?;
        if id.is_empty() {
            return None;
        }
        return Some(PaintRef::GradientRef(id.to_string()));
    }
    crate::color::parse_color(s).map(PaintRef::Solid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn none_keyword() {
        assert_eq!(parse_paint_ref("none"), Some(PaintRef::None));
    }

    #[test]
    fn solid_color() {
        assert_eq!(
            parse_paint_ref("#ff0000"),
            Some(PaintRef::Solid(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            }))
        );
    }

    #[test]
    fn gradient_url_reference() {
        assert_eq!(
            parse_paint_ref("url(#grad1)"),
            Some(PaintRef::GradientRef("grad1".to_string()))
        );
    }

    #[test]
    fn fallback_color_after_url_is_rejected() {
        assert_eq!(parse_paint_ref("url(#a) red"), None);
    }

    #[test]
    fn current_color_is_rejected() {
        assert_eq!(parse_paint_ref("currentColor"), None);
    }
}
