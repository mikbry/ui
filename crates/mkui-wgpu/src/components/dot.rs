//! Generic status `Dot` atom.
//!
//! Tiny coloured pip used as a status pip on table rows and inline state
//! signals. Variant is one of the four status-colour tokens (`Ok`, `Warn`,
//! `Danger`, `Neutral`) — what each means in a given product is decided
//! downstream. An optional `halo` ring at 1.5× diameter and an optional
//! `animation` enum (`None | Pulse | PulseUrgent | Spin`) act as generic
//! visual modifiers; the animation lives on [`Scene::animations`] for the
//! renderer to interpret per-frame.

use crate::theme::{DotSize, DotVariant, WgpuTheme};
use crate::types::{
    CornerRadii, DotAnimation, DotAnimationInstance, Point, Quad, Rect, Scene, Size,
};

/// Emit a dot into `scene` centred at `point`. The atom paints the pip
/// (and optional halo) as quads, and pushes a single
/// [`DotAnimationInstance`] into `scene.animations` when `animation` is
/// non-`None`.
// allow: signature parallels `badge` — `variant` / `size` resolve through
// the theme, then the generic visual modifiers (`halo`, `animation`) follow.
// Bundling them into a param-struct would split the atom layer's call shape
// without clarity gain.
#[allow(clippy::too_many_arguments)]
pub fn dot(
    scene: &mut Scene,
    point: Point,
    variant: DotVariant,
    size: DotSize,
    halo: bool,
    animation: DotAnimation,
    theme: &WgpuTheme,
) {
    let style = theme.dot_style(variant, size);
    let radius = style.diameter * 0.5;

    if halo {
        let halo_diameter = style.diameter * 1.5;
        let halo_origin = Point::new(point.x - halo_diameter * 0.5, point.y - halo_diameter * 0.5);
        scene.quad(Quad {
            rect: Rect::new(halo_origin, Size::new(halo_diameter, halo_diameter)),
            fill: style.halo,
            corner_radii: CornerRadii::all(halo_diameter * 0.5),
            stroke: None,
        });
    }

    let origin = Point::new(point.x - radius, point.y - radius);
    scene.quad(Quad {
        rect: Rect::new(origin, Size::new(style.diameter, style.diameter)),
        fill: style.fill,
        corner_radii: CornerRadii::all(radius),
        stroke: None,
    });

    scene.animate(DotAnimationInstance {
        center: point,
        radius,
        kind: animation,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Primitive, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(100.0, 100.0))
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

    #[test]
    fn emits_one_quad_per_variant_size_combination_without_halo() {
        let theme = WgpuTheme::default();
        for variant in [
            DotVariant::Ok,
            DotVariant::Warn,
            DotVariant::Danger,
            DotVariant::Neutral,
        ] {
            for size in [DotSize::Sm, DotSize::Md] {
                let mut s = scene();
                dot(
                    &mut s,
                    Point::new(50.0, 50.0),
                    variant,
                    size,
                    false,
                    DotAnimation::None,
                    &theme,
                );
                assert_eq!(
                    quads(&s).len(),
                    1,
                    "{variant:?}/{size:?} without halo should emit one quad"
                );
            }
        }
    }

    #[test]
    fn halo_emits_an_extra_quad_at_one_and_a_half_diameter() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(20.0, 20.0),
            DotVariant::Ok,
            DotSize::Md,
            true,
            DotAnimation::None,
            &theme,
        );
        let qs = quads(&s);
        assert_eq!(qs.len(), 2, "halo should add a second quad");
        let dot_size = DotSize::Md.diameter();
        let halo_size = qs[0].rect.size.width;
        let body_size = qs[1].rect.size.width;
        assert!((halo_size - dot_size * 1.5).abs() < 0.001);
        assert!((body_size - dot_size).abs() < 0.001);
        assert!(
            qs[0].fill.a < qs[1].fill.a,
            "halo should be drawn with lower alpha than the body"
        );
    }

    #[test]
    fn variants_resolve_to_distinct_fills() {
        let theme = WgpuTheme::default();
        let mut fills = Vec::new();
        for variant in [
            DotVariant::Ok,
            DotVariant::Warn,
            DotVariant::Danger,
            DotVariant::Neutral,
        ] {
            let mut s = scene();
            dot(
                &mut s,
                Point::new(10.0, 10.0),
                variant,
                DotSize::Md,
                false,
                DotAnimation::None,
                &theme,
            );
            fills.push(quads(&s)[0].fill);
        }
        for (i, a) in fills.iter().enumerate() {
            for b in fills.iter().skip(i + 1) {
                assert_ne!(a, b, "every status variant should resolve to a unique fill");
            }
        }
    }

    #[test]
    fn animation_none_pushes_no_metadata() {
        let theme = WgpuTheme::default();
        let mut s = scene();
        dot(
            &mut s,
            Point::new(10.0, 10.0),
            DotVariant::Ok,
            DotSize::Sm,
            false,
            DotAnimation::None,
            &theme,
        );
        assert!(s.animations.is_empty());
    }

    #[test]
    fn animation_non_none_pushes_one_instance_keyed_at_center() {
        let theme = WgpuTheme::default();
        for kind in [
            DotAnimation::Pulse,
            DotAnimation::PulseUrgent,
            DotAnimation::Spin,
        ] {
            let mut s = scene();
            dot(
                &mut s,
                Point::new(42.0, 24.0),
                DotVariant::Warn,
                DotSize::Md,
                true,
                kind,
                &theme,
            );
            assert_eq!(s.animations.len(), 1, "{kind:?} should push one instance");
            assert_eq!(s.animations[0].kind, kind);
            assert_eq!(s.animations[0].center, Point::new(42.0, 24.0));
            assert!((s.animations[0].radius - DotSize::Md.diameter() * 0.5).abs() < 0.001);
        }
    }
}
