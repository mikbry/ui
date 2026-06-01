//! Generic shadcn-aligned `Badge` atom.
//!
//! Non-interactive label chip used for status signals on rows and inline
//! markers. Four variants (`Default`, `Destructive`, `Outline`,
//! `Secondary`) and two sizes (`Default`, `Sm`) — product-specific
//! signals (state pills, role tags, tier markers) compose this atom in
//! downstream crates and do **not** extend the enum.

use crate::theme::{BadgeSize, BadgeVariant, WgpuTheme};
use crate::types::{CornerRadii, Point, Quad, Rect, Scene, Size, Stroke, Text, TextStyle};

/// Emit a badge into `scene` at `rect`. The caller picks the rect so the
/// atom can be laid out by the parent (table row, header strip, etc.);
/// `variant` + `size` resolve through [`WgpuTheme::badge_style`].
pub fn badge(
    scene: &mut Scene,
    rect: Rect,
    label: impl Into<String>,
    variant: BadgeVariant,
    size: BadgeSize,
    theme: &WgpuTheme,
) {
    let style = theme.badge_style(variant, size);

    let stroke = if style.stroke_width > 0.0 {
        Some(Stroke {
            color: style.stroke,
            width: style.stroke_width,
        })
    } else {
        None
    };

    scene.quad(Quad {
        rect,
        fill: style.fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke,
    });

    let label_style = TextStyle {
        color: style.label_color,
        ..style.label_style
    };
    let label_height = label_style.line_height_px;
    let label_rect = Rect::new(
        Point::new(
            rect.origin.x + style.padding.left,
            rect.origin.y + (rect.size.height - label_height) * 0.5,
        ),
        Size::new(
            (rect.size.width - style.padding.left - style.padding.right).max(0.0),
            label_height,
        ),
    );
    scene.text(Text {
        rect: label_rect,
        content: label.into(),
        style: label_style,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(200.0, 80.0))
    }

    fn quads(scene: &Scene) -> Vec<&Quad> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect()
    }

    fn texts(scene: &Scene) -> Vec<&Text> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn emits_quad_and_text_for_every_variant_size_combination() {
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(60.0, 22.0));
        for variant in [
            BadgeVariant::Default,
            BadgeVariant::Destructive,
            BadgeVariant::Outline,
            BadgeVariant::Secondary,
            BadgeVariant::Ghost,
            BadgeVariant::Link,
        ] {
            for size in [BadgeSize::Default, BadgeSize::Sm] {
                let mut s = scene();
                badge(&mut s, rect, "v1.0", variant, size, &theme);
                assert_eq!(
                    quads(&s).len(),
                    1,
                    "{variant:?}/{size:?} should emit one quad"
                );
                let labels = texts(&s);
                assert_eq!(
                    labels.len(),
                    1,
                    "{variant:?}/{size:?} should emit one text primitive"
                );
                assert_eq!(labels[0].content, "v1.0");
            }
        }
    }

    #[test]
    fn outline_variant_has_stroke_other_variants_do_not() {
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(60.0, 22.0));

        let mut outline = scene();
        badge(
            &mut outline,
            rect,
            "x",
            BadgeVariant::Outline,
            BadgeSize::Default,
            &theme,
        );
        assert!(quads(&outline)[0].stroke.is_some());

        for filled in [
            BadgeVariant::Default,
            BadgeVariant::Destructive,
            BadgeVariant::Secondary,
            BadgeVariant::Ghost,
            BadgeVariant::Link,
        ] {
            let mut s = scene();
            badge(&mut s, rect, "x", filled, BadgeSize::Default, &theme);
            assert!(
                quads(&s)[0].stroke.is_none(),
                "{filled:?} should not draw a stroke"
            );
        }
    }

    #[test]
    fn ghost_and_link_variants_resolve_to_transparent_chrome() {
        // Smoke: Ghost + Link round-trip through `badge_style` without
        // panicking and resolve to a chrome-less background (alpha 0). The
        // label colour comes through non-zero so the text is still drawn.
        let theme = WgpuTheme::default();
        for variant in [BadgeVariant::Ghost, BadgeVariant::Link] {
            let style = theme.badge_style(variant, BadgeSize::Default);
            assert_eq!(
                style.fill.a, 0.0,
                "{variant:?} should resolve to a transparent fill"
            );
            assert!(
                style.label_color.a > 0.0,
                "{variant:?} should still resolve to a visible label colour"
            );
        }
    }

    #[test]
    fn sm_size_uses_smaller_label_text() {
        let theme = WgpuTheme::default();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(60.0, 22.0));

        let mut default_scene = scene();
        badge(
            &mut default_scene,
            rect,
            "x",
            BadgeVariant::Default,
            BadgeSize::Default,
            &theme,
        );
        let mut sm_scene = scene();
        badge(
            &mut sm_scene,
            rect,
            "x",
            BadgeVariant::Default,
            BadgeSize::Sm,
            &theme,
        );

        let default_label = texts(&default_scene)[0];
        let sm_label = texts(&sm_scene)[0];
        assert!(
            sm_label.style.font_size_px < default_label.style.font_size_px,
            "Sm badge should resolve to a smaller font than Default"
        );
    }
}
