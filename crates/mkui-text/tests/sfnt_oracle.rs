//! #67 oracle parity: the from-scratch narrow SFNT decoder is validated against
//! `ttf-parser` — a known-correct independent parser used **only** as a
//! dev-dependency test oracle (never a runtime dependency of `mkui-text`).
//!
//! Parity is checked three ways per the issue's "Test oracle dependency"
//! section:
//! - outline-extraction parity for `M` / `A` / `g`,
//! - cmap lookup parity across the ASCII showcase set,
//! - table-directory/truncation rejection parity.

use std::sync::Arc;

use mkui_text::{OutlineCommand, SfntFace};

const ABEL: &[u8] = include_bytes!("fixtures/abel/Abel-Regular.ttf");

fn ours() -> SfntFace {
    SfntFace::parse(Arc::from(ABEL.to_vec().into_boxed_slice()), 0).expect("Abel decodes")
}

fn oracle() -> ttf_parser::Face<'static> {
    ttf_parser::Face::parse(ABEL, 0).expect("ttf-parser parses Abel")
}

/// Collects an outline into two rotation-invariant multisets: the on-curve
/// anchor points (every MoveTo + segment endpoint) and the quadratic control
/// points. Coordinates are kept as half-unit integers (`*2`) so the implied
/// on-curve midpoints (which land on `.5` boundaries) compare exactly. The
/// multisets are invariant to which on-curve point a parser chooses to start a
/// contour on, so they prove geometric equality without coupling to start
/// choice.
#[derive(Default)]
struct OutlineSets {
    anchors: Vec<(i64, i64)>,
    controls: Vec<(i64, i64)>,
    moves: usize,
    closes: usize,
}

impl OutlineSets {
    fn sorted(mut self) -> Self {
        self.anchors.sort_unstable();
        self.controls.sort_unstable();
        self
    }
}

fn key(x: f32, y: f32) -> (i64, i64) {
    ((x * 2.0).round() as i64, (y * 2.0).round() as i64)
}

fn ours_sets(face: &SfntFace, ch: char) -> OutlineSets {
    let gid = face.glyph_index(ch).unwrap();
    let outline = face.glyph_outline(gid).unwrap();
    let mut s = OutlineSets::default();
    for cmd in &outline.commands {
        match *cmd {
            OutlineCommand::MoveTo { x, y } => {
                s.moves += 1;
                s.anchors.push(key(x, y));
            }
            OutlineCommand::LineTo { x, y } => s.anchors.push(key(x, y)),
            OutlineCommand::QuadTo { cx, cy, x, y } => {
                s.controls.push(key(cx, cy));
                s.anchors.push(key(x, y));
            }
            OutlineCommand::Close => s.closes += 1,
            _ => {}
        }
    }
    s.sorted()
}

/// `ttf_parser::OutlineBuilder` sink that records the same multisets, so the
/// oracle's decomposition is compared on equal terms.
struct OracleSink {
    sets: OutlineSets,
}

impl ttf_parser::OutlineBuilder for OracleSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.sets.moves += 1;
        self.sets.anchors.push(key(x, y));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.sets.anchors.push(key(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.sets.controls.push(key(x1, y1));
        self.sets.anchors.push(key(x, y));
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        panic!("Abel is a quadratic TrueType font; no cubic should appear");
    }
    fn close(&mut self) {
        self.sets.closes += 1;
    }
}

fn oracle_sets(face: &ttf_parser::Face, ch: char) -> OutlineSets {
    let gid = face.glyph_index(ch).unwrap();
    let mut sink = OracleSink {
        sets: OutlineSets::default(),
    };
    face.outline_glyph(gid, &mut sink)
        .expect("oracle outlines glyph");
    sink.sets.sorted()
}

#[test]
fn outline_extraction_parity_for_m_a_g() {
    let ours = ours();
    let oracle = oracle();
    for ch in ['M', 'A', 'g'] {
        let a = ours_sets(&ours, ch);
        let b = oracle_sets(&oracle, ch);
        assert_eq!(a.moves, b.moves, "{ch}: contour count differs");
        assert_eq!(a.closes, b.closes, "{ch}: close count differs");
        assert_eq!(a.anchors, b.anchors, "{ch}: on-curve anchor points differ");
        assert_eq!(a.controls, b.controls, "{ch}: control points differ");
    }
}

#[test]
fn cmap_lookup_parity_for_ascii_set() {
    let ours = ours();
    let oracle = oracle();
    for ch in (0x20u8..=0x7e).map(|b| b as char) {
        let mine = ours.glyph_index(ch).map(|g| g as u32);
        let theirs = oracle.glyph_index(ch).map(|g| g.0 as u32);
        assert_eq!(mine, theirs, "cmap divergence for {ch:?}");
    }
}

#[test]
fn advance_parity_for_ascii_set() {
    let ours = ours();
    let oracle = oracle();
    for ch in (0x20u8..=0x7e).map(|b| b as char) {
        if let (Some(g), Some(og)) = (ours.glyph_index(ch), oracle.glyph_index(ch)) {
            let mine = ours.advance_width(g);
            let theirs = oracle.glyph_hor_advance(og).unwrap_or(0);
            assert_eq!(mine, theirs, "advance divergence for {ch:?}");
        }
    }
}

#[test]
fn malformed_and_truncated_inputs_rejected_like_oracle() {
    // A range of truncation points: both parsers must reject every one. (A
    // healthy full parse is the control that the harness itself is sound.)
    assert!(ttf_parser::Face::parse(ABEL, 0).is_ok());
    assert!(SfntFace::parse(Arc::from(ABEL.to_vec().into_boxed_slice()), 0).is_ok());

    for cut in [3usize, 8, 11, 64, 256, ABEL.len() / 2] {
        let truncated = &ABEL[..cut];
        let oracle_rejects = ttf_parser::Face::parse(truncated, 0).is_err();
        let ours_rejects =
            SfntFace::parse(Arc::from(truncated.to_vec().into_boxed_slice()), 0).is_err();
        assert!(
            oracle_rejects && ours_rejects,
            "truncation at {cut} bytes: oracle_rejects={oracle_rejects} ours_rejects={ours_rejects}"
        );
    }
}
