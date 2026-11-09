//! Sprint 8 Wave 2 vector-primitives showcase (#143).
//!
//! Renders button / card / dot as a 3x2 grid: the left column is a
//! showcase-local faceted-polygon approximation of the same rounded-rect
//! shape every atom in `mkui-wgpu::components` draws via `Quad` +
//! `CornerRadii::all(r)` (see the "Scope note" below for why it isn't the
//! real `Quad` path); the right column emits the identical rounded-rect
//! geometry through
//! `Primitive::VectorPath` (#139) — one `moveTo` plus a `cubicTo` arc per
//! corner. A maximally-radiused rounded rect (`r == size/2` on a square)
//! closes into the dot row's circle, so one path builder covers all three
//! rows: the button/card corners (#140/#141) and the dot's full-circle case
//! (#142) are the same construction at different radii. Corners use a cubic
//! (not quadratic) Bézier arc with the standard circle-approximation handle
//! length `k = 4/3 * (sqrt(2) - 1) ≈ 0.5523 * r` — a single `quadTo` per 90°
//! corner overshoots by ~6% of `r` (a visible squircle at the dot's ~55px
//! radius), while the cubic form is accurate to ~0.03%. `encode_vector_path`
//! (fills) and `stroke_to_fill` (strokes) both subdivide cubics already, so
//! this needs no new pipeline support.
//!
//! Sizes are well above the real shadcn-token scale (dot radius ~55px, card
//! ~150px tall) specifically so the triangulated-vs-vector difference reads
//! at 1x window scale with no zoom, per the issue's close-blocker clause.
//!
//! - `cargo run -p vector-showcase --release --features slug` opens the
//!   window with both columns live.
//! - `cargo run -p vector-showcase --release` (no `slug`) opens the window
//!   with the right column falling back to the same faceted-polygon
//!   approximation as the left — the zero-API-break contract #140-#142
//!   commit to for the real atoms, mirrored here for this demo's own shapes.
//! - `HEADLESS=1` (either build) exits after a single walk pass — see
//!   `mkui_wgpu::Mkui::run`.
//!
//! Scope note (acceptance criterion "triangulated column shows visible
//! polygons"): `mkui-wgpu`'s shared native tessellator
//! (`tessellate_quad`/`push_rect`) does not consume `Quad::corner_radii` at
//! all — a rounded `Quad` renders as a plain square there, not a faceted
//! polygon. That's the exact, documented, GPU-test-asserted baseline #142's
//! own `render/dot_vector_gpu.rs` builds its triangulated-vs-vector
//! comparison against, and this showcase does **not** touch that shared
//! code — an earlier revision of this PR did, and had to be reverted
//! because it silently broke #142's merged acceptance test and changed the
//! rendered look of every `CornerRadii`-using atom in the toolkit
//! (button/card/dot/badge/panel/scrollbar/slider/swatch/text_field). #212
//! still tracks that production-wide gap.
//!
//! Instead, the left column's "triangulated" shape is built *locally*, only
//! in this example: [`rounded_rect_polygon`] generates a genuine low-facet
//! polygon (straight chords between [`ROUNDED_CORNER_SEGMENTS`]-per-corner
//! sample points — a 12-gon for the dot's full circle), and
//! [`push_faceted_polygon_fill`] rasterizes it into many single-row `Quad`
//! primitives (a scanline fill, exact for this shape's straight edges since
//! it's always convex). The result is real, inspectable facet geometry —
//! not the production `Quad` path, and not a claim that the shared
//! tessellator gap is fixed — that demonstrates the polygon-vs-smooth-curve
//! contrast the issue asks for without touching code #142 depends on. The
//! on-screen caption below names this explicitly.

use mkui_wgpu::components::label;
use mkui_wgpu::{
    Color, CornerRadii, FontFaceId, Point, Quad, Rect, Scene, Size, Stroke, TextAlign, TextStyle,
};

const WINDOW: Size = Size::new(860.0, 660.0);

const COL_LEFT_X: f32 = 60.0;
const COL_RIGHT_X: f32 = 460.0;
const COL_WIDTH: f32 = 340.0;
const ROW_LABEL_X: f32 = 8.0;

const FILL: Color = Color::rgba(0.94, 0.79, 0.48, 0.92);
const BORDER: Color = Color::rgba(0.96, 0.93, 0.88, 0.8);
const CARD_FILL: Color = Color::rgba(0.16, 0.13, 0.11, 0.92);
const DOT_FILL: Color = Color::rgba(0.36, 0.74, 0.45, 0.96);

// Row geometry — module consts (not locals in `build_scene`) so tests share
// one source of truth for each row's position/size/radius instead of
// duplicating (and risking drift from) these literals.
const BUTTON_Y: f32 = 150.0;
const BUTTON_SIZE: Size = Size::new(260.0, 70.0);
const BUTTON_RADIUS: f32 = 18.0;
const BUTTON_STROKE_WIDTH: f32 = 2.0;
const CARD_Y: f32 = BUTTON_Y + BUTTON_SIZE.height + 30.0;
const CARD_SIZE: Size = Size::new(300.0, 150.0);
const CARD_RADIUS: f32 = 36.0;
const CARD_STROKE_WIDTH: f32 = 1.5;
const DOT_Y: f32 = CARD_Y + CARD_SIZE.height + 30.0;
const DOT_SIZE: Size = Size::new(110.0, 110.0);
const DOT_RADIUS: f32 = DOT_SIZE.width * 0.5;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let scene = build_scene();
    println!(
        "vector-primitives showcase: {} scene primitive(s) — close the window to exit",
        scene.primitives.len()
    );
    mkui_wgpu::Mkui::with_scene(scene).run()?;
    Ok(())
}

fn heading_style() -> TextStyle {
    TextStyle {
        font: FontFaceId(0),
        font_size_px: 20.0,
        line_height_px: 26.0,
        color: Color::rgb(0.98, 0.95, 0.90),
        align: TextAlign::Start,
    }
}

fn caption_style() -> TextStyle {
    TextStyle {
        font: FontFaceId(0),
        font_size_px: 13.0,
        line_height_px: 18.0,
        color: Color::rgba(0.92, 0.88, 0.82, 0.92),
        align: TextAlign::Start,
    }
}

fn column_header_style() -> TextStyle {
    TextStyle {
        font: FontFaceId(0),
        font_size_px: 14.0,
        line_height_px: 18.0,
        color: Color::rgba(0.94, 0.79, 0.48, 0.95),
        align: TextAlign::Start,
    }
}

fn row_label_style() -> TextStyle {
    TextStyle {
        font: FontFaceId(0),
        font_size_px: 12.0,
        line_height_px: 16.0,
        color: Color::rgba(0.85, 0.82, 0.78, 0.85),
        align: TextAlign::Start,
    }
}

/// Facet count per 90° rounded corner for the showcase's own "triangulated"
/// baseline (not `mkui-wgpu`'s shared tessellator — see the module docs). A
/// full circle (4 corners) comes out a 12-gon: deliberately low enough to
/// read as visibly polygonal next to the vector lane's true curve at the
/// dot row's ~55px radius, with no zoom.
const ROUNDED_CORNER_SEGMENTS: usize = 3;

/// Append one rounded corner's sample points (`segments + 1` points,
/// standard math angle convention: `0` points `+x`, increasing angle
/// rotates toward `+y`, i.e. "down" in this file's y-down pixel space — no
/// adapter y-flip here, unlike the `slug`-only `rounded_rect_commands`) to
/// `verts`, as straight chords between points on the circle of `radius`
/// centred at
/// `(cx, cy)`. `radius <= 0` degenerates to the single literal corner point
/// `(cx, cy)` — a sharp corner needs no arc.
fn push_corner_points(
    verts: &mut Vec<(f32, f32)>,
    cx: f32,
    cy: f32,
    radius: f32,
    start_angle: f32,
    end_angle: f32,
) {
    if radius <= 0.0 {
        verts.push((cx, cy));
        return;
    }
    let step = (end_angle - start_angle) / ROUNDED_CORNER_SEGMENTS as f32;
    for i in 0..=ROUNDED_CORNER_SEGMENTS {
        let angle = start_angle + step * i as f32;
        verts.push((cx + radius * angle.cos(), cy + radius * angle.sin()));
    }
}

/// A closed, convex polygon (ordered perimeter points, `0..w, 0..h` local
/// pixel space) approximating a rounded rect: straight edges plus one
/// low-facet corner (see [`push_corner_points`]) at each of the 4 corners.
/// `radius == min(w, h) / 2` on a square closes it into the dot row's
/// 12-gon "circle".
fn rounded_rect_polygon(w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    use std::f32::consts::PI;

    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let mut verts = Vec::with_capacity(4 * (ROUNDED_CORNER_SEGMENTS + 1));
    push_corner_points(&mut verts, r, r, r, PI, PI * 1.5);
    push_corner_points(&mut verts, w - r, r, r, PI * 1.5, PI * 2.0);
    push_corner_points(&mut verts, w - r, h - r, r, 0.0, PI * 0.5);
    push_corner_points(&mut verts, r, h - r, r, PI * 0.5, PI);
    verts
}

/// The polygon's horizontal extent at `y` — the min/max x among every edge
/// that crosses the scanline `y` (half-open per edge so a vertex exactly on
/// the scanline isn't double-counted). Valid for any simple polygon, but
/// only *exact* for a convex one like [`rounded_rect_polygon`], where "every
/// crossing's x" and "the true fill span" coincide (min to max, no need for
/// even-odd pairing of multiple spans).
fn polygon_x_range_at_y(polygon: &[(f32, f32)], y: f32) -> Option<(f32, f32)> {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut found = false;
    for i in 0..polygon.len() {
        let (x0, y0) = polygon[i];
        let (x1, y1) = polygon[(i + 1) % polygon.len()];
        if (y0 <= y) != (y1 <= y) {
            let t = (y - y0) / (y1 - y0);
            let x = x0 + t * (x1 - x0);
            min_x = min_x.min(x);
            max_x = max_x.max(x);
            found = true;
        }
    }
    found.then_some((min_x, max_x))
}

/// Rasterize `polygon` (in `rect`'s local `0..w, 0..h` space) into a stack of
/// 1px-tall `Quad` rows — real, inspectable facet geometry built only from
/// the one primitive (`Quad`) guaranteed to tessellate exactly as drawn
/// (axis-aligned, no corner rounding involved). The polygon's straight edges
/// and corner vertices are still exactly where they are; row height only
/// sets the scanline sampling resolution, not the shape's geometry.
fn push_faceted_polygon_fill(scene: &mut Scene, rect: Rect, polygon: &[(f32, f32)], fill: Color) {
    const ROW_HEIGHT: f32 = 1.0;
    let mut y = 0.0;
    while y < rect.size.height {
        let sample_y = (y + ROW_HEIGHT * 0.5).min(rect.size.height - f32::EPSILON);
        if let Some((x0, x1)) = polygon_x_range_at_y(polygon, sample_y) {
            if x1 > x0 {
                scene.quad(Quad {
                    rect: Rect::new(
                        Point::new(rect.origin.x + x0, rect.origin.y + y),
                        Size::new(x1 - x0, ROW_HEIGHT),
                    ),
                    fill,
                    corner_radii: CornerRadii::all(0.0),
                    stroke: None,
                });
            }
        }
        y += ROW_HEIGHT;
    }
}

/// The showcase's local "triangulated" baseline: a faceted-polygon
/// approximation of the same rounded-rect `vector_rounded_rect` draws
/// smoothly, rasterized entirely from `Quad` rows (see the module docs for
/// why this doesn't touch `mkui-wgpu`'s shared tessellator). A `stroke` is
/// faked by rasterizing the full polygon in the stroke color first, then a
/// `stroke.width`-inset polygon in `fill` on top — the same two-layer idea
/// the real triangulated `Quad` path's 4 edge-strips approximate, just
/// following the rounded corner instead of cutting across it.
fn faceted_rounded_rect(
    scene: &mut Scene,
    rect: Rect,
    radius: f32,
    fill: Color,
    stroke: Option<Stroke>,
) {
    if let Some(stroke) = stroke {
        let outer = rounded_rect_polygon(rect.size.width, rect.size.height, radius);
        push_faceted_polygon_fill(scene, rect, &outer, stroke.color);

        let inset = stroke.width.max(1.0);
        let inner_w = (rect.size.width - 2.0 * inset).max(0.0);
        let inner_h = (rect.size.height - 2.0 * inset).max(0.0);
        let inner_r = (radius - inset).max(0.0);
        let inner_rect = Rect::new(
            Point::new(rect.origin.x + inset, rect.origin.y + inset),
            Size::new(inner_w, inner_h),
        );
        let inner = rounded_rect_polygon(inner_w, inner_h, inner_r);
        push_faceted_polygon_fill(scene, inner_rect, &inner, fill);
    } else {
        let polygon = rounded_rect_polygon(rect.size.width, rect.size.height, radius);
        push_faceted_polygon_fill(scene, rect, &polygon, fill);
    }
}

/// Standard cubic-Bézier circular-arc handle length, relative to the arc's
/// radius: `4/3 * (sqrt(2) - 1)`. One cubic segment per 90° corner
/// approximates a circular arc to within ~0.03% of `r` (vs. ~6% for a
/// single `quadTo`), which matters at the dot row's ~55px radius.
#[cfg(feature = "slug")]
const CIRCLE_ARC_HANDLE: f32 = 0.552_284_8;

/// Build a rounded-rectangle path (`0..w, 0..h`, `moveTo` + `lineTo` +
/// `cubicTo` per corner) in the `mkui-vector2d-wgpu` adapter's **y-up**
/// convention: path-local `y = 0` is the rect's screen-space *bottom* edge
/// and increasing `y` goes *up* toward the top edge (`VectorPath stays in
/// its own units, y-up; the adapter performs the y-flip`, per
/// `mkui_vector2d_wgpu::PlacedVectorPath`'s own doc comment). Pair with an
/// `origin_px` at the rect's bottom-left (`[rect.origin.x,
/// rect.height_end()]`), not its top-left — using the rect's top-left here
/// would place every shape above where it belongs, shifted up by its own
/// height.
#[cfg(feature = "slug")]
fn rounded_rect_commands(w: f32, h: f32, r: f32) -> Vec<mkui_vector2d::PathCommand> {
    use mkui_vector2d::{PathCommand, Vec2};

    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let k = r * CIRCLE_ARC_HANDLE;

    vec![
        PathCommand::MoveTo(Vec2::new(r, h)),
        PathCommand::LineTo(Vec2::new(w - r, h)),
        PathCommand::CubicTo {
            control1: Vec2::new(w - r + k, h),
            control2: Vec2::new(w, h - r + k),
            to: Vec2::new(w, h - r),
        },
        PathCommand::LineTo(Vec2::new(w, r)),
        PathCommand::CubicTo {
            control1: Vec2::new(w, r - k),
            control2: Vec2::new(w - r + k, 0.0),
            to: Vec2::new(w - r, 0.0),
        },
        PathCommand::LineTo(Vec2::new(r, 0.0)),
        PathCommand::CubicTo {
            control1: Vec2::new(r - k, 0.0),
            control2: Vec2::new(0.0, r - k),
            to: Vec2::new(0.0, r),
        },
        PathCommand::LineTo(Vec2::new(0.0, h - r)),
        PathCommand::CubicTo {
            control1: Vec2::new(0.0, h - r + k),
            control2: Vec2::new(r - k, h),
            to: Vec2::new(r, h),
        },
        PathCommand::Close,
    ]
}

/// Vector rounded rect: the same shape as [`faceted_rounded_rect`],
/// built as a `Primitive::VectorPath` (#139) via [`rounded_rect_commands`].
/// `radius == min(w, h) / 2` closes into a stadium/circle, which is how the
/// dot row below reuses this same builder.
#[cfg(feature = "slug")]
fn vector_rounded_rect(
    scene: &mut Scene,
    rect: Rect,
    radius: f32,
    fill: Color,
    stroke: Option<(Color, f32)>,
) {
    use mkui_vector2d::{Bounds, FillRule, Stroke as VectorStroke, Vec2, VectorPath};
    use std::sync::Arc;

    let w = rect.size.width;
    let h = rect.size.height;
    let commands = rounded_rect_commands(w, h, radius);
    let bounds = Bounds::new(Vec2::ZERO, Vec2::new(w, h));
    let path = VectorPath::new(commands, FillRule::NonZero, bounds);

    scene.vector_path(mkui_wgpu::PlacedVectorPath {
        path: Arc::new(path),
        stroke: stroke.map(|(_, width)| VectorStroke::new(width)),
        stroke_color: stroke
            .map(|(color, _)| color)
            .unwrap_or(Color::rgba(0.0, 0.0, 0.0, 0.0)),
        fill_color: Some(fill),
        // Bottom-left, not top-left — see `rounded_rect_commands`' doc.
        origin_px: [rect.origin.x, rect.height_end()],
        scale_px_per_unit: 1.0,
    });
}

/// Draw one showcase row: `size` centered in each column at `y`, the
/// faceted-polygon baseline on the left and (under `slug`) the equivalent
/// `VectorPath` on the right. Without `slug`, the right column falls back to
/// the same faceted-polygon call as the left — the component-level
/// zero-API-break contract, mirrored here for the demo's own shapes.
fn draw_pair(
    scene: &mut Scene,
    y: f32,
    size: Size,
    radius: f32,
    fill: Color,
    stroke: Option<Stroke>,
) {
    let left_rect = Rect::new(
        Point::new(COL_LEFT_X + (COL_WIDTH - size.width) * 0.5, y),
        size,
    );
    faceted_rounded_rect(scene, left_rect, radius, fill, stroke);

    let right_rect = Rect::new(
        Point::new(COL_RIGHT_X + (COL_WIDTH - size.width) * 0.5, y),
        size,
    );
    #[cfg(feature = "slug")]
    {
        let vector_stroke = stroke.map(|s| (s.color, s.width));
        vector_rounded_rect(scene, right_rect, radius, fill, vector_stroke);
    }
    #[cfg(not(feature = "slug"))]
    {
        faceted_rounded_rect(scene, right_rect, radius, fill, stroke);
    }
}

fn row_label(scene: &mut Scene, y_center: f32, text: &str) {
    label(
        scene,
        Rect::new(
            Point::new(ROW_LABEL_X, y_center - 8.0),
            Size::new(50.0, 16.0),
        ),
        text,
        row_label_style(),
    );
}

fn build_scene() -> Scene {
    let mut scene = Scene::new(WINDOW);

    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 14.0), Size::new(740.0, 26.0)),
        "Vector Primitives — Triangulated vs Vector (#143)",
        heading_style(),
    );

    #[cfg(feature = "slug")]
    let subtitle = "feature = \"slug\" is ON: left = faceted polygon (Quad rows), right = Primitive::VectorPath (Slug coverage pipeline).";
    #[cfg(not(feature = "slug"))]
    let subtitle = "feature = \"slug\" is OFF: the vector column is unavailable and falls back to the same faceted-polygon approximation as the left column.";
    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 44.0), Size::new(740.0, 36.0)),
        subtitle,
        caption_style(),
    );

    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 86.0), Size::new(COL_WIDTH, 18.0)),
        "Triangulated — faceted polygon (Quad rows)",
        column_header_style(),
    );
    #[cfg(feature = "slug")]
    let right_header = "Vector — Primitive::VectorPath";
    #[cfg(not(feature = "slug"))]
    let right_header = "Vector — unavailable (build with --features slug)";
    label(
        &mut scene,
        Rect::new(Point::new(COL_RIGHT_X, 86.0), Size::new(COL_WIDTH, 18.0)),
        right_header,
        column_header_style(),
    );

    // Honest caveat: this showcase-local faceted polygon is NOT the real
    // Primitive::Quad path real components use (see the module docs — that
    // shared tessellator still renders a rounded Quad as a plain square,
    // #212). The box height below is sized for the 2 lines this text
    // actually wraps to at COL_WIDTH (verified against the bitmap font's
    // real glyph-advance wrap — a previous, undersized box here truncated a
    // caption mid-sentence and silently dropped its trailing issue
    // reference).
    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 104.0), Size::new(COL_WIDTH, 36.0)),
        "(not mkui-wgpu's shared Quad tessellator — a showcase-local polygon raster; see #212)",
        caption_style(),
    );

    // Row 1: button — small corner radius, the Wave 2 workhorse case (#140).
    row_label(&mut scene, BUTTON_Y + BUTTON_SIZE.height * 0.5, "Button");
    draw_pair(
        &mut scene,
        BUTTON_Y,
        BUTTON_SIZE,
        BUTTON_RADIUS,
        FILL,
        Some(Stroke {
            color: BORDER,
            width: BUTTON_STROKE_WIDTH,
        }),
    );

    // Row 2: card — large corner radius, the longer-curve case (#141).
    row_label(&mut scene, CARD_Y + CARD_SIZE.height * 0.5, "Card");
    draw_pair(
        &mut scene,
        CARD_Y,
        CARD_SIZE,
        CARD_RADIUS,
        CARD_FILL,
        Some(Stroke {
            color: BORDER,
            width: CARD_STROKE_WIDTH,
        }),
    );

    // Row 3: dot — a square cell with radius == side / 2 closes the rounded
    // rect into a circle; 55px radius sits in the issue's 40-60px band so the
    // polygon-vs-circle difference reads at 1x with no zoom.
    row_label(&mut scene, DOT_Y + DOT_SIZE.height * 0.5, "Dot");
    draw_pair(&mut scene, DOT_Y, DOT_SIZE, DOT_RADIUS, DOT_FILL, None);

    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_wgpu::Primitive;

    /// Number of 1px scanline rows `push_faceted_polygon_fill` emits for a
    /// shape of `height`, with no stroke: every row in `0..height` always
    /// has positive width for this file's concrete rounded-rect shapes (the
    /// corner radius never exceeds half the shorter side), so this is exact,
    /// not just a lower bound. `height.round()` handles float step
    /// accumulation; every height in this file is already a whole number.
    fn fill_only_row_count(height: f32) -> usize {
        height.round() as usize
    }

    /// As [`fill_only_row_count`], but for a stroked shape: the outer
    /// (stroke-color) polygon's full height, plus the inner (fill-color)
    /// polygon's height after `faceted_rounded_rect`'s `stroke.width.max(1.0)`
    /// inset on both sides.
    fn stroked_row_count(height: f32, stroke_width: f32) -> usize {
        let inset = stroke_width.max(1.0);
        fill_only_row_count(height) + fill_only_row_count((height - 2.0 * inset).max(0.0))
    }

    /// Exact expected `Quad` count for one faceted-polygon column (left
    /// always; right too when `slug` is off) — button + card are stroked
    /// (2 layered polygons each), dot is fill-only (1 polygon).
    fn one_column_quad_count() -> usize {
        stroked_row_count(BUTTON_SIZE.height, BUTTON_STROKE_WIDTH)
            + stroked_row_count(CARD_SIZE.height, CARD_STROKE_WIDTH)
            + fill_only_row_count(DOT_SIZE.height)
    }

    #[test]
    fn scene_renders_the_expected_text_and_quad_counts() {
        let scene = build_scene();
        let quads = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Quad(_)))
            .count();
        let texts = scene
            .primitives
            .iter()
            .filter(|p| matches!(p, Primitive::Text(_)))
            .count();

        #[cfg(feature = "slug")]
        {
            // Only the left column rasterizes to Quads when the vector lane
            // is live — exactly one column's worth, not 3 (one per shape):
            // proof this is genuine per-row faceting, not a single Quad.
            assert_eq!(
                quads,
                one_column_quad_count(),
                "left column should emit exactly one column's worth of faceted-polygon rows"
            );
            let vector_paths = scene
                .primitives
                .iter()
                .filter(|p| matches!(p, Primitive::VectorPath(_)))
                .count();
            assert_eq!(
                vector_paths, 3,
                "right column should emit exactly 3 VectorPaths"
            );
        }
        #[cfg(not(feature = "slug"))]
        {
            // Both columns fall back to the same faceted-polygon rasterizer.
            assert_eq!(
                quads,
                one_column_quad_count() * 2,
                "both columns should fall back to the same faceted-polygon row count"
            );
        }

        // Title + subtitle + 2 column headers + the caveat + 3 row labels =
        // 8 text cells.
        assert_eq!(texts, 8, "expected 8 label/heading text primitives");
    }

    #[test]
    fn window_viewport_matches_the_constructed_scene() {
        let scene = build_scene();
        assert_eq!(scene.viewport, WINDOW);
    }

    /// Regression test for the y-axis bug a code review caught before merge:
    /// `mkui_vector2d_wgpu::PlacedVectorPath` is y-up (increasing path-y goes
    /// *up* the screen), the opposite of this crate's y-down `Rect`. Getting
    /// it wrong silently shifts every vector shape above where it belongs by
    /// its own height — nothing panics, nothing fails to compile, it just
    /// renders wrong. Compares each `PlacedVectorPath`'s screen-space bbox
    /// (derived from `origin_px` + path `bounds` under the adapter's
    /// documented y-flip) directly against its row's known `(Y, SIZE)` —
    /// the same source-of-truth constants `build_scene` draws from, not
    /// something inferred from the left column's (now many, per-row) `Quad`
    /// primitives.
    #[cfg(feature = "slug")]
    #[test]
    fn vector_path_screen_bbox_matches_its_row_position_and_size() {
        let scene = build_scene();
        let paths: Vec<&mkui_wgpu::PlacedVectorPath> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::VectorPath(v) => Some(v),
                _ => None,
            })
            .collect();
        let rows = [
            (BUTTON_Y, BUTTON_SIZE),
            (CARD_Y, CARD_SIZE),
            (DOT_Y, DOT_SIZE),
        ];
        assert_eq!(paths.len(), rows.len());

        let eps = 0.01;
        for (path, (row_y, size)) in paths.iter().zip(rows.iter()) {
            let [ox, oy] = path.origin_px;
            let bounds = &path.path.bounds;
            // x: unflipped — screen_x = origin_x + path_x.
            let screen_left = ox + bounds.min.x;
            let screen_width = bounds.max.x - bounds.min.x;
            // y: adapter-flipped — screen_y = origin_y - path_y.
            let screen_top = oy - bounds.max.y;
            let screen_height = bounds.max.y - bounds.min.y;

            let expected_left = COL_RIGHT_X + (COL_WIDTH - size.width) * 0.5;
            assert!(
                (screen_left - expected_left).abs() < eps,
                "vector left {screen_left} should be {expected_left} (centred in the right column)"
            );
            assert!(
                (screen_top - row_y).abs() < eps,
                "vector top {screen_top} should be the row's own y={row_y}"
            );
            assert!(
                (screen_width - size.width).abs() < eps,
                "vector width {screen_width} should be the row's own width={}",
                size.width
            );
            assert!(
                (screen_height - size.height).abs() < eps,
                "vector height {screen_height} should be the row's own height={}",
                size.height
            );
        }
    }

    /// Sanity check for the faceted-polygon rasterizer itself (button-shaped
    /// parameters: straight middle edges on every side, unlike the dot's
    /// pure-corner-arc circle) — the union of every emitted row `Quad`
    /// should span exactly the source rect, at a chosen non-zero origin to
    /// also catch an origin-offset bug.
    #[test]
    fn push_faceted_polygon_fill_quads_union_to_the_source_rect() {
        let rect = Rect::new(Point::new(40.0, 25.0), Size::new(200.0, 80.0));
        let radius = 20.0;
        let mut scene = Scene::new(Size::new(400.0, 200.0));
        let polygon = rounded_rect_polygon(rect.size.width, rect.size.height, radius);
        push_faceted_polygon_fill(&mut scene, rect, &polygon, FILL);

        let quads: Vec<&Quad> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect();
        assert_eq!(
            quads.len(),
            rect.size.height as usize,
            "one row per scanline"
        );

        let min_x = quads
            .iter()
            .map(|q| q.rect.origin.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = quads
            .iter()
            .map(|q| q.rect.width_end())
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = quads
            .iter()
            .map(|q| q.rect.origin.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = quads
            .iter()
            .map(|q| q.rect.height_end())
            .fold(f32::NEG_INFINITY, f32::max);

        let eps = 0.01;
        assert!((min_x - rect.origin.x).abs() < eps, "min_x={min_x}");
        assert!((max_x - rect.width_end()).abs() < eps, "max_x={max_x}");
        assert!((min_y - rect.origin.y).abs() < eps, "min_y={min_y}");
        assert!((max_y - rect.height_end()).abs() < eps, "max_y={max_y}");
    }

    /// The faceting proof Codex's round-2 review asked for, mirroring
    /// `mkui-wgpu/src/render/dot_vector_gpu.rs`'s own
    /// `dot_vector_vs_triangulated_diagonal_corner` methodology: a point
    /// inside the dot's bounding square but outside a *true* circle (the
    /// corner a circle doesn't have) must be **excluded** by this showcase's
    /// local faceted-polygon approximation too — proof it's a genuine
    /// low-facet polygon, not a smuggled-back plain square. (Contrast: the
    /// real `Primitive::Quad` + `tessellate_quad` path — not used here —
    /// would *include* that point, since it's just a rectangle; that's
    /// exactly the behavior #212 tracks and this showcase avoids rendering.)
    #[test]
    fn faceted_dot_polygon_excludes_its_bounding_squares_corner() {
        let polygon = rounded_rect_polygon(DOT_SIZE.width, DOT_SIZE.height, DOT_RADIUS);
        let center = DOT_RADIUS;

        // Same 0.875*radius offset dot_vector_gpu.rs uses: comfortably inside
        // the bounding square, well outside the circle boundary.
        let offset = DOT_RADIUS * 0.875;
        let corner_y = center + offset;
        let corner_x = center + offset;
        match polygon_x_range_at_y(&polygon, corner_y) {
            Some((_, max_x)) => assert!(
                corner_x > max_x,
                "the bounding square's corner ({corner_x}, {corner_y}) must fall outside the \
                 faceted polygon's span at that row (max_x={max_x}) — a circle has no corner"
            ),
            None => panic!("row {corner_y} should still be inside the polygon's bounding box"),
        }

        // The shape's own centre row must still be fully covered (rules out
        // a trivial pass from an empty/degenerate polygon).
        let (min_x, max_x) = polygon_x_range_at_y(&polygon, center).expect("centre row is filled");
        assert!(min_x < center && max_x > center);
    }

    /// The dot row's circle is the killer-demo shape (#142): its 4 cubic
    /// corner arcs must actually approximate a circle, not a visible
    /// squircle. Evaluates each arc's midpoint (De Casteljau at `t = 0.5`)
    /// and asserts it sits within the ~0.03% cubic-approximation error of
    /// the nominal radius from the shape's center — a single `quadTo` per
    /// corner would fail this by ~6%.
    #[cfg(feature = "slug")]
    #[test]
    fn dot_corner_arcs_approximate_a_true_circle() {
        use mkui_vector2d::{PathCommand, Vec2};

        let diameter = 110.0_f32;
        let r = diameter * 0.5;
        let commands = rounded_rect_commands(diameter, diameter, r);
        let center = Vec2::new(r, r);

        let mut current = Vec2::ZERO;
        let mut checked = 0;
        for cmd in &commands {
            match *cmd {
                PathCommand::MoveTo(p) => current = p,
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => {
                    let mid = cubic_bezier_point(current, control1, control2, to, 0.5);
                    let dist = ((mid.x - center.x).powi(2) + (mid.y - center.y).powi(2)).sqrt();
                    assert!(
                        (dist - r).abs() < r * 0.001,
                        "arc midpoint distance {dist} should be within 0.1% of r={r}"
                    );
                    checked += 1;
                    current = to;
                }
                _ => {}
            }
        }
        assert_eq!(checked, 4, "expected 4 corner arcs on a full circle");
    }

    #[cfg(feature = "slug")]
    fn cubic_bezier_point(
        p0: mkui_vector2d::Vec2,
        p1: mkui_vector2d::Vec2,
        p2: mkui_vector2d::Vec2,
        p3: mkui_vector2d::Vec2,
        t: f32,
    ) -> mkui_vector2d::Vec2 {
        let mt = 1.0 - t;
        let x = mt * mt * mt * p0.x
            + 3.0 * mt * mt * t * p1.x
            + 3.0 * mt * t * t * p2.x
            + t * t * t * p3.x;
        let y = mt * mt * mt * p0.y
            + 3.0 * mt * mt * t * p1.y
            + 3.0 * mt * t * t * p2.y
            + t * t * t * p3.y;
        mkui_vector2d::Vec2::new(x, y)
    }
}
