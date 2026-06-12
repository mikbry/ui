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

pub fn tessellate_scene(scene: &Scene) -> Vec<GuiTriangle> {
    let system = BitmapTextSystem::new();
    tessellate_scene_with_text(scene, &system)
}

/// Tessellate `scene` using an explicit text system. Renderers that hold
/// their own `Arc<dyn TextSystem>` (so they can swap implementations
/// between sprints) call this directly rather than the convenience wrapper.
pub fn tessellate_scene_with_text(scene: &Scene, text_system: &dyn TextSystem) -> Vec<GuiTriangle> {
    let mut triangles = Vec::new();
    for primitive in &scene.primitives {
        match primitive {
            Primitive::Shadow(shadow) => tessellate_shadow(&mut triangles, *shadow),
            Primitive::Quad(quad) => tessellate_quad(&mut triangles, *quad),
            Primitive::Text(text) => tessellate_text(&mut triangles, text, text_system),
            Primitive::Icon(icon) => tessellate_icon(&mut triangles, icon),
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

fn tessellate_text(triangles: &mut Vec<GuiTriangle>, text: &Text, system: &dyn TextSystem) {
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
}
