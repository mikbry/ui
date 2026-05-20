//! Scene → triangle tessellation and the bitmap fallback glyph table.
//!
//! Kept private: the public contract is `tessellate_scene` re-exported from
//! `lib.rs`. Everything else (bitmap font table, text wrapping, primitive
//! push helpers) is implementation detail and will be replaced once the
//! SDF / MSDF atlas path lands (see `docs/gui.md`).

use crate::types::{
    Color, GuiTriangle, Icon, Insets, Point, Primitive, Quad, Rect, Scene, Shadow, Size, Text,
    TextAlign,
};

pub fn tessellate_scene(scene: &Scene) -> Vec<GuiTriangle> {
    let mut triangles = Vec::new();
    for primitive in &scene.primitives {
        match primitive {
            Primitive::Shadow(shadow) => tessellate_shadow(&mut triangles, *shadow),
            Primitive::Quad(quad) => tessellate_quad(&mut triangles, *quad),
            Primitive::Text(text) => tessellate_text(&mut triangles, text),
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

fn tessellate_text(triangles: &mut Vec<GuiTriangle>, text: &Text) {
    let scale = (text.style.font_size_px / 10.0).max(1.0);
    let glyph_width = 5.0 * scale;
    let glyph_height = 7.0 * scale;
    let advance = 5.8 * scale;
    let max_glyphs = max_glyphs_for_width(text.rect.size.width, advance, glyph_width);
    let max_lines =
        ((text.rect.size.height / text.style.line_height_px.max(1.0)).floor() as usize).max(1);
    let wrapped_lines = wrap_text_lines(&text.content, max_glyphs, max_lines);

    for (line_index, line) in wrapped_lines.iter().enumerate() {
        let glyphs = line.chars().map(normalize_bitmap_char).collect::<Vec<_>>();
        let line_width = measure_line_width(&glyphs, advance, glyph_width);
        let origin_x = match text.style.align {
            TextAlign::Start => text.rect.origin.x,
            TextAlign::Center => text.rect.origin.x + (text.rect.size.width - line_width) * 0.5,
            TextAlign::End => text.rect.width_end() - line_width,
        };
        let line_top = text.rect.origin.y
            + line_index as f32 * text.style.line_height_px
            + ((text.style.line_height_px - glyph_height).max(0.0) * 0.5);
        let mut pen_x = origin_x;

        for glyph in glyphs {
            if glyph == ' ' {
                pen_x += advance;
                continue;
            }

            for (row_index, row_bits) in bitmap_glyph(glyph).into_iter().enumerate() {
                for col in 0..5 {
                    let mask = 1 << (4 - col);
                    if row_bits & mask == 0 {
                        continue;
                    }
                    push_text_cell(
                        triangles,
                        Point::new(
                            pen_x + col as f32 * scale,
                            line_top + row_index as f32 * scale,
                        ),
                        scale,
                        text.style.color,
                    );
                }
            }

            pen_x += advance;
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

fn max_glyphs_for_width(width: f32, advance: f32, glyph_width: f32) -> usize {
    if width <= glyph_width {
        1
    } else {
        (((width - glyph_width) / advance).floor() as usize + 1).max(1)
    }
}

fn wrap_text_lines(content: &str, max_glyphs: usize, max_lines: usize) -> Vec<String> {
    let mut wrapped = Vec::new();
    let mut truncated = false;

    for paragraph in content.split('\n') {
        let paragraph_lines = wrap_paragraph(paragraph, max_glyphs);
        if paragraph_lines.is_empty() {
            wrapped.push(String::new());
        } else {
            wrapped.extend(paragraph_lines);
        }
        if wrapped.len() > max_lines {
            truncated = true;
            wrapped.truncate(max_lines);
            break;
        }
    }

    if wrapped.len() > max_lines {
        wrapped.truncate(max_lines);
        truncated = true;
    }

    if truncated && !wrapped.is_empty() {
        let last = wrapped.pop().unwrap_or_default();
        wrapped.push(ellipsize_line(&last, max_glyphs));
    }

    wrapped
}

fn wrap_paragraph(paragraph: &str, max_glyphs: usize) -> Vec<String> {
    let chars = paragraph.chars().collect::<Vec<_>>();
    if chars.is_empty() {
        return vec![];
    }

    let mut lines = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        while start < chars.len() && chars[start].is_whitespace() {
            start += 1;
        }
        if start >= chars.len() {
            break;
        }

        let remaining = chars.len() - start;
        if remaining <= max_glyphs {
            lines.push(chars[start..].iter().collect::<String>());
            break;
        }

        let end = start + max_glyphs;
        let mut break_at = None;
        for index in (start..end).rev() {
            if chars[index].is_whitespace() {
                break_at = Some(index);
                break;
            }
        }

        match break_at {
            Some(index) if index > start => {
                lines.push(chars[start..index].iter().collect::<String>());
                start = index + 1;
            }
            _ => {
                lines.push(chars[start..end].iter().collect::<String>());
                start = end;
            }
        }
    }

    lines
}

fn ellipsize_line(line: &str, max_glyphs: usize) -> String {
    let mut glyphs = line.trim_end().chars().collect::<Vec<_>>();
    if max_glyphs <= 3 {
        return ".".repeat(max_glyphs);
    }

    if glyphs.len() > max_glyphs - 3 {
        glyphs.truncate(max_glyphs - 3);
    }

    let mut result = glyphs.into_iter().collect::<String>();
    result.push_str("...");
    result
}

fn measure_line_width(glyphs: &[char], advance: f32, glyph_width: f32) -> f32 {
    match glyphs.len() {
        0 => 0.0,
        count => (count.saturating_sub(1)) as f32 * advance + glyph_width,
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

fn normalize_bitmap_char(character: char) -> char {
    match character {
        '²' => '2',
        '×' => 'X',
        '–' | '—' => '-',
        '‘' | '’' => '\'',
        _ => character,
    }
}

fn bitmap_glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01111, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b01111,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01111, 0b10000, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b00001, 0b00001, 0b00001, 0b00001, 0b10001, 0b10001, 0b01110,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        'a' => [
            0b00000, 0b01110, 0b00001, 0b01111, 0b10001, 0b10011, 0b01101,
        ],
        'b' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b11110,
        ],
        'c' => [
            0b00000, 0b01110, 0b10001, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'd' => [
            0b00001, 0b00001, 0b01101, 0b10011, 0b10001, 0b10001, 0b01111,
        ],
        'e' => [
            0b00000, 0b01110, 0b10001, 0b11111, 0b10000, 0b10001, 0b01110,
        ],
        'f' => [
            0b00110, 0b01001, 0b01000, 0b11100, 0b01000, 0b01000, 0b01000,
        ],
        'g' => [
            0b00000, 0b01111, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110,
        ],
        'h' => [
            0b10000, 0b10000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001,
        ],
        'i' => [
            0b00100, 0b00000, 0b01100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'j' => [
            0b00010, 0b00000, 0b00110, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'k' => [
            0b10000, 0b10000, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010,
        ],
        'l' => [
            0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        'm' => [
            0b00000, 0b11010, 0b10101, 0b10101, 0b10101, 0b10101, 0b10101,
        ],
        'n' => [
            0b00000, 0b10110, 0b11001, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'o' => [
            0b00000, 0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'p' => [
            0b00000, 0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000,
        ],
        'q' => [
            0b00000, 0b01101, 0b10011, 0b10001, 0b01111, 0b00001, 0b00001,
        ],
        'r' => [
            0b00000, 0b10110, 0b11001, 0b10000, 0b10000, 0b10000, 0b10000,
        ],
        's' => [
            0b00000, 0b01111, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        't' => [
            0b01000, 0b01000, 0b11100, 0b01000, 0b01000, 0b01001, 0b00110,
        ],
        'u' => [
            0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b10011, 0b01101,
        ],
        'v' => [
            0b00000, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'w' => [
            0b00000, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        'x' => [
            0b00000, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'y' => [
            0b00000, 0b10001, 0b10001, 0b10001, 0b01111, 0b00001, 0b01110,
        ],
        'z' => [
            0b00000, 0b11111, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b00001, 0b00001, 0b11110,
        ],
        '6' => [
            0b01110, 0b10000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110,
        ],
        ':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        '.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        ',' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110, 0b00100,
        ],
        '+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        '-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        '/' => [
            0b00001, 0b00010, 0b00100, 0b00100, 0b01000, 0b10000, 0b00000,
        ],
        '|' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        '(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        ')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        '\'' => [
            0b00100, 0b00100, 0b00010, 0b00000, 0b00000, 0b00000, 0b00000,
        ],
        '=' => [
            0b00000, 0b11111, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000,
        ],
        '?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        '#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        '·' => [
            0b00000, 0b00000, 0b00000, 0b00100, 0b00000, 0b00000, 0b00000,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        _ => bitmap_glyph('?'),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::HudTheme;
    use crate::types::{Scene, Size, Text};

    #[test]
    fn text_tessellation_produces_triangles() {
        let mut scene = Scene::new(Size::new(240.0, 80.0));
        scene.text(Text {
            rect: Rect::new(Point::new(8.0, 8.0), Size::new(220.0, 24.0)),
            content: "Undo Ctrl/Cmd+Z".to_string(),
            style: HudTheme::default().body_style,
        });

        let triangles = tessellate_scene(&scene);
        assert!(!triangles.is_empty());
        assert!(triangles.iter().all(|triangle| triangle.color.a > 0.0));
    }

    #[test]
    fn long_text_wraps_and_ellipsizes() {
        let lines = wrap_text_lines("selected terrain patch with a very long action hint", 12, 2);

        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with("..."));
    }

    #[test]
    fn bitmap_font_has_own_glyphs_for_hash_and_middle_dot() {
        let hash = bitmap_glyph('#');
        let dot = bitmap_glyph('·');
        let fallback = bitmap_glyph('?');
        assert_ne!(hash, fallback, "'#' should have its own glyph");
        assert_ne!(dot, fallback, "'·' should have its own glyph");
        assert!(hash.iter().any(|row| *row != 0));
        assert!(dot.iter().any(|row| *row != 0));
    }
}
