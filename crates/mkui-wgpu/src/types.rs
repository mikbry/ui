//! Shared GUI data types.
//!
//! These are pure data: geometry primitives, text/font handles, layout inputs,
//! and the generic hit-testing contract. Nothing here depends on the render
//! backend or on product-specific domain types, so the whole module can be shared
//! between native, web, and future backend shells.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Insets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FontFaceId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct IconId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font: FontFaceId,
    pub glyph_index: u32,
    pub px_per_em: u16,
    pub subpixel_bin: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shadow {
    pub rect: Rect,
    pub blur_radius: f32,
    pub spread: f32,
    pub color: Color,
    pub corner_radii: CornerRadii,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad {
    pub rect: Rect,
    pub fill: Color,
    pub corner_radii: CornerRadii,
    pub stroke: Option<Stroke>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub rect: Rect,
    pub content: String,
    pub style: TextStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Icon {
    pub rect: Rect,
    pub icon: IconId,
    pub tint: Color,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Primitive {
    Shadow(Shadow),
    Quad(Quad),
    Text(Text),
    Icon(Icon),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub viewport: Size,
    pub primitives: Vec<Primitive>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GuiTriangle {
    pub points: [Point; 3],
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontFaceId,
    pub font_size_px: f32,
    pub line_height_px: f32,
    pub color: Color,
    pub align: TextAlign,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextBuffer {
    pub font: FontFaceId,
    pub size: Size,
    pub text: String,
    pub style: TextStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlyphPlacement {
    pub key: GlyphKey,
    pub atlas_rect: AtlasRect,
    pub screen_rect: Rect,
    pub color: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RasterizedGlyph {
    pub key: GlyphKey,
    pub width: u16,
    pub height: u16,
    pub bearing_x: i16,
    pub bearing_y: i16,
    pub advance_px: f32,
    pub alpha_mask: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IconMask {
    pub icon: IconId,
    pub width: u16,
    pub height: u16,
    pub alpha_mask: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Constraints {
    pub min: Size,
    pub max: Size,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StackStyle {
    pub axis: Axis,
    pub gap: f32,
    pub padding: Insets,
}

/// A clickable region with an associated target identifier. Overlay panels
/// push these during layout and apps hit-test them on pointer events. The
/// target type is left generic so each panel can use its own enum (tool id,
/// element key, inspector field, …) without coupling to this crate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRegion<T> {
    pub rect: Rect,
    pub target: T,
}

/// Layout snapshot for an overlay panel: the outer chrome rect plus a list of
/// interactive regions. Clicks that land inside `panel_rect` but outside every
/// entry in `hit_regions` should still be swallowed so the panel doesn't let
/// tool actions fire through its empty space.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelLayout<T> {
    pub panel_rect: Option<Rect>,
    pub hit_regions: Vec<HitRegion<T>>,
}

impl<T> Default for PanelLayout<T> {
    fn default() -> Self {
        Self {
            panel_rect: None,
            hit_regions: Vec::new(),
        }
    }
}

impl<T> PanelLayout<T> {
    /// Finds the topmost region containing `point`. Later regions win, so
    /// small widgets drawn over a larger background (steppers over a slider
    /// track) are picked in preference to the backdrop.
    pub fn hit(&self, point: Point) -> Option<&HitRegion<T>> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| rect_contains(region.rect, point))
    }

    /// True when `point` lands inside the panel chrome, even if no widget
    /// region matched. Use this from apps to consume clicks on the panel's
    /// empty space instead of letting them fall through to the viewport.
    pub fn contains_panel(&self, point: Point) -> bool {
        self.panel_rect
            .map(|rect| rect_contains(rect, point))
            .unwrap_or(false)
    }
}

/// Point-in-rect test, treating the rect as half-open on the top / left and
/// closed on the bottom / right. Exposed so panels that keep their own layout
/// type (without the full `PanelLayout<T>` wrapper) share the same contract.
pub fn rect_contains(rect: Rect, point: Point) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

impl Rect {
    pub const fn new(origin: Point, size: Size) -> Self {
        Self { origin, size }
    }

    pub fn expand(self, amount: f32) -> Self {
        Self {
            origin: Point::new(self.origin.x - amount, self.origin.y - amount),
            size: Size::new(
                (self.size.width + amount * 2.0).max(0.0),
                (self.size.height + amount * 2.0).max(0.0),
            ),
        }
    }

    pub fn inset(self, insets: Insets) -> Self {
        Self {
            origin: Point::new(self.origin.x + insets.left, self.origin.y + insets.top),
            size: Size::new(
                (self.size.width - insets.left - insets.right).max(0.0),
                (self.size.height - insets.top - insets.bottom).max(0.0),
            ),
        }
    }

    pub fn height_end(self) -> f32 {
        self.origin.y + self.size.height
    }

    pub fn width_end(self) -> f32 {
        self.origin.x + self.size.width
    }
}

impl Insets {
    pub const fn all(value: f32) -> Self {
        Self {
            left: value,
            top: value,
            right: value,
            bottom: value,
        }
    }

    pub const fn symmetric(horizontal: f32, vertical: f32) -> Self {
        Self {
            left: horizontal,
            top: vertical,
            right: horizontal,
            bottom: vertical,
        }
    }
}

impl CornerRadii {
    pub const fn all(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }
}

impl Color {
    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const fn with_alpha(self, a: f32) -> Self {
        Self { a, ..self }
    }

    pub const fn multiply_alpha(self, factor: f32) -> Self {
        Self {
            a: self.a * factor,
            ..self
        }
    }
}

impl Scene {
    pub fn new(viewport: Size) -> Self {
        Self {
            viewport,
            primitives: Vec::new(),
        }
    }

    pub fn push(&mut self, primitive: Primitive) {
        self.primitives.push(primitive);
    }

    pub fn shadow(&mut self, shadow: Shadow) {
        self.push(Primitive::Shadow(shadow));
    }

    pub fn quad(&mut self, quad: Quad) {
        self.push(Primitive::Quad(quad));
    }

    pub fn text(&mut self, text: Text) {
        self.push(Primitive::Text(text));
    }

    pub fn icon(&mut self, icon: Icon) {
        self.push(Primitive::Icon(icon));
    }
}

impl Constraints {
    pub const fn new(min: Size, max: Size) -> Self {
        Self { min, max }
    }
}

pub struct StackCursor {
    rect: Rect,
    style: StackStyle,
    cursor: Point,
}

impl StackCursor {
    pub fn new(rect: Rect, style: StackStyle) -> Self {
        let cursor = Point::new(
            rect.origin.x + style.padding.left,
            rect.origin.y + style.padding.top,
        );
        Self {
            rect,
            style,
            cursor,
        }
    }

    pub fn next(&mut self, size: Size) -> Rect {
        let rect = Rect::new(self.cursor, size);
        match self.style.axis {
            Axis::Horizontal => {
                self.cursor.x += size.width + self.style.gap;
            }
            Axis::Vertical => {
                self.cursor.y += size.height + self.style.gap;
            }
        }
        rect
    }

    pub fn content_bounds(&self) -> Rect {
        self.rect.inset(self.style.padding)
    }
}

pub trait Element {
    fn layout(&mut self, constraints: Constraints) -> Size;
    fn paint(&mut self, scene: &mut Scene, rect: Rect);
}

impl fmt::Display for FontFaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FontFaceId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stack_cursor_flows_vertically() {
        let mut cursor = StackCursor::new(
            Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 200.0)),
            StackStyle {
                axis: Axis::Vertical,
                gap: 8.0,
                padding: Insets::all(10.0),
            },
        );
        let first = cursor.next(Size::new(100.0, 20.0));
        let second = cursor.next(Size::new(100.0, 20.0));

        assert_eq!(first.origin.y, 10.0);
        assert_eq!(second.origin.y, 38.0);
    }

    #[test]
    fn panel_layout_hit_prefers_later_region_on_overlap() {
        let mut layout: PanelLayout<&'static str> = PanelLayout::default();
        layout.panel_rect = Some(Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 200.0)));
        layout.hit_regions.push(HitRegion {
            rect: Rect::new(Point::new(10.0, 10.0), Size::new(100.0, 100.0)),
            target: "background",
        });
        layout.hit_regions.push(HitRegion {
            rect: Rect::new(Point::new(40.0, 40.0), Size::new(20.0, 20.0)),
            target: "foreground",
        });

        let hit = layout.hit(Point::new(50.0, 50.0)).expect("point inside");
        assert_eq!(hit.target, "foreground");
    }

    #[test]
    fn panel_layout_contains_panel_only_inside_rect() {
        let mut layout: PanelLayout<()> = PanelLayout::default();
        layout.panel_rect = Some(Rect::new(Point::new(100.0, 50.0), Size::new(50.0, 40.0)));

        assert!(layout.contains_panel(Point::new(120.0, 70.0)));
        assert!(!layout.contains_panel(Point::new(10.0, 10.0)));
        assert!(layout.hit(Point::new(120.0, 70.0)).is_none());
    }

    #[test]
    fn panel_layout_contains_panel_is_false_without_rect() {
        let layout: PanelLayout<()> = PanelLayout::default();
        assert!(!layout.contains_panel(Point::new(0.0, 0.0)));
    }
}
