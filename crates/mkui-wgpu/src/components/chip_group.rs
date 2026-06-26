//! `chip_group` — horizontal row of toggle chips.
//!
//! The shadcn "Toggle Group" shape: options share one row, one is marked
//! active, and the widget pushes a `HitRegion` per chip into the caller's
//! `PanelLayout<T>` keyed by `to_target(index)`. Returns the emitted chip
//! rects (matching index → option order) for the caller's own layout / tests.

use crate::theme::{ButtonSize, ButtonState, ButtonVariant, WgpuTheme};
use crate::types::{HitRegion, PanelLayout, Point, Rect, Scene, Size};

use super::button_with;

/// Horizontal group of toggle chips (shadcn "Toggle Group" shape). Options
/// share one row, one is marked active, and the widget pushes a `HitRegion`
/// per chip into `layout` keyed by `to_target(index)`.
///
/// Returns the emitted chip rects (matching index → option order) so the
/// caller can wire their own layout / test assertions.
// allow: signature mirrors `button` (variant / size / theme as separate
// args) plus the two extras `chip_group` needs — `layout` to push HitRegions
// into and `to_target` to key them. Bundling these into a param-struct would
// diverge from the rest of the components module without clarity gain.
#[allow(clippy::too_many_arguments)]
pub fn chip_group<T>(
    scene: &mut Scene,
    layout: &mut PanelLayout<T>,
    row_rect: Rect,
    labels: &[&str],
    selected: usize,
    variant: ButtonVariant,
    size: ButtonSize,
    theme: &WgpuTheme,
    to_target: impl Fn(usize) -> T,
) -> Vec<Rect> {
    const INNER_PADDING: f32 = 10.0;
    const GAP: f32 = 6.0;
    const CHIP_HEIGHT: f32 = 22.0;
    const CHIP_BOTTOM_INSET: f32 = 6.0;

    if labels.is_empty() {
        return Vec::new();
    }

    let style = theme.button_style(variant, size);
    let count = labels.len() as f32;
    let available = row_rect.size.width - INNER_PADDING * 2.0 - GAP * (count - 1.0);
    let chip_width = (available / count).max(40.0);
    let chip_y = row_rect.origin.y + row_rect.size.height - CHIP_HEIGHT - CHIP_BOTTOM_INSET;

    let mut rects = Vec::with_capacity(labels.len());
    for (index, label_text) in labels.iter().enumerate() {
        let chip_rect = Rect::new(
            Point::new(
                row_rect.origin.x + INNER_PADDING + index as f32 * (chip_width + GAP),
                chip_y,
            ),
            Size::new(chip_width, CHIP_HEIGHT),
        );
        let state = if index == selected {
            ButtonState::Active
        } else {
            ButtonState::Idle
        };
        button_with(scene, chip_rect, label_text, state, &style);
        layout.hit_regions.push(HitRegion {
            rect: chip_rect,
            target: to_target(index),
        });
        rects.push(chip_rect);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_group_pushes_one_hit_region_per_option() {
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let theme = WgpuTheme::default();
        let mut layout: PanelLayout<usize> = PanelLayout::default();
        let row = Rect::new(Point::new(0.0, 0.0), Size::new(300.0, 50.0));
        let rects = chip_group(
            &mut scene,
            &mut layout,
            row,
            &["A", "B", "C"],
            1,
            ButtonVariant::Outline,
            ButtonSize::Sm,
            &theme,
            |i| i,
        );
        assert_eq!(rects.len(), 3);
        assert_eq!(layout.hit_regions.len(), 3);
        assert_eq!(layout.hit_regions[1].target, 1);
    }
}
