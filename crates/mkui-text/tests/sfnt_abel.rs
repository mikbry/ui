//! #67 acceptance: the narrow SFNT decoder reads the licensed Abel fixture and
//! reproduces the checked-in glyph-`M` calibration, including the Tier-2
//! baseline-diff threshold feasibility at 12 px.
//!
//! The calibration constants below are copied from
//! `tests/fixtures/abel/CALIBRATION.json` (derived by the reproducible
//! `calibrate.mjs` tool). The test re-derives every value from the *decoded*
//! outline so a drift between the parser and the committed calibration fails
//! loudly.

use std::sync::Arc;

use mkui_text::SfntFace;

const ABEL: &[u8] = include_bytes!("fixtures/abel/Abel-Regular.ttf");

fn face() -> SfntFace {
    SfntFace::parse(Arc::from(ABEL.to_vec().into_boxed_slice()), 0).expect("Abel decodes")
}

#[test]
fn decodes_face_header_family_and_metrics() {
    let face = face();
    assert_eq!(face.units_per_em(), 2048);
    assert_eq!(face.num_glyphs(), 259);
    assert_eq!(face.family_name(), Some("Abel"));
    let metrics = face.metrics();
    assert_eq!(metrics.units_per_em, 2048);
}

#[test]
fn maps_ascii_showcase_set_to_glyphs() {
    let face = face();
    // Every printable ASCII letter/digit resolves to a real (non-notdef) glyph.
    for ch in ('A'..='Z').chain('a'..='z').chain('0'..='9') {
        assert!(
            face.glyph_index(ch).is_some(),
            "Abel should map ASCII {ch:?}"
        );
    }
    // The calibrated glyph id + advance for 'M'.
    assert_eq!(face.glyph_index('M'), Some(48));
    assert_eq!(face.advance_width(48), 1339);
    // A character outside the font is unmapped (the fallback boundary).
    assert_eq!(face.glyph_index('中'), None);
}

#[test]
fn glyph_m_outline_matches_calibrated_font_unit_bounds() {
    let face = face();
    let gid = face.glyph_index('M').unwrap();
    let outline = face.glyph_outline(gid).unwrap();

    // One contour => exactly one MoveTo.
    let moves = outline
        .commands
        .iter()
        .filter(|c| matches!(c, mkui_text::OutlineCommand::MoveTo { .. }))
        .count();
    assert_eq!(moves, 1, "glyph M has a single contour");

    // Decoded ink bounds equal the committed font-unit bbox.
    let b = outline.ink_bounds;
    assert_eq!(
        (b.min_x, b.min_y, b.max_x, b.max_y),
        (164.0, 0.0, 1176.0, 1434.0)
    );
}

/// The Tier-2 baseline-diff threshold formula, re-derived from the decoded
/// glyph-`M` bounds, must reproduce the committed calibration AND stay feasible
/// (the outward-rounded pixel rectangle has at least `threshold` pixels) at
/// every showcase size — the precondition #67 requires before the GPU test is
/// accepted.
#[test]
fn glyph_m_threshold_is_calibrated_and_feasible_at_all_sizes() {
    let face = face();
    let upem = face.units_per_em() as f64;
    let gid = face.glyph_index('M').unwrap();
    let b = face.glyph_outline(gid).unwrap().ink_bounds;
    let (x_min, y_min, x_max, y_max) = (
        b.min_x as f64,
        b.min_y as f64,
        b.max_x as f64,
        b.max_y as f64,
    );
    let ink_area = (x_max - x_min) * (y_max - y_min);

    // (px, expected threshold, expected outward-rounded bbox area) from
    // CALIBRATION.json.
    let calibration = [
        (12.0, 10i64, 63i64),
        (16.0, 10, 108),
        (24.0, 20, 221),
        (48.0, 80, 850),
    ];

    for (px, want_threshold, want_area) in calibration {
        let scale = px / upem;
        let scaled_ink_area = ink_area * scale * scale;
        let threshold = (scaled_ink_area.min((10.0f64).max(scaled_ink_area * 0.10))).ceil() as i64;

        // Outward-rounded rectangle: floor minima, ceil maxima.
        let rx_min = (x_min * scale).floor();
        let ry_min = (y_min * scale).floor();
        let rx_max = (x_max * scale).ceil();
        let ry_max = (y_max * scale).ceil();
        let rounded_area = ((rx_max - rx_min) * (ry_max - ry_min)) as i64;

        assert_eq!(threshold, want_threshold, "threshold at {px}px");
        assert_eq!(rounded_area, want_area, "rounded bbox area at {px}px");
        assert!(
            rounded_area >= threshold,
            "outward-rounded rect at {px}px must hold >= threshold pixels"
        );
    }
}
