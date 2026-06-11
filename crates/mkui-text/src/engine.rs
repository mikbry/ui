//! Cached text preparation + width-dependent layout.
//!
//! [`TextEngine`] separates **preparation** (parse/normalize a string under a
//! [`LayoutSpec`], compute size-independent intrinsic metrics) from
//! **width-dependent layout** (wrapping + positioning at a given max width). A
//! [`PreparedText`] holds the preparation result and caches each
//! [`TextLayout`] it produces, so repeatedly laying the same text out at
//! different widths never re-prepares the text.
//!
//! No renderer resource is involved — the engine wraps an
//! `Arc<dyn TextSystem>` and returns plain CPU-side metrics and positioned
//! [`LayoutRun`]s.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::system::{LayoutRun, LayoutSpec, TextSystem};

/// Engine that prepares text once and lays it out at many widths.
///
/// Cheap to clone — it is just an `Arc<dyn TextSystem>` handle.
#[derive(Clone)]
pub struct TextEngine {
    system: Arc<dyn TextSystem>,
}

impl TextEngine {
    /// Wrap a text system in a caching engine.
    pub fn new(system: Arc<dyn TextSystem>) -> Self {
        Self { system }
    }

    /// The underlying text system.
    pub fn system(&self) -> &Arc<dyn TextSystem> {
        &self.system
    }

    /// Prepare `text` under `spec`. This performs the size-independent work
    /// (intrinsic, unwrapped layout + metrics) once; subsequent
    /// [`PreparedText::layout`] calls reuse it.
    pub fn prepare(&self, text: impl Into<String>, spec: LayoutSpec) -> PreparedText {
        let text = text.into();
        // Intrinsic layout: unwrapped (no max width), so wrapping never
        // splits the content. This is the "prepared" form every width-bounded
        // layout is derived from.
        let intrinsic_runs = self.system.layout(&text, &spec, None);
        let intrinsic_width_px = max_run_width(&intrinsic_runs);
        let intrinsic = Arc::new(build_layout(intrinsic_runs, intrinsic_width_px, &spec));
        PreparedText {
            system: Arc::clone(&self.system),
            text,
            spec,
            intrinsic,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

/// Size-independent preparation of a string, plus a per-width layout cache.
pub struct PreparedText {
    system: Arc<dyn TextSystem>,
    text: String,
    spec: LayoutSpec,
    /// The unwrapped (intrinsic) layout — also the `None`-width result.
    intrinsic: Arc<TextLayout>,
    /// Memoized width-bounded layouts, keyed by the `f32` width bit pattern.
    cache: Mutex<HashMap<u32, Arc<TextLayout>>>,
}

impl PreparedText {
    /// The original source text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The spec this text was prepared under.
    pub fn spec(&self) -> &LayoutSpec {
        &self.spec
    }

    /// Intrinsic (unwrapped) logical width in pixels — the width the content
    /// occupies when never wrapped.
    pub fn intrinsic_width_px(&self) -> f32 {
        self.intrinsic.logical_width_px
    }

    /// The intrinsic, unwrapped layout (equivalent to `layout(None)`).
    pub fn intrinsic_layout(&self) -> Arc<TextLayout> {
        Arc::clone(&self.intrinsic)
    }

    /// Lay the prepared text out at `max_width_px`, wrapping as needed. Results
    /// are cached per width, so calling this repeatedly — including at many
    /// different widths — never re-prepares the text.
    pub fn layout(&self, max_width_px: Option<f32>) -> Arc<TextLayout> {
        let Some(width) = max_width_px else {
            return Arc::clone(&self.intrinsic);
        };
        // NaN never compares equal to itself; fold it onto the unbounded case
        // rather than poisoning the cache with an unreachable key.
        if !width.is_finite() {
            return Arc::clone(&self.intrinsic);
        }
        let key = width.to_bits();
        if let Some(found) = self.cache.lock().unwrap().get(&key) {
            return Arc::clone(found);
        }
        let runs = self.system.layout(&self.text, &self.spec, Some(width));
        let logical_width = max_run_width(&runs);
        let layout = Arc::new(build_layout(runs, logical_width, &self.spec));
        let mut cache = self.cache.lock().unwrap();
        // Another thread may have inserted between our miss and now; keep the
        // first winner so identical widths share one allocation.
        Arc::clone(cache.entry(key).or_insert(layout))
    }
}

/// A laid-out text block: positioned runs plus block- and line-level metrics.
#[derive(Debug, Clone)]
pub struct TextLayout {
    /// One positioned run per produced line (bitmap path) or shaped segment.
    pub runs: Vec<LayoutRun>,
    /// Per-line metrics, indexed parallel to the wrapped lines.
    pub lines: Vec<LineMetrics>,
    /// Width of the widest line at this layout's max width.
    pub logical_width_px: f32,
    /// Total block height (line height × line count).
    pub block_height_px: f32,
    /// The line advance used for this block.
    pub line_height_px: f32,
}

/// Line-level metrics extracted from a [`LayoutRun`].
#[derive(Debug, Clone)]
pub struct LineMetrics {
    /// Top of the line box, relative to the block origin.
    pub top_px: f32,
    /// Baseline y, relative to the block origin.
    pub baseline_px: f32,
    /// Ascent above the baseline.
    pub ascent_px: f32,
    /// Descent below the baseline.
    pub descent_px: f32,
    /// Visible width of the line.
    pub width_px: f32,
    /// Range of glyph indices this line occupies in the flattened glyph
    /// stream (`runs.iter().flat_map(|r| &r.glyphs)`), for hit-testing.
    pub glyph_range: Range<usize>,
}

/// Visible width of a single run: the pen advance to the last glyph's trailing
/// edge. Renderer-neutral — derived only from glyph positions/advances.
fn run_width(run: &LayoutRun) -> f32 {
    run.glyphs
        .iter()
        .map(|g| g.x_px + g.x_advance_px)
        .fold(0.0_f32, f32::max)
}

fn max_run_width(runs: &[LayoutRun]) -> f32 {
    runs.iter().map(run_width).fold(0.0_f32, f32::max)
}

fn build_layout(runs: Vec<LayoutRun>, logical_width_px: f32, spec: &LayoutSpec) -> TextLayout {
    let line_height_px = spec.line_height_px.max(1.0);
    let mut lines = Vec::with_capacity(runs.len());
    let mut glyph_cursor = 0usize;
    for run in &runs {
        let glyph_end = glyph_cursor + run.glyphs.len();
        lines.push(LineMetrics {
            top_px: run.origin_y_px,
            baseline_px: run.line_y_baseline_px,
            ascent_px: run.line_ascent_px,
            descent_px: run.line_descent_px,
            width_px: run_width(run),
            glyph_range: glyph_cursor..glyph_end,
        });
        glyph_cursor = glyph_end;
    }
    let block_height_px = lines.len() as f32 * line_height_px;
    TextLayout {
        runs,
        lines,
        logical_width_px,
        block_height_px,
        line_height_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::BitmapTextSystem;

    fn engine() -> TextEngine {
        TextEngine::new(Arc::new(BitmapTextSystem::new()))
    }

    #[test]
    fn prepare_then_layout_at_multiple_widths() {
        let eng = engine();
        let prepared = eng.prepare("hello world from mkui", LayoutSpec::default());
        let wide = prepared.layout(Some(1_000.0));
        let narrow = prepared.layout(Some(40.0));
        // Wrapping at a narrow width produces more lines than the wide layout.
        assert!(narrow.lines.len() > wide.lines.len());
        // The intrinsic width is the unwrapped width and is at least as wide
        // as any wrapped layout's logical width.
        assert!(prepared.intrinsic_width_px() >= narrow.logical_width_px);
    }

    #[test]
    fn layout_is_cached_per_width() {
        let eng = engine();
        let prepared = eng.prepare("cache me", LayoutSpec::default());
        let a = prepared.layout(Some(120.0));
        let b = prepared.layout(Some(120.0));
        // Same width returns the very same cached allocation.
        assert!(Arc::ptr_eq(&a, &b));
        let c = prepared.layout(Some(60.0));
        assert!(!Arc::ptr_eq(&a, &c));
    }

    #[test]
    fn none_and_non_finite_width_reuse_intrinsic() {
        let eng = engine();
        let prepared = eng.prepare("abc", LayoutSpec::default());
        let intrinsic = prepared.intrinsic_layout();
        assert!(Arc::ptr_eq(&prepared.layout(None), &intrinsic));
        assert!(Arc::ptr_eq(&prepared.layout(Some(f32::NAN)), &intrinsic));
        assert!(Arc::ptr_eq(
            &prepared.layout(Some(f32::INFINITY)),
            &intrinsic
        ));
    }

    #[test]
    fn empty_string_yields_one_zero_width_line() {
        let eng = engine();
        let prepared = eng.prepare("", LayoutSpec::default());
        let layout = prepared.layout(Some(100.0));
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0].width_px, 0.0);
        assert_eq!(layout.logical_width_px, 0.0);
        assert!(layout.block_height_px > 0.0);
    }

    #[test]
    fn whitespace_only_layout_has_metrics() {
        let eng = engine();
        let prepared = eng.prepare("   ", LayoutSpec::default());
        let layout = prepared.layout(Some(100.0));
        assert!(!layout.lines.is_empty());
        assert!(layout.block_height_px > 0.0);
    }

    #[test]
    fn long_unbroken_word_does_not_panic_and_wraps() {
        let eng = engine();
        let prepared = eng.prepare("supercalifragilisticexpialidocious", LayoutSpec::default());
        let layout = prepared.layout(Some(40.0));
        assert!(!layout.runs.is_empty());
        // Every glyph index is covered exactly once by the line ranges.
        let total_glyphs: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
        let covered: usize = layout.lines.iter().map(|l| l.glyph_range.len()).sum();
        assert_eq!(total_glyphs, covered);
    }

    #[test]
    fn baseline_and_height_metrics_are_populated() {
        let eng = engine();
        let prepared = eng.prepare("Ag", LayoutSpec::default());
        let layout = prepared.layout(None);
        let line = &layout.lines[0];
        assert!(line.baseline_px > line.top_px);
        assert!(line.ascent_px > 0.0);
        assert_eq!(line.glyph_range, 0..2);
    }

    #[test]
    fn ascii_glyph_count_round_trips() {
        let eng = engine();
        let prepared = eng.prepare("Hello", LayoutSpec::default());
        let layout = prepared.layout(None);
        let total: usize = layout.runs.iter().map(|r| r.glyphs.len()).sum();
        assert_eq!(total, 5);
    }
}
