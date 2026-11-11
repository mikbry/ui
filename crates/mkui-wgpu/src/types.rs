//! Shared GUI data types.
//!
//! These are pure data: geometry primitives, text/font handles, layout inputs,
//! and the generic hit-testing contract. Nothing here depends on the render
//! backend or on product-specific domain types, so the whole module can be shared
//! between native, web, and future backend shells.

use std::fmt;
#[cfg(feature = "slug")]
use std::sync::Arc;

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

/// An RGBA color, authored in **sRGB perceptual space** (a designer-picked
/// shade, `0.0..=1.0` per channel; alpha is a linear coverage weight, never
/// gamma-encoded).
///
/// # Color-space contract (ADR 0006 §"Color space + blending")
///
/// mkui renders in **linear space**: the render pass composites into a linear
/// intermediate framebuffer and a final present pass encodes linear → sRGB at
/// the surface boundary. Every `Color` value is therefore converted to linear
/// exactly once, at that boundary, via [`Color::to_linear_rgba`]. Callers keep
/// authoring perceptual colors (`Color::rgb` / `Color::from_srgb`); they never
/// pre-linearize by hand, and no color literal in the tree stores linear
/// channels (the color-literal audit confirmed all are sRGB perceptual, alpha
/// linear). Blending in the wrong (sRGB-encoded) space — the pre-Sprint-8 bug
/// this contract closes — darkened partial-coverage edges on anti-aliased text.
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
    /// A Slug vector glyph (#66), drawn through the `mkui-vector2d-wgpu`
    /// coverage pipeline rather than tessellated to triangles. Present only
    /// under the `slug` feature — the variant *is* the scene-level seam #67's
    /// outline text system emits into; the renderer collects these in scene
    /// order and dispatches them on the Slug lane via the ordered command
    /// stream (see [`crate::render_command`]).
    #[cfg(feature = "slug")]
    SlugGlyph(mkui_vector2d_wgpu::PlacedSlugGlyph),
    /// An arbitrary vector path fill and/or stroke (#139), the Sprint 8 Wave 1
    /// scene-level seam parallel to [`Primitive::SlugGlyph`]: consumer
    /// components (button/card/dot, Wave 2 #140-#142) emit this instead of a
    /// triangulated approximation. Drawn through the same `slug` coverage
    /// pipeline, on the ordered command stream (see [`crate::render_command`]).
    #[cfg(feature = "slug")]
    VectorPath(PlacedVectorPath),
}

/// An arbitrary [`mkui_vector2d_wgpu::VectorPath`] fill and/or stroke, placed
/// on screen (#139). Parallels [`PlacedSlugGlyph`](mkui_vector2d_wgpu::PlacedSlugGlyph) ---
/// `path` is `Arc`-shared so re-pushing the same primitive across frames (a
/// component's steady-state scene) only bumps a refcount at the *scene*
/// level, never re-clones the path geometry there; the renderer still copies
/// the geometry once per frame when lowering to the adapter's own
/// `PlacedVectorPath`/`PlacedStroke` (#138's public types, which own their
/// geometry rather than sharing it). The placement (`origin_px` /
/// `scale_px_per_unit`) matches the Slug glyph lane so a path scales with
/// zoom exactly like a glyph.
///
/// `stroke` and `fill_color` combine independently:
/// - `stroke: None` --- fill only, painted with `fill_color` (must be `Some`).
/// - `stroke: Some`, `fill_color: None` --- stroke only, painted with
///   `stroke_color`.
/// - `stroke: Some`, `fill_color: Some` --- fill *and* stroke, each with its
///   own color (`fill_color` for the fill, `stroke_color` for the outline).
/// - Both `stroke` and `fill_color` `None` paints nothing; the renderer skips
///   it rather than treating it as an error.
///
/// Scope decision (#139): the issue's original sketch of this struct had no
/// stroke color field, but Sprint 8 plan v2 §2.2 is explicit that `Stroke` is
/// `mkui_vector2d::Stroke`'s cap/join/dash descriptor (held "alongside"
/// `VectorPath`, not embedding a color) and that Sprint 8 "ships solid-color
/// fill + solid-color stroke" --- a stroke needs a paint color independent of
/// `fill_color` to satisfy that. `stroke_color` is the one field this struct
/// adds beyond the issue's sketch to close that gap; it is read only when
/// `stroke` is `Some`. `stroke.width_px` and the adapter's `scale_px_per_unit`
/// placement scaling are unchanged from `mkui_vector2d_wgpu::PlacedStroke` ---
/// no local reinterpretation of stroke units here.
#[cfg(feature = "slug")]
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedVectorPath {
    /// The backend-neutral fill/stroke-centreline geometry.
    pub path: Arc<mkui_vector2d_wgpu::VectorPath>,
    /// How to paint the path's outline (width, caps, joins, dash), or `None`
    /// for fill-only.
    pub stroke: Option<mkui_vector2d_wgpu::Stroke>,
    /// The stroke's paint color. Ignored when `stroke` is `None`.
    pub stroke_color: Color,
    /// The fill color, or `None` for stroke-only.
    pub fill_color: Option<Color>,
    /// Screen-pixel position (y-down) of the path's origin `(0, 0)`.
    pub origin_px: [f32; 2],
    /// Pixels per path unit (placement scale).
    pub scale_px_per_unit: f32,
}

/// Generic motion primitive emitted by atoms like `dot`. The renderer
/// interprets the kind into per-frame animation behaviour (alpha pulse,
/// rotation, …); the static [`Primitive`] list keeps a stationary
/// representation so headless / golden-image tests stay stable.
///
/// `None` is the resting value — atoms only push an instance into
/// [`Scene::animations`] when the kind is non-`None`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DotAnimation {
    #[default]
    None,
    Pulse,
    PulseUrgent,
    Spin,
}

/// One active animation in a [`Scene`]. The renderer keys per-frame motion
/// off `kind`, anchored at `center` with `radius` for amplitude.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DotAnimationInstance {
    pub center: Point,
    pub radius: f32,
    pub kind: DotAnimation,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub viewport: Size,
    pub primitives: Vec<Primitive>,
    pub animations: Vec<DotAnimationInstance>,
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

/// Point-in-rounded-rect test (#176: exact hit-testing for embedding hosts).
///
/// `rect_contains` treats every quad as a plain rectangle, so a click in a
/// corner that `tessellation::clamp_corner_radii` + `push_rounded_rect_fill`
/// chamfer away from the rendered silhouette would still register as a hit —
/// visually missing the control but logically landing on it. This test
/// clips each corner the same way the renderer does (radius clamped to half
/// the shorter side, exactly `tessellation::clamp_corner_radii`'s formula)
/// and rejects a point that falls in the clipped-off triangle outside the
/// corner's quarter-circle.
///
/// Zero radii (the common case: most controls aren't rounded) degrade to a
/// plain bounding-box test — every point that passes `rect_contains` is
/// trivially outside all four (zero-radius) corner-exclusion zones.
pub fn rect_contains_rounded(rect: Rect, corner_radii: CornerRadii, point: Point) -> bool {
    if !rect_contains(rect, point) {
        return false;
    }

    let max_r = (rect.size.width.max(0.0).min(rect.size.height.max(0.0)) * 0.5).max(0.0);
    let top_left = corner_radii.top_left.clamp(0.0, max_r);
    let top_right = corner_radii.top_right.clamp(0.0, max_r);
    let bottom_right = corner_radii.bottom_right.clamp(0.0, max_r);
    let bottom_left = corner_radii.bottom_left.clamp(0.0, max_r);

    let left = rect.origin.x;
    let top = rect.origin.y;
    let right = rect.origin.x + rect.size.width;
    let bottom = rect.origin.y + rect.size.height;

    if top_left > 0.0 && point.x < left + top_left && point.y < top + top_left {
        return within_corner_circle(point, Point::new(left + top_left, top + top_left), top_left);
    }
    if top_right > 0.0 && point.x > right - top_right && point.y < top + top_right {
        return within_corner_circle(
            point,
            Point::new(right - top_right, top + top_right),
            top_right,
        );
    }
    if bottom_right > 0.0 && point.x > right - bottom_right && point.y > bottom - bottom_right {
        return within_corner_circle(
            point,
            Point::new(right - bottom_right, bottom - bottom_right),
            bottom_right,
        );
    }
    if bottom_left > 0.0 && point.x < left + bottom_left && point.y > bottom - bottom_left {
        return within_corner_circle(
            point,
            Point::new(left + bottom_left, bottom - bottom_left),
            bottom_left,
        );
    }
    true
}

fn within_corner_circle(point: Point, center: Point, radius: f32) -> bool {
    let dx = point.x - center.x;
    let dy = point.y - center.y;
    dx * dx + dy * dy <= radius * radius
}

/// sRGB → linear transfer (the sRGB EOTF). Maps a gamma-encoded perceptual
/// channel in `[0, 1]` to its linear-light value so alpha blending composites
/// in the physically-correct space. `0.0`/`1.0` are fixed points; `0.5` → ~0.214.
/// Inverse of [`linear_to_srgb`]. Shared by [`Color::to_linear_rgba`] and the
/// renderer's clear/vertex boundary; the present shader carries the same math
/// in WGSL for the encode direction.
pub fn srgb_to_linear(component: f32) -> f32 {
    let component = component.clamp(0.0, 1.0);
    if component <= 0.04045 {
        component / 12.92
    } else {
        ((component + 0.055) / 1.055).powf(2.4)
    }
}

/// linear → sRGB transfer (the sRGB OETF). Inverse of [`srgb_to_linear`]. The
/// windowed present pass performs this encode on the GPU (see `present.wgsl`);
/// this CPU copy backs the round-trip unit tests and any headless encode.
pub fn linear_to_srgb(component: f32) -> f32 {
    let component = component.clamp(0.0, 1.0);
    if component <= 0.0031308 {
        component * 12.92
    } else {
        1.055 * component.powf(1.0 / 2.4) - 0.055
    }
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

    /// Expand to at least `min` on each axis, independently per axis, about
    /// the same center — unlike [`Self::expand`], which grows both axes by
    /// the same amount. A dimension that already meets `min` is left
    /// untouched. Used by widgets (e.g. `button`'s hit-target) whose
    /// *visual* rect may be smaller than a platform's minimum hit-target
    /// size on one or both axes.
    pub fn expand_to_min_size(self, min: Size) -> Self {
        let extra_w = (min.width - self.size.width).max(0.0);
        let extra_h = (min.height - self.size.height).max(0.0);
        Self {
            origin: Point::new(self.origin.x - extra_w * 0.5, self.origin.y - extra_h * 0.5),
            size: Size::new(self.size.width + extra_w, self.size.height + extra_h),
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
    /// Fully transparent — "no paint". A sentinel distinct from any named
    /// theme token: it represents the absence of a themed value, not a
    /// palette choice, so resolvers may use it without pulling a
    /// `ThemeTokens` field (see issue #205).
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// Construct an opaque color from **sRGB perceptual** channels.
    ///
    /// Semantically identical to [`rgb`](Self::rgb) — `Color` is defined in
    /// sRGB space — but the explicit spelling documents intent at call sites
    /// that pass raw literals, per ADR 0006 §"Color space + blending". The
    /// perceptual → linear conversion happens once at the render boundary
    /// ([`to_linear_rgba`](Self::to_linear_rgba)); do not pre-linearize here.
    pub const fn from_srgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    /// [`from_srgb`](Self::from_srgb) with an explicit linear alpha coverage.
    pub const fn from_srgb_a(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    /// Convert this sRGB-perceptual color to the **linear** RGBA the render
    /// pass composites with. The RGB channels pass through the sRGB EOTF
    /// ([`srgb_to_linear`]); alpha is a linear coverage weight and is copied
    /// through unchanged. This is the single color-space boundary conversion —
    /// see the type-level contract.
    pub fn to_linear_rgba(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a,
        ]
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
            animations: Vec::new(),
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

    /// Record a Slug vector glyph (#66) at its scene position. The glyph draws
    /// on the Slug lane in the order it was pushed relative to other
    /// primitives. Available only under the `slug` feature.
    #[cfg(feature = "slug")]
    pub fn slug_glyph(&mut self, glyph: mkui_vector2d_wgpu::PlacedSlugGlyph) {
        self.push(Primitive::SlugGlyph(glyph));
    }

    /// Record an arbitrary vector path fill/stroke (#139) at its scene
    /// position. Draws on the same Slug lane as [`Scene::slug_glyph`], in the
    /// order it was pushed relative to other primitives. Available only under
    /// the `slug` feature.
    #[cfg(feature = "slug")]
    pub fn vector_path(&mut self, path: PlacedVectorPath) {
        self.push(Primitive::VectorPath(path));
    }

    /// Record a non-`None` animation. Atoms emit at most one instance per
    /// call; `DotAnimation::None` is a no-op so callers can pass the value
    /// straight through without an outer match.
    pub fn animate(&mut self, instance: DotAnimationInstance) {
        if matches!(instance.kind, DotAnimation::None) {
            return;
        }
        self.animations.push(instance);
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
        let mut layout = PanelLayout::<&'static str> {
            panel_rect: Some(Rect::new(Point::new(0.0, 0.0), Size::new(200.0, 200.0))),
            ..Default::default()
        };
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
        let layout = PanelLayout::<()> {
            panel_rect: Some(Rect::new(Point::new(100.0, 50.0), Size::new(50.0, 40.0))),
            ..Default::default()
        };

        assert!(layout.contains_panel(Point::new(120.0, 70.0)));
        assert!(!layout.contains_panel(Point::new(10.0, 10.0)));
        assert!(layout.hit(Point::new(120.0, 70.0)).is_none());
    }

    #[test]
    fn panel_layout_contains_panel_is_false_without_rect() {
        let layout: PanelLayout<()> = PanelLayout::default();
        assert!(!layout.contains_panel(Point::new(0.0, 0.0)));
    }

    #[test]
    fn srgb_linear_fixed_points_and_midpoint() {
        // 0 and 1 are fixed points in both directions.
        assert!(srgb_to_linear(0.0).abs() < f32::EPSILON);
        assert!((srgb_to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!(linear_to_srgb(0.0).abs() < f32::EPSILON);
        assert!((linear_to_srgb(1.0) - 1.0).abs() < 1e-6);
        // sRGB 0.5 → ~0.214 linear (the canonical mid-gray value).
        assert!((srgb_to_linear(0.5) - 0.21404114).abs() < 1e-5);
    }

    #[test]
    fn srgb_linear_round_trips() {
        for step in 0..=20 {
            let c = step as f32 / 20.0;
            let round = linear_to_srgb(srgb_to_linear(c));
            assert!((round - c).abs() < 1e-4, "round-trip failed at {c}");
        }
    }

    #[test]
    fn to_linear_rgba_linearizes_rgb_but_preserves_alpha() {
        // Alpha is a linear coverage weight and must pass through untouched,
        // while RGB is linearized. A 50%-alpha mid-gray is the discriminating
        // case: sRGB-space blending (the old bug) would have used 0.5 for the
        // color channels instead of ~0.214.
        let linear = Color::from_srgb_a(0.5, 0.5, 0.5, 0.5).to_linear_rgba();
        assert!((linear[0] - 0.21404114).abs() < 1e-5);
        assert!((linear[1] - 0.21404114).abs() < 1e-5);
        assert!((linear[2] - 0.21404114).abs() < 1e-5);
        assert_eq!(linear[3], 0.5, "alpha stays linear");
    }

    #[test]
    fn from_srgb_matches_rgb_storage() {
        // The intent-signalling constructor stores the same channels as `rgb` —
        // the conversion is deferred to the render boundary, not construction.
        assert_eq!(Color::from_srgb(0.2, 0.4, 0.6), Color::rgb(0.2, 0.4, 0.6));
        assert_eq!(
            Color::from_srgb_a(0.2, 0.4, 0.6, 0.8),
            Color::rgba(0.2, 0.4, 0.6, 0.8)
        );
    }

    // ---- #176: rect_contains_rounded (exact hit-testing) ----

    #[test]
    fn rounded_hit_test_matches_plain_rect_for_zero_radii() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(100.0, 50.0));
        let zero = CornerRadii::all(0.0);
        for p in [
            Point::new(0.0, 0.0),
            Point::new(99.0, 1.0),
            Point::new(1.0, 49.0),
            Point::new(50.0, 25.0),
        ] {
            assert_eq!(rect_contains_rounded(rect, zero, p), rect_contains(rect, p));
        }
    }

    #[test]
    fn rounded_hit_test_rejects_the_clipped_off_corner() {
        // A 40x40 square with a 20px corner radius: (2, 2) is well inside the
        // bounding box but sits in the triangular sliver the renderer clips
        // away from the top-left quarter-circle.
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0));
        let radii = CornerRadii::all(20.0);
        assert!(
            !rect_contains_rounded(rect, radii, Point::new(2.0, 2.0)),
            "corner pixel outside the rounded silhouette must miss"
        );
        assert!(
            rect_contains(rect, Point::new(2.0, 2.0)),
            "sanity: the plain bounding box still contains it"
        );
    }

    #[test]
    fn rounded_hit_test_accepts_the_corner_arc_and_the_center() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0));
        let radii = CornerRadii::all(20.0);
        // Inside the top-left corner's exclusion box (x < 20, y < 20) but
        // within the quarter-circle arc itself: dx=-5, dy=-14 from the
        // circle's center (20, 20), distance² = 221 <= 400.
        assert!(rect_contains_rounded(rect, radii, Point::new(15.0, 6.0)));
        // Dead center of the square is always inside, radii or not.
        assert!(rect_contains_rounded(rect, radii, Point::new(20.0, 20.0)));
    }

    #[test]
    fn rounded_hit_test_only_clips_rounded_corners() {
        // Asymmetric radii: only the top-left corner is rounded. The
        // top-right corner pixel must still hit even though it's as close to
        // its own (zero-radius) corner as the rejected top-left pixel was.
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0));
        let radii = CornerRadii {
            top_left: 20.0,
            top_right: 0.0,
            bottom_right: 0.0,
            bottom_left: 0.0,
        };
        assert!(!rect_contains_rounded(rect, radii, Point::new(2.0, 2.0)));
        assert!(rect_contains_rounded(rect, radii, Point::new(38.0, 2.0)));
    }

    #[test]
    fn rounded_hit_test_clamps_oversized_radii_like_the_renderer() {
        // A radius far larger than half the rect's shorter side is clamped
        // before the corner test runs — mirroring
        // `tessellation::clamp_corner_radii` so hit-testing and rendering
        // never disagree about the silhouette.
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 20.0));
        let radii = CornerRadii::all(1000.0);
        // Clamped radius is 10 (half the shorter side), circle centered at
        // (10, 10). (3, 3) is distance ~9.9 from that center — inside the
        // clamped arc.
        assert!(rect_contains_rounded(rect, radii, Point::new(3.0, 3.0)));
        // (2, 2) is distance ~11.3 from the same center — just outside the
        // clamped arc, proving the clamp (not an unclamped 1000px radius)
        // governs the test.
        assert!(!rect_contains_rounded(rect, radii, Point::new(2.0, 2.0)));
    }

    #[test]
    fn rounded_hit_test_rejects_outside_bounding_box_first() {
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(40.0, 40.0));
        let radii = CornerRadii::all(10.0);
        assert!(!rect_contains_rounded(
            rect,
            radii,
            Point::new(100.0, 100.0)
        ));
    }
}
