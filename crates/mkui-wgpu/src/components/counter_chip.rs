//! `counter_chip` — icon + count chip with a hover/focus tooltip (#191, C6
//! of #173).
//!
//! [`super::chip_group`] is a row of toggle options sharing one
//! pick-one-of-many shape (one `HitRegion` per option, a selected index). A
//! counter chip is a single, non-toggling icon+count readout — a different
//! shape, not a row of options — so this lands as a new atom (mirroring
//! [`super::badge`] / [`super::dot`]) rather than an extension of
//! `chip_group`, per the issue's "Builds on" note.
//!
//! Hover and focus are each supplied by the caller every frame, the same
//! orthogonal "state is supplied per-frame" shape [`crate::theme::ButtonState`]
//! uses (see the `theme` module doc): `counter_chip` takes `hovered: bool`
//! and `focused: bool` separately and reveals the tooltip bubble whenever
//! either is `true` — the issue's acceptance criterion is "hover **or**
//! focus," not hover alone, and a host routing keyboard focus has no
//! pointer position to fold into a single flag. The bubble drawn here is a
//! minimal inline pill, not the general-purpose `Tooltip` atom tracked by
//! #192 (C7 of #173); once that lands, this call site should delegate to it
//! instead of drawing its own. It is also drawn mid-scene like every other
//! primitive here — anything the caller draws afterward paints over it;
//! stacking the bubble above later content is a layering concern for #192,
//! not something this atom tries to solve on its own.
//!
//! The bubble is clamped to stay inside `scene.viewport`: it flips to below
//! the chip when there isn't room above (e.g. a HUD counter pinned to the
//! screen's top edge), and its x-position clamps so it never runs off the
//! left/right edge.

use crate::theme::CounterChipStyle;
use crate::types::{CornerRadii, Icon, IconId, Point, Quad, Rect, Scene, Size, Stroke, TextStyle};

use super::label;

/// Horizontal gap between the icon and the count label.
const ICON_COUNT_GAP: f32 = 4.0;

/// Emitted regions from one [`counter_chip`] call — useful for layout math
/// and test/debug assertions. `tooltip_rect` is `None` whenever both
/// `hovered` and `focused` were `false`, or `tooltip` was empty (nothing
/// was drawn to hit-test).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CounterChipRegions {
    pub chip_rect: Rect,
    pub icon_rect: Rect,
    pub tooltip_rect: Option<Rect>,
}

/// Approximate advance width per char, used only to size the tooltip
/// bubble. The same known approximation
/// `components::text_field::approx_char_advance_px` uses, for the same
/// reason: `mkui-text`'s real measurement API (`TextEngine`/`PreparedText`,
/// #182) needs a loaded `TextSystem`/font to shape against, and this
/// crate's components layer takes no such handle — only a `WgpuTheme` and
/// plain geometry — so it has nothing to call that API with. Not real glyph
/// measurement; visually fine for short tooltip strings on a
/// roughly-monospace-ish font.
fn approx_char_advance_px(text_style: &TextStyle) -> f32 {
    text_style.font_size_px * 0.5
}

/// Icon + count chip. Draws the pill chrome, the icon (an [`IconId`] handle
/// — rasterizing it is a renderer concern, not this atom's) and the count
/// label; when `hovered` or `focused` is `true` and `tooltip` is non-empty
/// it also draws a tooltip bubble above the chip, clamped to
/// `scene.viewport`.
// allow: mirrors `dot` / `chip_group` — a variant-free atom whose arguments
// are the icon/count/tooltip payload plus the two orthogonal interaction
// states. Bundling these into a param-struct would diverge from the rest of
// the atom layer's call shape without clarity gain.
#[allow(clippy::too_many_arguments)]
pub fn counter_chip(
    scene: &mut Scene,
    chip_rect: Rect,
    icon: IconId,
    count: u32,
    tooltip: &str,
    hovered: bool,
    focused: bool,
    style: &CounterChipStyle,
) -> CounterChipRegions {
    scene.quad(Quad {
        rect: chip_rect,
        fill: style.chip_fill,
        corner_radii: CornerRadii::all(style.corner_radius),
        stroke: Some(Stroke {
            color: style.chip_stroke,
            width: style.chip_stroke_width,
        }),
    });

    let icon_diameter = (chip_rect.size.height - style.padding.top - style.padding.bottom).max(0.0);
    let icon_rect = Rect::new(
        Point::new(
            chip_rect.origin.x + style.padding.left,
            chip_rect.origin.y + (chip_rect.size.height - icon_diameter) * 0.5,
        ),
        Size::new(icon_diameter, icon_diameter),
    );
    scene.icon(Icon {
        rect: icon_rect,
        icon,
        tint: style.icon_tint,
    });

    let count_height = style.count_style.line_height_px;
    let count_x = icon_rect.width_end() + ICON_COUNT_GAP;
    let count_rect = Rect::new(
        Point::new(
            count_x,
            chip_rect.origin.y + (chip_rect.size.height - count_height) * 0.5,
        ),
        Size::new(
            (chip_rect.width_end() - style.padding.right - count_x).max(0.0),
            count_height,
        ),
    );
    label(scene, count_rect, count.to_string(), style.count_style);

    let tooltip_rect = ((hovered || focused) && !tooltip.is_empty()).then(|| {
        let text_width =
            tooltip.chars().count() as f32 * approx_char_advance_px(&style.tooltip_text_style);
        let bubble_width = text_width + style.tooltip_padding.left + style.tooltip_padding.right;
        let bubble_height = style.tooltip_text_style.line_height_px
            + style.tooltip_padding.top
            + style.tooltip_padding.bottom;

        let viewport = scene.viewport;
        let above_y = chip_rect.origin.y - style.tooltip_gap - bubble_height;
        // Flip below the chip when there's no room above (e.g. a HUD
        // counter pinned to the screen's top edge) rather than clipping off
        // the top — an unreachable tooltip doesn't satisfy "reveals a
        // tooltip on hover/focus".
        let bubble_y = if above_y >= 0.0 {
            above_y
        } else {
            chip_rect.height_end() + style.tooltip_gap
        };
        let centered_x = chip_rect.origin.x + (chip_rect.size.width - bubble_width) * 0.5;
        let max_x = (viewport.width - bubble_width).max(0.0);
        let bubble_x = centered_x.clamp(0.0, max_x);

        let bubble_rect = Rect::new(
            Point::new(bubble_x, bubble_y),
            Size::new(bubble_width, bubble_height),
        );
        scene.quad(Quad {
            rect: bubble_rect,
            fill: style.tooltip_fill,
            corner_radii: CornerRadii::all(style.tooltip_corner_radius),
            stroke: Some(Stroke {
                color: style.tooltip_stroke,
                width: style.tooltip_stroke_width,
            }),
        });
        label(
            scene,
            bubble_rect.inset(style.tooltip_padding),
            tooltip.to_string(),
            style.tooltip_text_style,
        );
        bubble_rect
    });

    CounterChipRegions {
        chip_rect,
        icon_rect,
        tooltip_rect,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Primitive, Size};

    fn scene() -> Scene {
        Scene::new(Size::new(200.0, 120.0))
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

    fn icons(scene: &Scene) -> Vec<&Icon> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Icon(i) => Some(i),
                _ => None,
            })
            .collect()
    }

    fn texts(scene: &Scene) -> Vec<&crate::types::Text> {
        scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    fn chip_rect() -> Rect {
        Rect::new(Point::new(20.0, 60.0), Size::new(70.0, 22.0))
    }

    #[test]
    fn not_hovered_emits_chip_icon_and_count_but_no_tooltip() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();

        let regions = counter_chip(
            &mut s,
            chip_rect(),
            IconId(1),
            7,
            "7 collected items",
            false,
            false,
            &style,
        );

        assert_eq!(quads(&s).len(), 1, "idle: chip pill only, no tooltip quad");
        assert_eq!(icons(&s).len(), 1);
        assert_eq!(icons(&s)[0].icon, IconId(1));
        let count_texts = texts(&s);
        assert_eq!(count_texts.len(), 1);
        assert_eq!(count_texts[0].content, "7");
        assert!(regions.tooltip_rect.is_none());
    }

    #[test]
    fn hovered_reveals_tooltip_bubble_and_text() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();

        let regions = counter_chip(
            &mut s,
            chip_rect(),
            IconId(1),
            7,
            "7 collected items",
            true,
            false,
            &style,
        );

        assert_eq!(
            quads(&s).len(),
            2,
            "hovered: chip pill + tooltip bubble quad"
        );
        let tooltip_texts: Vec<_> = texts(&s)
            .into_iter()
            .filter(|t| t.content == "7 collected items")
            .collect();
        assert_eq!(tooltip_texts.len(), 1, "tooltip content should be drawn");
        assert!(regions.tooltip_rect.is_some());
    }

    #[test]
    fn focus_alone_reveals_the_tooltip_without_hover() {
        // The acceptance criterion is "hover **or** focus" — a
        // keyboard-focused chip with no pointer hovering it must still
        // reveal the tooltip. `hovered: false, focused: true` is the case
        // that proves the two states are independent, not the same bool in
        // disguise.
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        let regions = counter_chip(
            &mut s,
            chip_rect(),
            IconId(2),
            3,
            "tooltip",
            false,
            true,
            &style,
        );
        assert!(regions.tooltip_rect.is_some());
    }

    #[test]
    fn neither_hovered_nor_focused_shows_no_tooltip() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        let regions = counter_chip(
            &mut s,
            chip_rect(),
            IconId(2),
            3,
            "tooltip",
            false,
            false,
            &style,
        );
        assert!(regions.tooltip_rect.is_none());
    }

    #[test]
    fn empty_tooltip_string_suppresses_bubble_even_when_hovered() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();

        let regions = counter_chip(&mut s, chip_rect(), IconId(1), 7, "", true, false, &style);

        assert_eq!(quads(&s).len(), 1, "no tooltip content: no bubble quad");
        assert!(regions.tooltip_rect.is_none());
    }

    #[test]
    fn tooltip_bubble_sits_above_the_chip_and_is_horizontally_centered() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        let chip = chip_rect();

        let regions = counter_chip(&mut s, chip, IconId(1), 7, "hi", true, false, &style);
        let tooltip = regions.tooltip_rect.expect("tooltip should be drawn");

        assert!(
            tooltip.height_end() <= chip.origin.y,
            "tooltip bottom edge must sit above the chip's top edge"
        );
        let chip_center = chip.origin.x + chip.size.width * 0.5;
        let tooltip_center = tooltip.origin.x + tooltip.size.width * 0.5;
        assert!((chip_center - tooltip_center).abs() < 0.5);
    }

    #[test]
    fn tooltip_flips_below_the_chip_when_pinned_to_the_top_edge() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        // A HUD counter pinned to the screen's top-left corner: no room
        // above the chip for the bubble.
        let chip = Rect::new(Point::new(10.0, 0.0), Size::new(70.0, 22.0));

        let regions = counter_chip(&mut s, chip, IconId(1), 7, "hi", true, false, &style);
        let tooltip = regions.tooltip_rect.expect("tooltip should be drawn");

        assert!(
            tooltip.origin.y >= chip.height_end(),
            "with no room above, the bubble must flip below the chip instead of clipping off-screen"
        );
    }

    #[test]
    fn tooltip_x_clamps_inside_the_viewport_near_the_left_edge() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        // A narrow chip near the viewport's left edge with a long tooltip —
        // the centered bubble would otherwise start at a negative x.
        let chip = Rect::new(Point::new(2.0, 60.0), Size::new(20.0, 22.0));

        let regions = counter_chip(&mut s, chip, IconId(1), 7, "overflow", true, false, &style);
        let tooltip = regions.tooltip_rect.expect("tooltip should be drawn");

        assert!(
            tooltip.origin.x >= 0.0,
            "the bubble must not run off the viewport's left edge"
        );
        assert!(
            tooltip.width_end() <= s.viewport.width + 0.01,
            "the bubble must not run off the viewport's right edge"
        );
    }

    #[test]
    fn icon_rect_fits_within_the_chip_vertically() {
        let theme = WgpuTheme::default();
        let style = theme.counter_chip();
        let mut s = scene();
        let chip = chip_rect();

        let regions = counter_chip(&mut s, chip, IconId(5), 1, "x", false, false, &style);

        assert!(regions.icon_rect.origin.y >= chip.origin.y);
        assert!(regions.icon_rect.height_end() <= chip.height_end());
        assert!(regions.icon_rect.origin.x >= chip.origin.x);
    }
}
