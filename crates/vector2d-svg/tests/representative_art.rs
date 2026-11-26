//! Integration test (#201): a single representative SVG exercising every
//! element and attribute in the sanitized subset, proving it lowers to
//! drawable `mkui-vector2d` geometry end-to-end — without a GPU, matching
//! `examples/vector-showcase`'s own `vector_path_contains` style of
//! correctness check (point containment via `encode_vector_path`'s curve
//! records), since no renderer in this workspace can shade a gradient yet.

use mkui_vector2d::{encode_vector_path, stroke_to_fill, SlugConfig, SlugCurve, Vec2};
use mkui_vector2d_svg::{parse_svg, Paint};

const SUBSET_SAMPLE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="200" viewBox="0 0 200 200">
    <defs>
        <linearGradient id="lg" x1="0" y1="0" x2="1" y2="0">
            <stop offset="0" stop-color="#000000"/>
            <stop offset="1" stop-color="#ffffff"/>
        </linearGradient>
        <radialGradient id="rg" cx="0.5" cy="0.5" r="0.5">
            <stop offset="0%" stop-color="rgb(255,0,0)"/>
            <stop offset="100%" stop-color="rgba(0,0,255,0.5)"/>
        </radialGradient>
    </defs>
    <g fill="#336699" transform="translate(10, 10)">
        <rect x="0" y="0" width="30" height="20"/>
        <circle cx="50" cy="10" r="10"/>
        <ellipse cx="80" cy="10" rx="15" ry="8"/>
        <line x1="0" y1="30" x2="30" y2="30" stroke="#000000"/>
        <polyline points="0,40 10,50 20,40" stroke="#000000" fill="none"/>
        <polygon points="40,40 60,40 50,60" fill="url(#lg)"/>
        <path d="M70,40 L90,40 L80,60 Z" fill="url(#rg)"/>
        <path d="M100,40 A10 10 0 0 1 120,40 Z" fill="#ff00ff"/>
        <g opacity="0.5" transform="rotate(10, 100, 100)">
            <rect x="100" y="100" width="10" height="10" fill="#00ff00"/>
        </g>
    </g>
</svg>"##;

#[test]
fn representative_sample_parses_and_every_shape_lowers_to_drawable_geometry() {
    let doc = parse_svg(SUBSET_SAMPLE).expect("the sanitized subset sample must parse");

    assert_eq!(doc.width, Some(200.0));
    assert_eq!(doc.height, Some(200.0));
    assert!(doc.view_box.is_some());

    // rect, circle, ellipse, line, polyline, polygon, path, path-with-arc,
    // nested-group rect = 9 shapes. The <defs> gradients contribute none.
    assert_eq!(doc.shapes.len(), 9);

    let config = SlugConfig::new(4, 4, 1);
    let mut gradient_fills = 0;
    let mut solid_fills = 0;
    let mut strokes = 0;

    for shape in &doc.shapes {
        // Every fill lowers to encodable curve geometry — proves the path
        // commands, the composed CTM, and the y-flip never produce NaN/Inf
        // or a singular transform anywhere in the sample.
        if shape.fill.is_some() {
            encode_vector_path(&shape.path, &config)
                .unwrap_or_else(|e| panic!("fill failed to encode: {e}"));
        }
        if let Some((stroke, _)) = &shape.stroke {
            let outline = stroke_to_fill(&shape.path, stroke);
            encode_vector_path(&outline, &config)
                .unwrap_or_else(|e| panic!("stroke outline failed to encode: {e}"));
            strokes += 1;
        }
        match &shape.fill {
            Some(Paint::Gradient(_)) => gradient_fills += 1,
            Some(Paint::Solid(_)) => solid_fills += 1,
            None => {}
        }
    }

    assert_eq!(gradient_fills, 2, "polygon + arc path use url(#...) fills");
    assert!(solid_fills >= 5);
    assert!(strokes >= 2, "line + polyline are stroked");
}

#[test]
fn group_transform_places_the_rect_fill_where_expected() {
    // The outer group translates by (10, 10); the rect's first command is a
    // `MoveTo` at its local top-left corner (0, 0) (pre-translate), which
    // must therefore land at on-screen SVG (10, 10) once the group
    // transform is applied.
    use mkui_vector2d::PathCommand;

    let doc = parse_svg(SUBSET_SAMPLE).unwrap();
    let rect = &doc.shapes[0];
    let top_left_local = match rect.path.commands[0] {
        PathCommand::MoveTo(p) => p,
        other => panic!("expected the rect's first command to be a MoveTo, got {other:?}"),
    };
    let placed_yup = rect.path.transform.transform_point(top_left_local);
    // Flip back to SVG's own y-down convention to compare against the
    // authored (x, y) numbers directly.
    let placed_svg = Vec2::new(placed_yup.x, -placed_yup.y);
    assert!((placed_svg.x - 10.0).abs() < 1e-4);
    assert!((placed_svg.y - 10.0).abs() < 1e-4);
}

#[test]
fn a_filled_triangle_contains_its_centroid() {
    // Black-box point-containment check mirroring
    // `examples/vector-showcase`'s `vector_path_contains` helper: build a
    // standalone triangle, encode it, and confirm a ray cast from the
    // centroid crosses an odd number of curve y-extents below it (the same
    // non-zero-winding intuition `encode_vector_path` relies on elsewhere).
    let xml = r##"<svg xmlns="http://www.w3.org/2000/svg">
        <path d="M0,0 L30,0 L0,30 Z" fill="#000000"/>
    </svg>"##;
    let doc = parse_svg(xml).unwrap();
    let shape = &doc.shapes[0];
    let config = SlugConfig::new(4, 4, 1);
    let glyph = encode_vector_path(&shape.path, &config).unwrap();

    // Centroid of the SVG-space triangle (0,0),(30,0),(0,30) is (10,10);
    // after the crate's y-flip that's local point (10, -10), then through
    // the (identity) transform.
    let centroid = shape.path.transform.transform_point(Vec2::new(10.0, -10.0));
    assert!(point_in_fill(&glyph.curves, centroid));
    // A point well outside the triangle must not count as inside.
    let outside = shape
        .path
        .transform
        .transform_point(Vec2::new(-100.0, -100.0));
    assert!(!point_in_fill(&glyph.curves, outside));
}

/// Minimal non-zero-winding point-in-fill test via a horizontal ray cast
/// against each curve's flattened chord — good enough for this test's
/// straight-edged fixtures (mirrors the intent, not the exact
/// implementation, of `vector-showcase`'s own `vector_path_contains`).
fn point_in_fill(curves: &[SlugCurve], p: Vec2) -> bool {
    let mut winding = 0i32;
    for c in curves {
        let (a, b) = (c.p0, c.p2);
        if (a.y <= p.y && b.y > p.y) || (b.y <= p.y && a.y > p.y) {
            let t = (p.y - a.y) / (b.y - a.y);
            let x_at_y = a.x + t * (b.x - a.x);
            if x_at_y > p.x {
                winding += if b.y > a.y { 1 } else { -1 };
            }
        }
    }
    winding != 0
}
