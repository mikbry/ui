//! Sprint 8 Wave 2 vector-primitives showcase (#143).
//!
//! Renders button / card / dot as a 3x2 grid: the left column is the same
//! triangulated `Primitive::Quad` construction every atom in
//! `mkui-wgpu::components` already uses (`CornerRadii::all(r)` on a rect);
//! the right column emits the identical rounded-rect geometry through
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
//!   with the right column falling back to the same triangulated geometry
//!   as the left — the zero-API-break contract #140-#142 commit to for the
//!   real atoms, mirrored here for this demo's own shapes.
//! - `HEADLESS=1` (either build) exits after a single walk pass — see
//!   `mkui_wgpu::Mkui::run`.
//!
//! Scope note (acceptance criterion "triangulated column shows visible
//! polygons" — not literally met, by design): `mkui-wgpu`'s native
//! tessellator (`tessellate_quad`/`push_rect`) does not consume
//! `Quad::corner_radii` at all — a rounded `Quad` renders as a plain square,
//! not a faceted polygon. This is not a bug this showcase introduces or
//! should fix: it is the exact, documented, GPU-test-asserted baseline
//! #142's own `render/dot_vector_gpu.rs` builds its triangulated-vs-vector
//! comparison against ("the triangulated fallback is visibly polygonal
//! (here: literally square)"), and that PR's own scope decision explicitly
//! defers rounding the fallback as a separate, unrelated concern (#212).
//! Faceting it here would silently break that merged acceptance test and
//! change the rendered look of every `CornerRadii`-using atom in the
//! toolkit (button/card/dot/badge/panel/scrollbar/slider/swatch/text_field)
//! — exactly the kind of out-of-scope, hard-to-review change the issue's
//! own "default to minimum scope" guidance warns against. The on-screen
//! caption below names the gap so a viewer isn't left thinking the left
//! column is broken.

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

/// Triangulated rounded rect: the construction every `mkui-wgpu::components`
/// atom already uses (`Quad` + `CornerRadii::all(r)`).
fn triangulated_rounded_rect(
    scene: &mut Scene,
    rect: Rect,
    radius: f32,
    fill: Color,
    stroke: Option<Stroke>,
) {
    scene.quad(Quad {
        rect,
        fill,
        corner_radii: CornerRadii::all(radius),
        stroke,
    });
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

/// Vector rounded rect: the same shape as [`triangulated_rounded_rect`],
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
/// triangulated `Quad` on the left and (under `slug`) the equivalent
/// `VectorPath` on the right. Without `slug`, the right column falls back to
/// the same triangulated call as the left — the component-level zero-API-
/// break contract, mirrored here for the demo's own shapes.
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
    triangulated_rounded_rect(scene, left_rect, radius, fill, stroke);

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
        triangulated_rounded_rect(scene, right_rect, radius, fill, stroke);
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
    let subtitle = "feature = \"slug\" is ON: left = Primitive::Quad, right = Primitive::VectorPath (Slug coverage pipeline).";
    #[cfg(not(feature = "slug"))]
    let subtitle =
        "feature = \"slug\" is OFF: the vector column is unavailable and falls back to the same triangulated Quad as the left column.";
    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 44.0), Size::new(740.0, 36.0)),
        subtitle,
        caption_style(),
    );

    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 86.0), Size::new(COL_WIDTH, 18.0)),
        "Triangulated — Primitive::Quad",
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

    // Honest caveat (#212, same baseline #142's dot_vector_gpu.rs GPU test
    // asserts): the native tessellator doesn't facet corner_radii at all, so
    // the left column renders as a plain square, not a faceted polygon. The
    // box height below is sized for the 2 lines this text actually wraps to
    // at COL_WIDTH (verified against the bitmap font's real glyph-advance
    // wrap — a previous, undersized box here truncated this caption
    // mid-sentence and silently dropped the trailing issue reference).
    label(
        &mut scene,
        Rect::new(Point::new(COL_LEFT_X, 104.0), Size::new(COL_WIDTH, 36.0)),
        "(renders as a plain square — same #142-tested baseline, see #212)",
        caption_style(),
    );

    // Row 1: button — small corner radius, the Wave 2 workhorse case (#140).
    let button_y = 150.0;
    let button_size = Size::new(260.0, 70.0);
    row_label(&mut scene, button_y + button_size.height * 0.5, "Button");
    draw_pair(
        &mut scene,
        button_y,
        button_size,
        18.0,
        FILL,
        Some(Stroke {
            color: BORDER,
            width: 2.0,
        }),
    );

    // Row 2: card — large corner radius, the longer-curve case (#141).
    let card_y = button_y + button_size.height + 30.0;
    let card_size = Size::new(300.0, 150.0);
    row_label(&mut scene, card_y + card_size.height * 0.5, "Card");
    draw_pair(
        &mut scene,
        card_y,
        card_size,
        36.0,
        Color::rgba(0.16, 0.13, 0.11, 0.92),
        Some(Stroke {
            color: BORDER,
            width: 1.5,
        }),
    );

    // Row 3: dot — a square cell with radius == side / 2 closes the rounded
    // rect into a circle; 55px radius sits in the issue's 40-60px band so the
    // polygon-vs-circle difference reads at 1x with no zoom.
    let dot_y = card_y + card_size.height + 30.0;
    let dot_size = Size::new(110.0, 110.0);
    row_label(&mut scene, dot_y + dot_size.height * 0.5, "Dot");
    draw_pair(
        &mut scene,
        dot_y,
        dot_size,
        dot_size.width * 0.5,
        Color::rgba(0.36, 0.74, 0.45, 0.96),
        None,
    );

    scene
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_wgpu::Primitive;

    #[test]
    fn scene_renders_a_6_cell_grid_plus_labels() {
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
            // Only the left column is triangulated when the vector lane is
            // live: button + card + dot.
            assert_eq!(quads, 3, "left column should emit exactly 3 Quads");
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
            // Both columns fall back to triangulated: 6 Quads, no vector lane.
            assert_eq!(quads, 6, "both columns should fall back to 6 Quads total");
        }

        // Title + subtitle + 2 column headers + the #212 caveat + 3 row
        // labels = 8 text cells.
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
    /// it wrong silently shifts every vector shape above its paired
    /// triangulated one by its own height — nothing panics, nothing fails to
    /// compile, it just renders wrong.
    ///
    /// The right column's `PlacedVectorPath` sits in a different column than
    /// its paired left `Quad` (that's the whole point of a side-by-side
    /// comparison), so this doesn't assert equal rects — it asserts equal
    /// *size*, equal *row* (same top edge), and a horizontal offset of
    /// exactly `COL_RIGHT_X - COL_LEFT_X`, all derived from `origin_px` +
    /// path `bounds` under the adapter's documented y-flip.
    #[cfg(feature = "slug")]
    #[test]
    fn vector_path_screen_bbox_matches_its_paired_quad_row_and_size() {
        let scene = build_scene();
        let quads: Vec<&Quad> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::Quad(q) => Some(q),
                _ => None,
            })
            .collect();
        let paths: Vec<&mkui_wgpu::PlacedVectorPath> = scene
            .primitives
            .iter()
            .filter_map(|p| match p {
                Primitive::VectorPath(v) => Some(v),
                _ => None,
            })
            .collect();
        assert_eq!(quads.len(), paths.len());

        let eps = 0.01;
        for (quad, path) in quads.iter().zip(paths.iter()) {
            let [ox, oy] = path.origin_px;
            let bounds = &path.path.bounds;
            // x: unflipped — screen_x = origin_x + path_x.
            let screen_left = ox + bounds.min.x;
            let screen_width = bounds.max.x - bounds.min.x;
            // y: adapter-flipped — screen_y = origin_y - path_y.
            let screen_top = oy - bounds.max.y;
            let screen_height = bounds.max.y - bounds.min.y;

            assert!(
                (screen_left - quad.rect.origin.x - (COL_RIGHT_X - COL_LEFT_X)).abs() < eps,
                "vector left {screen_left} should sit exactly one column right of quad left {}",
                quad.rect.origin.x
            );
            assert!(
                (screen_top - quad.rect.origin.y).abs() < eps,
                "same row: vector top {screen_top} vs quad top {}",
                quad.rect.origin.y
            );
            assert!(
                (screen_width - quad.rect.size.width).abs() < eps,
                "same width: vector {screen_width} vs quad {}",
                quad.rect.size.width
            );
            assert!(
                (screen_height - quad.rect.size.height).abs() < eps,
                "same height: vector {screen_height} vs quad {}",
                quad.rect.size.height
            );
        }
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
