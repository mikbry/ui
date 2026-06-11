//! Cached text preparation + width-dependent layout.
//!
//! [`TextEngine`] separates **preparation** (shape a string under a
//! [`LayoutSpec`] into width-invariant glyphs + metrics) from
//! **width-dependent layout** (line breaking + positioning at a given max
//! width). A [`PreparedText`] holds the [`ShapedText`] produced **once** by
//! [`TextSystem::prepare`] and caches each [`TextLayout`] it produces, so
//! laying the same text out at many widths re-shapes nothing — each new width
//! only runs [`TextSystem::wrap`] over the already-shaped glyphs.
//!
//! No renderer resource is involved — the engine wraps an
//! `Arc<dyn TextSystem>` and returns plain CPU-side metrics and positioned
//! [`LayoutRun`]s.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::{Arc, Mutex};

use crate::system::{LayoutRun, LayoutSpec, ShapedText, TextSystem};

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

    /// Prepare `text` under `spec`. This runs the width-invariant shaping
    /// ([`TextSystem::prepare`]) **once**; subsequent [`PreparedText::layout`]
    /// calls reuse the shaped glyphs and only line-break at each width.
    pub fn prepare(&self, text: impl AsRef<str>, spec: LayoutSpec) -> PreparedText {
        let shaped = self.system.prepare(text.as_ref(), &spec);
        // The intrinsic (unwrapped) layout is the `None`-width wrap of the
        // already-shaped glyphs — still no re-shaping.
        let intrinsic_runs = self.system.wrap(&shaped, None);
        let intrinsic_width_px = max_run_width(&intrinsic_runs);
        let intrinsic = Arc::new(build_layout(
            intrinsic_runs,
            intrinsic_width_px,
            &shaped.spec,
        ));
        PreparedText {
            system: Arc::clone(&self.system),
            shaped,
            intrinsic,
            cache: Mutex::new(HashMap::new()),
        }
    }
}

/// Width-invariant preparation of a string, plus a per-width layout cache.
///
/// The text is shaped exactly once (at construction). Every [`layout`] call
/// reuses that shaping and only performs width-dependent line breaking, with
/// results memoized per width.
///
/// [`layout`]: PreparedText::layout
pub struct PreparedText {
    system: Arc<dyn TextSystem>,
    /// The width-invariant shaping — produced once, reused at every width.
    shaped: ShapedText,
    /// The unwrapped (intrinsic) layout — also the `None`-width result.
    intrinsic: Arc<TextLayout>,
    /// Memoized width-bounded layouts, keyed by the `f32` width bit pattern.
    cache: Mutex<HashMap<u32, Arc<TextLayout>>>,
}

impl PreparedText {
    /// The original source text.
    pub fn text(&self) -> &str {
        &self.shaped.text
    }

    /// The spec this text was prepared under.
    pub fn spec(&self) -> &LayoutSpec {
        &self.shaped.spec
    }

    /// The width-invariant shaped glyphs + metrics this text was prepared into.
    pub fn shaped(&self) -> &ShapedText {
        &self.shaped
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
    /// are cached per width, and the shaping done at construction is reused —
    /// so calling this repeatedly, at any number of widths, never re-shapes the
    /// text.
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
        // Width-dependent line breaking only — `wrap` consumes the shaped
        // glyphs; it does not re-shape.
        let runs = self.system.wrap(&self.shaped, Some(width));
        let logical_width = max_run_width(&runs);
        let layout = Arc::new(build_layout(runs, logical_width, &self.shaped.spec));
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

    #[test]
    fn shaping_runs_once_across_many_widths() {
        use crate::system::{ShapedText, TextSystem};
        use std::sync::atomic::{AtomicUsize, Ordering};

        // A counting mock that delegates shaping/breaking to the bitmap system
        // but records how often each half is invoked.
        struct Counting {
            inner: BitmapTextSystem,
            prepares: AtomicUsize,
            wraps: AtomicUsize,
        }
        impl TextSystem for Counting {
            fn resolve_font(&self, q: &crate::system::FontQuery) -> Option<crate::FontId> {
                self.inner.resolve_font(q)
            }
            fn register_font_bytes(
                &self,
                b: Arc<[u8]>,
                i: u32,
            ) -> Result<crate::FontId, crate::TextError> {
                self.inner.register_font_bytes(b, i)
            }
            fn layout(&self, text: &str, spec: &LayoutSpec, w: Option<f32>) -> Vec<LayoutRun> {
                let shaped = self.prepare(text, spec);
                self.wrap(&shaped, w)
            }
            fn prepare(&self, text: &str, spec: &LayoutSpec) -> ShapedText {
                self.prepares.fetch_add(1, Ordering::Relaxed);
                self.inner.prepare(text, spec)
            }
            fn wrap(&self, shaped: &ShapedText, w: Option<f32>) -> Vec<LayoutRun> {
                self.wraps.fetch_add(1, Ordering::Relaxed);
                self.inner.wrap(shaped, w)
            }
            fn rasterize(
                &self,
                k: crate::GlyphCacheKey,
            ) -> Result<crate::GlyphImage, crate::TextError> {
                self.inner.rasterize(k)
            }
            fn families(&self) -> Vec<String> {
                self.inner.families()
            }
        }

        let system = Arc::new(Counting {
            inner: BitmapTextSystem::new(),
            prepares: AtomicUsize::new(0),
            wraps: AtomicUsize::new(0),
        });
        let eng = TextEngine::new(system.clone());
        let prepared = eng.prepare("the quick brown fox", LayoutSpec::default());
        // Two distinct widths.
        let _ = prepared.layout(Some(200.0));
        let _ = prepared.layout(Some(40.0));

        // Preparation (shaping) ran exactly once; only line breaking repeats.
        assert_eq!(
            system.prepares.load(Ordering::Relaxed),
            1,
            "prepare must run once across widths"
        );
        // One wrap at construction (intrinsic) + one per distinct width.
        assert_eq!(
            system.wraps.load(Ordering::Relaxed),
            3,
            "wrap runs per width without re-preparing"
        );
    }
}
