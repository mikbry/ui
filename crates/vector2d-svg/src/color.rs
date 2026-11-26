//! Hand-written color value parser for the sanitized subset (#201).
//!
//! Only two syntaxes are accepted: hex (`#rgb`, `#rrggbb`) and functional
//! `rgb()`/`rgba()`. Per the issue's own scoping note, the full CSS
//! named-color table is *not* included — it is far more surface than the
//! subset needs and is cheap to add later if a representative asset turns
//! out to need it (deferred, not forgotten).

/// A straight-alpha color in `[0.0, 1.0]` per channel, matching the
/// convention `mkui_wgpu::types::Color` already uses for its RGBA fields
/// (this crate does not depend on `mkui-wgpu`, so it defines its own).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const BLACK: Self = Self {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        a: 1.0,
    };

    /// Return this color with its alpha multiplied by `opacity`, clamped to
    /// `[0, 1]`. Used to fold an element's/group's accumulated `opacity`
    /// into a resolved paint color.
    pub fn with_opacity_multiplied(self, opacity: f32) -> Self {
        Self {
            a: (self.a * opacity.clamp(0.0, 1.0)).clamp(0.0, 1.0),
            ..self
        }
    }
}

/// Parse a CSS/SVG color value: `#rgb`, `#rrggbb`, `rgb(r, g, b)`, or
/// `rgba(r, g, b, a)`. Channel components may be `0-255` integers or `0-100%`
/// percentages (per-call, components may mix either convention — SVG allows
/// it); alpha may be `0-1` or `0-100%`. Returns `None` on any syntax outside
/// this grammar (callers turn that into a typed [`crate::SvgError`]).
pub fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim();
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex(hex);
    }
    if let Some(inner) = strip_fn(s, "rgba") {
        return parse_rgb_components(inner, true);
    }
    if let Some(inner) = strip_fn(s, "rgb") {
        return parse_rgb_components(inner, false);
    }
    None
}

fn strip_fn<'a>(s: &'a str, name: &str) -> Option<&'a str> {
    let rest = s.strip_prefix(name)?;
    let rest = rest.trim_start();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    Some(inner)
}

fn parse_hex(hex: &str) -> Option<Color> {
    let digit = |c: u8| -> Option<u8> {
        match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            b'A'..=b'F' => Some(c - b'A' + 10),
            _ => None,
        }
    };
    let bytes = hex.as_bytes();
    match bytes.len() {
        3 => {
            let r = digit(bytes[0])?;
            let g = digit(bytes[1])?;
            let b = digit(bytes[2])?;
            Some(Color {
                r: (r * 17) as f32 / 255.0,
                g: (g * 17) as f32 / 255.0,
                b: (b * 17) as f32 / 255.0,
                a: 1.0,
            })
        }
        6 => {
            let byte =
                |i: usize| -> Option<u8> { Some(digit(bytes[i])? * 16 + digit(bytes[i + 1])?) };
            Some(Color {
                r: byte(0)? as f32 / 255.0,
                g: byte(2)? as f32 / 255.0,
                b: byte(4)? as f32 / 255.0,
                a: 1.0,
            })
        }
        _ => None,
    }
}

/// Parse the comma-separated component list inside `rgb(...)`/`rgba(...)`.
/// `with_alpha` selects whether a 4th (alpha) component is required.
fn parse_rgb_components(inner: &str, with_alpha: bool) -> Option<Color> {
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    let expected = if with_alpha { 4 } else { 3 };
    if parts.len() != expected {
        return None;
    }
    let channel = |p: &str| -> Option<f32> {
        if let Some(pct) = p.strip_suffix('%') {
            Some((crate::number::parse_single_number(pct)? / 100.0).clamp(0.0, 1.0))
        } else {
            Some((crate::number::parse_single_number(p)? / 255.0).clamp(0.0, 1.0))
        }
    };
    let alpha = |p: &str| -> Option<f32> {
        if let Some(pct) = p.strip_suffix('%') {
            Some((crate::number::parse_single_number(pct)? / 100.0).clamp(0.0, 1.0))
        } else {
            Some(crate::number::parse_single_number(p)?.clamp(0.0, 1.0))
        }
    };
    let r = channel(parts[0])?;
    let g = channel(parts[1])?;
    let b = channel(parts[2])?;
    let a = if with_alpha { alpha(parts[3])? } else { 1.0 };
    Some(Color { r, g, b, a })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hex() {
        assert_eq!(
            parse_color("#f00"),
            Some(Color {
                r: 1.0,
                g: 0.0,
                b: 0.0,
                a: 1.0
            })
        );
    }

    #[test]
    fn long_hex() {
        let c = parse_color("#336699").unwrap();
        assert!((c.r - 0x33 as f32 / 255.0).abs() < 1e-6);
        assert!((c.g - 0x66 as f32 / 255.0).abs() < 1e-6);
        assert!((c.b - 0x99 as f32 / 255.0).abs() < 1e-6);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn rgb_function() {
        let c = parse_color("rgb(255, 0, 128)").unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
        assert!((c.b - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(c.a, 1.0);
    }

    #[test]
    fn rgba_function_with_percent_alpha() {
        let c = parse_color("rgba(0, 0, 0, 50%)").unwrap();
        assert_eq!(c.a, 0.5);
    }

    #[test]
    fn rgb_function_with_percent_channels() {
        let c = parse_color("rgb(100%, 0%, 0%)").unwrap();
        assert_eq!(c.r, 1.0);
        assert_eq!(c.g, 0.0);
    }

    #[test]
    fn named_colors_are_rejected() {
        // Deliberately not in the subset (scope decision, see module docs).
        assert_eq!(parse_color("red"), None);
    }

    #[test]
    fn malformed_hex_is_rejected() {
        assert_eq!(parse_color("#zzz"), None);
        assert_eq!(parse_color("#12345"), None);
    }

    #[test]
    fn wrong_arity_rgb_is_rejected() {
        assert_eq!(parse_color("rgb(1, 2)"), None);
        assert_eq!(parse_color("rgba(1, 2, 3)"), None);
    }

    #[test]
    fn non_finite_channel_or_alpha_is_rejected() {
        assert_eq!(parse_color("rgb(nan, 0, 0)"), None);
        assert_eq!(parse_color("rgba(0, 0, 0, inf)"), None);
        assert_eq!(parse_color("rgb(inf%, 0, 0)"), None);
    }

    #[test]
    fn opacity_multiplies_alpha_and_clamps() {
        let c = Color::BLACK.with_opacity_multiplied(0.5);
        assert_eq!(c.a, 0.5);
        let c2 = Color::BLACK.with_opacity_multiplied(2.0);
        assert_eq!(c2.a, 1.0);
    }
}
