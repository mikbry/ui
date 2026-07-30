//! Scene → triangle tessellation.
//!
//! Text rasterization is delegated to an [`Arc<dyn TextSystem>`] from
//! [`mkui_text`]; this module no longer owns the 5×7 bitmap table. The
//! tessellator asks the text system to lay out each `Text` primitive into
//! [`LayoutRun`]s, then rasterizes each glyph into an `Alpha` `GlyphImage`
//! and emits one quad per non-zero source pixel.

use mkui_text::{
    BitmapTextSystem, FontId, GlyphFormat, HintingMode, LayoutSpec, TextAlign as MkuiTextAlign,
    TextSystem, VariationSettings,
};

use crate::types::{
    Color, GuiTriangle, Icon, Insets, Point, Primitive, Quad, Rect, Scene, Shadow, Size, Text,
    TextAlign,
};

/// Round `value_px` to the nearest physical pixel at `device_pixel_ratio`,
/// then convert back to logical pixels. A local copy of the same snap used
/// by `mkui-vector2d-wgpu`'s Slug lane (Phase 3) — duplicated rather than
/// shared across crates because this module has no `slug`-feature
/// dependency on that crate, and the function is two lines.
fn snap_to_physical_pixel(value_px: f32, device_pixel_ratio: f32) -> f32 {
    (value_px * device_pixel_ratio).round() / device_pixel_ratio.max(f32::EPSILON)
}

pub fn tessellate_scene(scene: &Scene) -> Vec<GuiTriangle> {
    let system = BitmapTextSystem::new();
    tessellate_scene_with_text(scene, &system)
}

/// Tessellate `scene` using an explicit text system. Renderers that hold
/// their own `Arc<dyn TextSystem>` (so they can swap implementations
/// between sprints) call this directly rather than the convenience wrapper.
/// `device_pixel_ratio` is `1.0` (no logical/physical split) — callers with
/// a real per-frame DPR use [`tessellate_primitives`] directly, as the
/// windowed `Renderer::render` does.
pub fn tessellate_scene_with_text(scene: &Scene, text_system: &dyn TextSystem) -> Vec<GuiTriangle> {
    tessellate_primitives(&scene.primitives, text_system, 1.0)
}

/// Tessellate a contiguous slice of primitives. The ordered render path (#66)
/// calls this per [`crate::RenderCommand`] run so each command's geometry is
/// emitted in scene order; `tessellate_scene_with_text` is the whole-scene
/// convenience over the full primitive list.
///
/// `device_pixel_ratio` is the frame's physical-pixels-per-logical-pixel
/// ratio (`1.0` when the caller's pixel space has no logical/physical
/// split) — every bitmap glyph position is snapped to the device-pixel grid
/// (#157 Phase 4 variant B, Codex plan step 8: "snap every glyph to device
/// pixels" — unconditional, unlike Phase 3's Slug baseline snap, which
/// gates on a small-text threshold specifically to protect Phase 1/2's
/// large-text adapter-parity fixtures; the bitmap lane has no equivalent
/// large-text case to protect, since it carries no adapter-comparison
/// fixtures at all). This function re-tessellates fresh from the
/// declarative `Scene` on every call (the windowed renderer calls it once
/// per frame), so — unlike Phase 3's first, reverted cut of the Slug
/// baseline snap — there is no scene-construction-time caching for a DPI
/// change to go stale against.
pub fn tessellate_primitives(
    primitives: &[Primitive],
    text_system: &dyn TextSystem,
    device_pixel_ratio: f32,
) -> Vec<GuiTriangle> {
    let mut triangles = Vec::new();
    for primitive in primitives {
        match primitive {
            Primitive::Shadow(shadow) => tessellate_shadow(&mut triangles, *shadow),
            Primitive::Quad(quad) => tessellate_quad(&mut triangles, *quad),
            Primitive::Text(text) => {
                tessellate_text(&mut triangles, text, text_system, device_pixel_ratio)
            }
            Primitive::Icon(icon) => tessellate_icon(&mut triangles, icon),
            // Slug glyphs are not tessellated to triangles — they draw on the
            // Slug coverage lane (#66). The ordered render path routes them
            // there directly, so the triangle tessellator skips them.
            #[cfg(feature = "slug")]
            Primitive::SlugGlyph(_) => {}
        }
    }
    triangles
}

fn tessellate_shadow(triangles: &mut Vec<GuiTriangle>, shadow: Shadow) {
    let spread = shadow.spread.max(0.0) + shadow.blur_radius.max(0.0) * 0.35;
    push_rect(
        triangles,
        shadow.rect.expand(spread),
        shadow.color.multiply_alpha(0.6),
    );
}

fn tessellate_quad(triangles: &mut Vec<GuiTriangle>, quad: Quad) {
    push_rect(triangles, quad.rect, quad.fill);
    if let Some(stroke) = quad.stroke {
        let width = stroke.width.max(1.0);
        let top = Rect::new(quad.rect.origin, Size::new(quad.rect.size.width, width));
        let bottom = Rect::new(
            Point::new(quad.rect.origin.x, quad.rect.height_end() - width),
            Size::new(quad.rect.size.width, width),
        );
        let left = Rect::new(quad.rect.origin, Size::new(width, quad.rect.size.height));
        let right = Rect::new(
            Point::new(quad.rect.width_end() - width, quad.rect.origin.y),
            Size::new(width, quad.rect.size.height),
        );
        push_rect(triangles, top, stroke.color);
        push_rect(triangles, bottom, stroke.color);
        push_rect(triangles, left, stroke.color);
        push_rect(triangles, right, stroke.color);
    }
}

fn tessellate_text(
    triangles: &mut Vec<GuiTriangle>,
    text: &Text,
    system: &dyn TextSystem,
    device_pixel_ratio: f32,
) {
    let line_height = text.style.line_height_px.max(1.0);
    let max_lines = ((text.rect.size.height / line_height).floor() as usize).max(1);
    // The bitmap face is the only face this renderer drives in Sprint 7, so a
    // font handle is always the reserved bitmap identity. `FontId` is opaque
    // (no public raw constructor); callers re-resolve through the text system
    // rather than forging an id from `text.style.font`.
    let spec = LayoutSpec {
        font_id: FontId::BITMAP,
        font_generation: 0,
        font_size_px: text.style.font_size_px,
        line_height_px: line_height,
        align: map_align(text.style.align),
        max_lines: Some(max_lines),
        hinting: HintingMode::None,
        variations: VariationSettings::empty(),
        synthesis_flags: 0,
    };

    let runs = system.layout(&text.content, &spec, Some(text.rect.size.width));
    for run in &runs {
        for glyph in &run.glyphs {
            // Space (0x20) has an all-zero bitmap; skip the rasterize call
            // entirely so the renderer never allocates an empty image.
            if glyph.glyph_id == ' ' as u32 {
                continue;
            }
            let key = run.cache_key(glyph);
            let image = match system.rasterize(key) {
                Ok(image) => image,
                Err(_) => continue,
            };
            if image.format != GlyphFormat::Alpha {
                continue;
            }
            let base_x = text.rect.origin.x + run.origin_x_px + glyph.x_px + image.left_px as f32;
            let base_y = text.rect.origin.y + run.origin_y_px + glyph.y_px + image.top_px as f32;
            // #157 Phase 4 variant B: snap the whole glyph cell's origin as
            // one unit (not each output pixel independently) so its 5×7
            // raster shape is preserved exactly, just aligned to the device
            // pixel grid. Unconditional — every bitmap glyph snaps,
            // regardless of font size (Codex plan step 8's literal text;
            // Codex round 1 of this PR's review correctly rejected an
            // earlier cut that gated this on the same small-text threshold
            // Phase 3 uses for Slug, which would have left the demo's own
            // 16px label unsnapped).
            let base_x = snap_to_physical_pixel(base_x, device_pixel_ratio);
            let base_y = snap_to_physical_pixel(base_y, device_pixel_ratio);
            for oy in 0..image.height_px {
                for ox in 0..image.width_px {
                    let alpha = image.data[(oy * image.width_px + ox) as usize];
                    if alpha == 0 {
                        continue;
                    }
                    push_text_cell(
                        triangles,
                        Point::new(base_x + ox as f32, base_y + oy as f32),
                        1.0,
                        text.style.color.multiply_alpha(alpha as f32 / 255.0),
                    );
                }
            }
        }
    }
}

fn tessellate_icon(triangles: &mut Vec<GuiTriangle>, icon: &Icon) {
    let tint = icon.tint.multiply_alpha(0.9);
    let inset = icon.rect.inset(Insets::all(
        icon.rect.size.width.min(icon.rect.size.height) * 0.2,
    ));
    push_rect(triangles, inset, tint);
}

fn map_align(align: TextAlign) -> MkuiTextAlign {
    match align {
        TextAlign::Start => MkuiTextAlign::Start,
        TextAlign::Center => MkuiTextAlign::Center,
        TextAlign::End => MkuiTextAlign::End,
    }
}

fn push_text_cell(triangles: &mut Vec<GuiTriangle>, origin: Point, size: f32, color: Color) {
    let rect = Rect::new(origin, Size::new(size, size));
    push_rect(
        triangles,
        rect.expand(size * 0.2),
        color.multiply_alpha(0.16),
    );
    push_rect(triangles, rect, color);
}

fn push_rect(triangles: &mut Vec<GuiTriangle>, rect: Rect, color: Color) {
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 || color.a <= 0.0 {
        return;
    }

    let top_left = rect.origin;
    let top_right = Point::new(rect.width_end(), rect.origin.y);
    let bottom_left = Point::new(rect.origin.x, rect.height_end());
    let bottom_right = Point::new(rect.width_end(), rect.height_end());
    triangles.push(GuiTriangle {
        points: [top_left, top_right, bottom_right],
        color,
    });
    triangles.push(GuiTriangle {
        points: [top_left, bottom_right, bottom_left],
        color,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::WgpuTheme;
    use crate::types::{Scene, Size, Text};

    #[test]
    fn text_tessellation_produces_triangles() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, 8.0), Size::new(220.0, 24.0)),
            content: "Undo Ctrl/Cmd+Z".to_string(),
            style: WgpuTheme::default().body_style,
        });

        let triangles = tessellate_scene(&scene);
        assert!(!triangles.is_empty());
        assert!(triangles.iter().all(|triangle| triangle.color.a > 0.0));
    }

    #[test]
    fn empty_text_yields_no_triangles() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, 8.0), Size::new(220.0, 24.0)),
            content: String::new(),
            style: WgpuTheme::default().body_style,
        });
        assert!(tessellate_scene(&scene).is_empty());
    }

    // ---- Phase 4 variant B: bitmap device-pixel snap (#157 step 8) --------

    // dame-rubric.md § Phase 4 variant B (N): the snap function itself,
    // tested in isolation with exact control over the sub-pixel sweep —
    // the same literal claim as Phase 3's Slug baseline snap ("100
    // sub-pixel offsets across one physical-pixel period group into
    // exactly 2 piecewise-constant cells, split at the period's midpoint").
    #[test]
    fn snap_to_physical_pixel_is_piecewise_constant_over_one_period() {
        let base = 10.0f32;
        let mut values = Vec::with_capacity(100);
        for i in 0..100 {
            let t = i as f32 / 100.0;
            values.push(snap_to_physical_pixel(base + t, 1.0));
        }
        let mut distinct = values.clone();
        distinct.dedup();
        assert_eq!(
            distinct,
            vec![10.0, 11.0],
            "expected exactly 2 cells (nearest-pixel snap), got {distinct:?}"
        );
        for (i, &v) in values.iter().enumerate() {
            let t = i as f32 / 100.0;
            let expected = if t < 0.5 { 10.0 } else { 11.0 };
            assert_eq!(
                v, expected,
                "offset {t} landed in the wrong cell (value {v})"
            );
        }
    }

    #[test]
    fn snap_to_physical_pixel_scales_grid_with_device_pixel_ratio() {
        for dpr in [1.0f32, 1.5, 2.0, 3.0] {
            let base = 10.0f32;
            let mut values = Vec::with_capacity(100);
            for i in 0..100 {
                let t = (i as f32 / 100.0) / dpr;
                values.push(snap_to_physical_pixel(base + t, dpr));
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert_eq!(
                distinct.len(),
                2,
                "device_pixel_ratio {dpr}: expected 2 cells, got {distinct:?}"
            );
            let delta = distinct[1] - distinct[0];
            assert!(
                (delta - 1.0 / dpr).abs() < 1e-4,
                "device_pixel_ratio {dpr}: cell delta {delta} should be one physical \
                 pixel (1/{dpr} logical px)"
            );
        }
    }

    /// Tessellate a single "#" glyph at `font_size_px`, `rect_origin_y`, and
    /// `device_pixel_ratio`, returning the minimum Y coordinate across every
    /// emitted triangle vertex — a stand-in for the glyph cell's overall
    /// screen position that shifts in lockstep with `base_y` regardless of
    /// which specific bits are "on" in the raster.
    fn min_triangle_y(font_size_px: f32, rect_origin_y: f32, device_pixel_ratio: f32) -> f32 {
        let system = BitmapTextSystem::new();
        let mut style = WgpuTheme::default().body_style;
        style.font_size_px = font_size_px;
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, rect_origin_y), Size::new(220.0, 40.0)),
            content: "#".to_string(),
            style,
        });
        let triangles = tessellate_primitives(&scene.primitives, &system, device_pixel_ratio);
        assert!(
            !triangles.is_empty(),
            "\"#\" must tessellate to some triangles"
        );
        triangles
            .iter()
            .flat_map(|t| t.points.iter().map(|p| p.y))
            .fold(f32::INFINITY, f32::min)
    }

    // Integration-level proof that `tessellate_text` actually wires the snap
    // in (gated correctly) through the real `Scene` -> `tessellate_primitives`
    // path, rather than the math being correct in isolation but never reached.
    #[test]
    fn bitmap_position_snap_moves_in_quantized_physical_pixel_steps_at_every_font_size() {
        // Codex round 1 of the Phase 4 PR review: an earlier cut gated this
        // snap on the same small-text threshold Phase 3 uses for Slug, which
        // left the demo's own 16px label (`examples/text`) unsnapped — Codex
        // plan step 8 says "snap every glyph to device pixels", with no
        // threshold. Sweep several sizes, explicitly including the real
        // demo label size (16px), to prove there is no such gate anymore.
        let device_pixel_ratio = 2.0f32;
        for font_size_px in [12.0f32, 16.0, 20.0, 48.0] {
            let mut values = Vec::with_capacity(100);
            for i in 0..100 {
                let rect_origin_y = 8.0 + i as f32 * 0.05;
                values.push(min_triangle_y(
                    font_size_px,
                    rect_origin_y,
                    device_pixel_ratio,
                ));
            }
            let mut distinct = values.clone();
            distinct.dedup();
            assert!(
                distinct.len() < values.len(),
                "font_size_px {font_size_px}: bitmap text position must be quantized, \
                 not continuous ({} distinct of {})",
                distinct.len(),
                values.len()
            );
            for w in distinct.windows(2) {
                let step = w[1] - w[0];
                assert!(
                    (step - 1.0 / device_pixel_ratio).abs() < 1e-4,
                    "font_size_px {font_size_px}: quantization step {step} must equal \
                     one physical pixel (1/{device_pixel_ratio} logical px)"
                );
            }
        }
    }
}
