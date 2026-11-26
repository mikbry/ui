//! The callback contract between this crate and a text/leaf measurer.
//!
//! `mkui-layout` never shapes a glyph itself — per the issue #63 decision to
//! keep renderer/device/text-stack concerns out of the layout engine, every
//! leaf's intrinsic content size (a `Text` run, a `Button` label, a
//! `Custom` extension node) comes from a caller-supplied [`LeafMeasure`]
//! implementation instead. A backend wires this to `mkui-text` (see the
//! `canonical_showcase_layout_with_real_bitmap_text_system` test for a
//! worked example using `mkui_text::BitmapTextSystem`); this crate's own
//! tests use a deterministic fake so layout assertions don't depend on glyph
//! metrics.
//!
//! Button sizing (label width/height) deliberately goes through this same
//! callback rather than a hardcoded constant — button height/padding are
//! theme data, not a layout-engine concern.

use mkui_runtime::{ButtonVariant, ResolvedStyle, TextVariant};

use crate::geometry::Size;

/// Intrinsic content size reported by a [`LeafMeasure`] call.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Measured {
    pub size: Size,
}

impl Measured {
    pub fn new(width_px: f32, height_px: f32) -> Self {
        Self {
            size: Size::new(width_px, height_px),
        }
    }
}

/// Caller-supplied leaf sizing. One call site per [`mkui_runtime::NodeKind`]
/// leaf variant (`Root` and `View` are never leaves — they're laid out by
/// the flex engine itself).
///
/// `max_width_px: None` asks for the leaf's **max-content** size — e.g. text
/// as if it never wraps — used when this crate needs a leaf's intrinsic
/// width to size a `flex`-row parent before any wrapping-at-final-width
/// pass. `Some(w)` asks for the size once wrapped/constrained to `w`.
pub trait LeafMeasure {
    /// Measure a `Text` node's content.
    fn measure_text(
        &self,
        content: &str,
        variant: TextVariant,
        style: &ResolvedStyle,
        max_width_px: Option<f32>,
    ) -> Measured;

    /// Measure a `Button` node's label.
    fn measure_button(
        &self,
        label: &str,
        variant: ButtonVariant,
        style: &ResolvedStyle,
        max_width_px: Option<f32>,
    ) -> Measured;

    /// Measure a `TextField` node (#312). Unlike [`Self::measure_button`],
    /// a text field's width is normally container-driven (it stretches to
    /// fill its row, like a real `<input>`), not content-driven — callers
    /// typically only care about the returned height when `max_width_px`
    /// is `Some`. `value`/`placeholder` are both handed over (rather than
    /// just whichever currently renders) so a measurer that *does* want a
    /// content-driven max-content width (`max_width_px: None`, sizing a Row
    /// container's flex-basis) can size against whichever is wider.
    fn measure_text_field(
        &self,
        value: &str,
        placeholder: &str,
        style: &ResolvedStyle,
        max_width_px: Option<f32>,
    ) -> Measured;

    /// Measure a `Custom` extension node. `mkui-layout` has no registry for
    /// these — it knows only `type_name` + the opaque `props` JSON payload
    /// the tree already carries. The default contributes zero size, which
    /// is correct for a node this measurer doesn't recognise: it still
    /// participates in flow (unless [`LeafMeasure::is_overlay`] says
    /// otherwise) but reserves no space of its own.
    fn measure_custom(
        &self,
        type_name: &str,
        props: &serde_json::Value,
        style: &ResolvedStyle,
        max_width_px: Option<f32>,
    ) -> Measured {
        let _ = (type_name, props, style, max_width_px);
        Measured::default()
    }

    /// Whether a `Custom` node floats over the flow entirely — the
    /// `mkui-wgpu` walker's `tooltip` / overlay-`badge` concept
    /// (`walker.rs::is_flow_excluded`), generalised: this crate has no
    /// built-in notion of either, so a measurer that cares opts a
    /// `type_name` out of flow here. Default: nothing is an overlay. An
    /// overlay node contributes no size, consumes no gap, and gets no rect
    /// in the [`crate::LayoutResult`] — a future renderer places it itself.
    fn is_overlay(&self, type_name: &str, props: &serde_json::Value) -> bool {
        let _ = (type_name, props);
        false
    }
}
