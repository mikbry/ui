//! Proves issue #63's acceptance criterion "wgpu walker can consume
//! computed rects instead of owning permanent layout logic". Issue #287
//! did the real swap this test originally only proved *compatible*: the
//! walker's `walk_app_tree` now calls `mkui_layout::compute_layout` itself,
//! via the same `mkui_wgpu::walker::WgpuLeafMeasure` this test builds below
//! (promoted from this file's own one-off `WgpuTextMeasure` sketch once
//! that was proven compatible — see `walker.rs`'s "Layout model" docs and
//! `mkui-layout`'s crate-level "Status" section). This test now exists to
//! guard the conversion from `mkui_layout`'s own rects into
//! `mkui_wgpu::types::Rect` — the exact type `WalkOutput::layouts` uses —
//! independently of the full walk, using the walker's real measurer rather
//! than a parallel copy that could drift from it.

use mkui_core::components::{Button, Mkui as CoreMkui, Text as CoreText, View};
use mkui_core::headless::{ButtonVariant, TextVariant};
use mkui_layout::{compute_layout, Point as LayoutPoint, Rect as LayoutRect, Size as LayoutSize};
use mkui_text::BitmapTextSystem;
use mkui_wgpu::theme::WgpuTheme;
use mkui_wgpu::types::{Point as WgpuPoint, Rect as WgpuRect, Size as WgpuSize};
use mkui_wgpu::walker::WgpuLeafMeasure;

fn layout_point_to_wgpu(p: LayoutPoint) -> WgpuPoint {
    WgpuPoint { x: p.x, y: p.y }
}

fn layout_size_to_wgpu(s: LayoutSize) -> WgpuSize {
    WgpuSize {
        width: s.width,
        height: s.height,
    }
}

fn layout_rect_to_wgpu(r: LayoutRect) -> WgpuRect {
    WgpuRect {
        origin: layout_point_to_wgpu(r.origin),
        size: layout_size_to_wgpu(r.size),
    }
}

#[test]
fn mkui_layout_rects_convert_cleanly_into_wgpu_types_and_cover_every_visible_node() {
    let core = CoreMkui::new().child(
        View::new()
            .class("flex items-center justify-between px-4 h-16")
            .child(CoreText::new("miklabs/ui").variant(TextVariant::Heading2))
            .child(Button::new("GitHub").variant(ButtonVariant::Ghost)),
    );

    let text_system = BitmapTextSystem::new();
    let theme = WgpuTheme::default();
    let measure = WgpuLeafMeasure {
        theme: &theme,
        text_system: &text_system,
        slug_font_id: None,
    };
    let result = compute_layout(core.tree(), LayoutSize::new(800.0, 600.0), &measure);

    // Every non-root node the builder created got a rect — three: the row
    // View, the heading Text, and the GitHub Button.
    assert_eq!(result.len(), 3);

    for (_, rect) in result.iter() {
        let wgpu_rect = layout_rect_to_wgpu(rect);
        assert!(wgpu_rect.size.width >= 0.0 && wgpu_rect.size.height >= 0.0);
        assert!(wgpu_rect.origin.x >= 0.0 && wgpu_rect.origin.y >= 0.0);
    }
}
