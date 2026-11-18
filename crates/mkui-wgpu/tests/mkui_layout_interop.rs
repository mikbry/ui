//! Proves issue #63's acceptance criterion "wgpu walker can consume
//! computed rects instead of owning permanent layout logic" — a
//! **compatibility** claim, not a rewrite of `walker.rs` yet (see that
//! module's "Layout model" docs and `mkui-layout`'s crate-level "Status"
//! section for why the actual swap is its own follow-up).
//!
//! This test runs `mkui_layout::compute_layout` over the same tree the
//! walker itself renders, using a [`mkui_layout::LeafMeasure`] that wraps
//! the identical `&dyn mkui_text::TextSystem` `WalkOptions::text_system`
//! already carries, then converts every resulting rect into
//! `mkui_wgpu::types::Rect` — the exact type `WalkOutput::layouts` uses
//! today. If `mkui-layout`'s output shape were ever incompatible with what
//! the walker needs (origin/size semantics, `NodeId` keying), this
//! conversion would not compile or the sanity assertions below would fail.

use mkui_core::components::{Button, Mkui as CoreMkui, Text as CoreText, View};
use mkui_core::headless::{ButtonVariant, TextVariant};
use mkui_layout::{
    compute_layout, LeafMeasure, Measured, Point as LayoutPoint, Rect as LayoutRect,
    Size as LayoutSize,
};
use mkui_runtime::ResolvedStyle;
use mkui_text::{BitmapTextSystem, FontId, HintingMode, LayoutSpec, TextSystem, VariationSettings};
use mkui_wgpu::types::{Point as WgpuPoint, Rect as WgpuRect, Size as WgpuSize};

/// Wraps a `&dyn TextSystem` — the same text system `WalkOptions` carries —
/// as a `mkui_layout::LeafMeasure`. A real backend would keep one of these
/// alongside its `WalkOptions` once the walker actually calls into
/// `mkui-layout`.
struct WgpuTextMeasure<'a> {
    text_system: &'a dyn TextSystem,
}

impl WgpuTextMeasure<'_> {
    fn font_size_px(variant: TextVariant) -> f32 {
        match variant {
            TextVariant::Heading1 => 32.0,
            TextVariant::Heading2 => 24.0,
            TextVariant::Heading3 => 20.0,
            TextVariant::Body | TextVariant::Code => 16.0,
            TextVariant::Caption | TextVariant::Label => 14.0,
            _ => 16.0,
        }
    }
}

impl LeafMeasure for WgpuTextMeasure<'_> {
    fn measure_text(
        &self,
        content: &str,
        variant: TextVariant,
        _style: &ResolvedStyle,
        max_width_px: Option<f32>,
    ) -> Measured {
        let font_size_px = Self::font_size_px(variant);
        let line_height_px = (font_size_px * 1.25).max(font_size_px + 2.0);
        let spec = LayoutSpec {
            font_id: FontId::BITMAP,
            font_generation: 0,
            font_size_px,
            line_height_px,
            align: mkui_text::TextAlign::Start,
            max_lines: None,
            hinting: HintingMode::None,
            variations: VariationSettings::empty(),
            synthesis_flags: 0,
        };
        let runs = self
            .text_system
            .layout(content, &spec, max_width_px.map(|w| w.max(1.0)));
        let width = runs
            .iter()
            .flat_map(|r| r.glyphs.iter())
            .map(|g| g.x_px + g.x_advance_px)
            .fold(0.0_f32, f32::max);
        let height = line_height_px * (runs.len().max(1) as f32);
        Measured::new(width, height)
    }

    fn measure_button(
        &self,
        label: &str,
        _variant: ButtonVariant,
        style: &ResolvedStyle,
        _max_width_px: Option<f32>,
    ) -> Measured {
        let label_width = self
            .measure_text(label, TextVariant::Label, style, None)
            .size
            .width;
        Measured::new(label_width + 32.0, 36.0)
    }
}

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
    let measure = WgpuTextMeasure {
        text_system: &text_system,
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
