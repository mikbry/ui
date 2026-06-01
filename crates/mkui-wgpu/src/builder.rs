//! Declarative scene builder.
//!
//! `UiBuilder<T>` sits one layer above [`components`](crate::components) and
//! gives scene-based panels a stack-based layout over a [`Scene`] +
//! [`PanelLayout<T>`]. The builder owns the cursor, the content rect, and the
//! theme, so hosts stop hand-rolling row math and per-row drawing helpers.
//!
//! The shape mirrors [`miklabs/ui`] and shadcn: a retained
//! `Panel > Stack > Row > Text / Input / Slider / ChipGroup` tree in
//! `mkui-core` maps 1:1 to immediate-mode method calls on `UiBuilder`.
//!
//! ```
//! use mkui_wgpu::prelude::*;
//! # let theme = WgpuTheme::default();
//! # let mut scene = Scene::new(Size::new(800.0, 600.0));
//! # let mut layout: PanelLayout<()> = PanelLayout::default();
//! # let content = Rect::new(Point::new(0.0, 0.0), Size::new(260.0, 200.0));
//! let mut ui = UiBuilder::<()>::new(&mut scene, &mut layout, &theme, content);
//! ui.heading("Inspector");
//! ui.subheading("Terrain patch");
//! ui.readonly_row("area", "1.4 m²");
//! ```
//!
//! The caller keeps product-specific domain knowledge out of this module — the builder only knows
//! about layout, theme, components, and typed targets.
//!
//! [`miklabs/ui`]: https://github.com/mikbry/ui

use crate::components;
use crate::theme::{
    ButtonSize, ButtonState, ButtonVariant, CardStyle, PanelStyle, TextVariant, WgpuTheme,
};
use crate::types::{
    Color, HitRegion, PanelLayout, Point, Rect, Scene, Size, Text, TextAlign, TextStyle,
};

/// Root builder. Wraps a [`Scene`], a [`PanelLayout<T>`], a theme reference,
/// and a content rect, exposing declarative methods for the components in the
/// shadcn / miklabs catalog. Method calls advance a vertical cursor so the
/// caller does not have to thread `cursor_y` through every helper.
pub struct UiBuilder<'a, T> {
    scene: &'a mut Scene,
    layout: &'a mut PanelLayout<T>,
    theme: &'a WgpuTheme,
    content: Rect,
    cursor_y: f32,
}

/// Declarative description of an inspector number row: label + value input,
/// optional +/- steppers, optional slider track.
///
/// This is the immediate-mode equivalent of building a miklabs `<Row>` with a
/// `<Text>` + `<Input>` + two `<Button variant=Ghost>` + `<Slider>` inside.
pub struct NumberRow<'a, T> {
    pub label: &'a str,
    pub value: &'a str,
    pub focused: bool,
    pub input_target: T,
    /// `(minus_target, plus_target)` — the hit targets for the stepper
    /// buttons. `None` hides the steppers entirely (e.g. fields without a
    /// declared range).
    pub steppers: Option<(T, T)>,
    /// `(fraction, target)` — slider fill fraction in `[0, 1]` plus the hit
    /// target for the track. `None` hides the slider.
    pub slider: Option<(f32, T)>,
}

/// Declarative description of an explorer / inspector list row: swatch +
/// label + detail line inside a card.
pub struct ListRow<'a, T> {
    pub label: &'a str,
    pub detail: &'a str,
    pub swatch_color: Color,
    pub selected: bool,
    /// Optional hit target. `None` means the row emits chrome only (the
    /// caller has already registered a hit region, or the row is readonly).
    pub target: Option<T>,
}

impl<'a, T> UiBuilder<'a, T> {
    /// New builder over an arbitrary content rect. The cursor starts at the
    /// top of the rect and advances downward as rows are emitted.
    pub fn new(
        scene: &'a mut Scene,
        layout: &'a mut PanelLayout<T>,
        theme: &'a WgpuTheme,
        content: Rect,
    ) -> Self {
        Self {
            scene,
            layout,
            theme,
            content,
            cursor_y: content.origin.y,
        }
    }

    /// Draw panel chrome around `rect`, register `rect` as the layout's panel
    /// rect (so `PanelLayout::contains_panel` swallows chrome clicks), and
    /// return a builder whose cursor sits at the top of the padded content.
    ///
    /// This is the shadcn / miklabs `<Card>` — `<View>` composition point: an
    /// scene starts with `UiBuilder::panel(...)` and chains component calls
    /// into its body, rather than drawing chrome separately and then wiring a
    /// second builder over the inner rect by hand.
    pub fn panel(
        scene: &'a mut Scene,
        layout: &'a mut PanelLayout<T>,
        theme: &'a WgpuTheme,
        rect: Rect,
        style: PanelStyle,
    ) -> Self {
        let content = components::panel(scene, rect, style);
        layout.panel_rect = Some(rect);
        Self {
            scene,
            layout,
            theme,
            content,
            cursor_y: content.origin.y,
        }
    }

    /// Same as [`Self::panel`] but also emits a `title` row at the top using
    /// `theme.title_style`, and places the cursor below the title. Mirrors
    /// the miklabs `<Card header="…">` pattern.
    pub fn titled_panel(
        scene: &'a mut Scene,
        layout: &'a mut PanelLayout<T>,
        theme: &'a WgpuTheme,
        rect: Rect,
        title: &str,
        style: PanelStyle,
    ) -> Self {
        let content = components::titled_panel(scene, rect, title, style, theme.title_style);
        layout.panel_rect = Some(rect);
        Self {
            scene,
            layout,
            theme,
            content,
            cursor_y: content.origin.y,
        }
    }

    /// Borrow the underlying scene. Reserved for corner cases (e.g. emitting
    /// a primitive the builder does not wrap yet); most call sites should
    /// compose via the builder methods.
    pub fn scene(&mut self) -> &mut Scene {
        self.scene
    }

    /// Borrow the underlying panel layout. Use this to push bespoke hit
    /// regions alongside the standard row widgets.
    pub fn layout(&mut self) -> &mut PanelLayout<T> {
        self.layout
    }

    /// Read-only access to the theme in use.
    pub fn theme(&self) -> &WgpuTheme {
        self.theme
    }

    /// Content rect the builder lays out into.
    pub fn content(&self) -> Rect {
        self.content
    }

    /// Current Y cursor — `content.origin.y + sum(emitted heights)`.
    pub fn cursor_y(&self) -> f32 {
        self.cursor_y
    }

    /// Reset the cursor to an absolute y coordinate. Useful when the caller
    /// wants to restart layout (e.g. after a scrolled region) or jump over a
    /// precomputed block (e.g. the panel's blurb area).
    pub fn set_cursor_y(&mut self, y: f32) -> &mut Self {
        self.cursor_y = y;
        self
    }

    /// Advance the cursor by `px` without emitting anything. Matches a
    /// miklabs `<View height=px />` spacer.
    pub fn gap(&mut self, px: f32) -> &mut Self {
        self.cursor_y += px;
        self
    }

    /// Reserve a row of `height` pixels at the current cursor. Returns the
    /// rect and advances the cursor by `height`. Callers who want a trailing
    /// gap should pair this with [`Self::gap`].
    pub fn reserve_row(&mut self, height: f32) -> Rect {
        let rect = Rect::new(
            Point::new(self.content.origin.x, self.cursor_y),
            Size::new(self.content.size.width, height),
        );
        self.cursor_y += height;
        rect
    }

    /// Emit a single-line heading at the current cursor.
    pub fn heading(&mut self, text: impl Into<String>) -> &mut Self {
        let style = self.theme.text_style(TextVariant::Heading);
        let rect = self.reserve_row(style.line_height_px);
        components::label(self.scene, rect, text, style);
        self
    }

    /// Emit a single-line subheading at the current cursor. Matches the
    /// amber-tinted "Terrain patch" subtitle used in the inspector.
    pub fn subheading(&mut self, text: impl Into<String>) -> &mut Self {
        let style = self.theme.text_style(TextVariant::Subheading);
        let rect = self.reserve_row(style.line_height_px);
        components::label(self.scene, rect, text, style);
        self
    }

    /// Emit a single-line block of text using `variant`.
    pub fn text(&mut self, text: impl Into<String>, variant: TextVariant) -> &mut Self {
        let style = self.theme.text_style(variant);
        let rect = self.reserve_row(style.line_height_px);
        components::label(self.scene, rect, text, style);
        self
    }

    /// Emit a multi-line block (already joined with `\n`) inside a rect of
    /// exactly `height` pixels. Text overflowing the rect clips through the
    /// bitmap glyph path like any other `Scene::text` primitive.
    pub fn paragraph(&mut self, text: impl Into<String>, height: f32) -> &mut Self {
        let rect = self.reserve_row(height);
        self.scene.text(Text {
            rect,
            content: text.into(),
            style: self.theme.body_style,
        });
        self
    }

    /// Emit a readonly `label: value` row — card chrome, left-aligned label,
    /// right-aligned value.
    pub fn readonly_row(&mut self, label: &str, value: &str) -> &mut Self {
        let row_rect = self.reserve_row(28.0);
        let card_style = self.theme.card();
        let body = self.theme.body_style;
        components::card(self.scene, row_rect, card_style);
        components::label(self.scene, label_rect(row_rect), label, body);
        let value_style = TextStyle {
            align: TextAlign::End,
            color: Color::rgba(0.98, 0.94, 0.88, 0.96),
            ..body
        };
        self.scene.text(Text {
            rect: value_text_rect(row_rect),
            content: value.to_string(),
            style: value_style,
        });
        self.gap(6.0)
    }

    /// Emit a number row: label + right-aligned input, optional +/- steppers,
    /// optional slider track. Hit regions for each interactive widget are
    /// pushed into the underlying [`PanelLayout<T>`] with the targets from
    /// `row`.
    pub fn number_row(&mut self, row: NumberRow<'_, T>) -> &mut Self
    where
        T: Copy,
    {
        let row_height: f32 = 60.0;
        let row_rect = self.reserve_row(row_height);
        components::card(self.scene, row_rect, self.theme.card());
        components::label(
            self.scene,
            label_rect(row_rect),
            row.label,
            self.theme.body_style,
        );

        let input_width = 88.0;
        let input_rect = Rect::new(
            Point::new(
                row_rect.origin.x + row_rect.size.width - input_width - 10.0,
                row_rect.origin.y + 5.0,
            ),
            Size::new(input_width, 20.0),
        );
        components::text_field(
            self.scene,
            input_rect,
            row.value,
            self.theme.input(row.focused),
        );
        self.layout.hit_regions.push(HitRegion {
            rect: input_rect,
            target: row.input_target,
        });

        let Some((minus, plus)) = row.steppers else {
            return self.gap(6.0);
        };

        let stepper_size = 20.0_f32;
        let stepper_y = row_rect.origin.y + row_height - stepper_size - 8.0;
        let plus_rect = Rect::new(
            Point::new(
                row_rect.origin.x + row_rect.size.width - stepper_size - 10.0,
                stepper_y,
            ),
            Size::new(stepper_size, stepper_size),
        );
        let minus_rect = Rect::new(
            Point::new(plus_rect.origin.x - stepper_size - 6.0, stepper_y),
            Size::new(stepper_size, stepper_size),
        );
        for (rect, symbol, target) in [(minus_rect, "-", minus), (plus_rect, "+", plus)] {
            components::button(
                self.scene,
                rect,
                symbol,
                ButtonVariant::Ghost,
                ButtonSize::Icon,
                ButtonState::Idle,
                self.theme,
            );
            self.layout.hit_regions.push(HitRegion { rect, target });
        }

        if let Some((fraction, slider_target)) = row.slider {
            let track_left = row_rect.origin.x + 10.0;
            let track_right = minus_rect.origin.x - 8.0;
            let track_width = (track_right - track_left).max(40.0);
            let track_rect = Rect::new(
                Point::new(track_left, stepper_y + 6.0),
                Size::new(track_width, 8.0),
            );
            let regions =
                components::slider(self.scene, track_rect, fraction, &self.theme.slider());
            self.layout.hit_regions.push(HitRegion {
                rect: regions.hit_rect,
                target: slider_target,
            });
        }

        self.gap(6.0)
    }

    /// Emit a picker row: label + horizontal chip group. The chip group is
    /// wired through [`components::chip_group`], so hit regions are pushed
    /// into the layout with one `to_target(index)` call per chip.
    pub fn picker_row(
        &mut self,
        label: &str,
        options: &[&str],
        selected: usize,
        to_target: impl Fn(usize) -> T,
    ) -> &mut Self {
        let row_rect = self.reserve_row(50.0);
        components::card(self.scene, row_rect, self.theme.card());
        components::label(
            self.scene,
            label_rect(row_rect),
            label,
            self.theme.body_style,
        );
        components::chip_group(
            self.scene,
            self.layout,
            row_rect,
            options,
            selected,
            ButtonVariant::Outline,
            ButtonSize::Sm,
            self.theme,
            to_target,
        );
        self.gap(6.0)
    }

    /// Emit a list row at the current cursor — swatch + label + detail inside
    /// a `Card` that flips to `card_selected` when `selected` is true. Pushes
    /// one hit region when `target` is `Some`.
    pub fn list_row(&mut self, height: f32, spec: ListRow<'_, T>) -> &mut Self {
        let rect = self.reserve_row(height);
        self.list_row_at(rect, spec);
        self
    }

    /// Emit a list row into a precomputed `rect`. Used when the caller owns
    /// its own layout pass (e.g. the virtualized list viewport, which
    /// decides row positions outside the builder's linear cursor).
    pub fn list_row_at(&mut self, rect: Rect, spec: ListRow<'_, T>) -> &mut Self {
        let card_style = if spec.selected {
            self.theme.card_selected()
        } else {
            self.theme.card()
        };
        draw_list_row(
            self.scene,
            rect,
            card_style,
            spec.swatch_color,
            spec.label,
            spec.detail,
            self.theme,
        );
        if let Some(target) = spec.target {
            self.layout.hit_regions.push(HitRegion { rect, target });
        }
        self
    }
}

fn label_rect(row: Rect) -> Rect {
    Rect::new(
        Point::new(row.origin.x + 10.0, row.origin.y + 5.0),
        Size::new(row.size.width * 0.48, 18.0),
    )
}

fn value_text_rect(row: Rect) -> Rect {
    Rect::new(
        Point::new(row.origin.x + 10.0, row.origin.y + 5.0),
        Size::new(row.size.width - 20.0, 18.0),
    )
}

fn draw_list_row(
    scene: &mut Scene,
    rect: Rect,
    card_style: CardStyle,
    swatch_color: Color,
    label: &str,
    detail: &str,
    theme: &WgpuTheme,
) {
    components::card(scene, rect, card_style);

    let swatch_size = 14.0_f32;
    let swatch_rect = Rect::new(
        Point::new(
            rect.origin.x + 8.0,
            rect.origin.y + (rect.size.height - swatch_size) * 0.5,
        ),
        Size::new(swatch_size, swatch_size),
    );
    components::swatch(scene, swatch_rect, swatch_color, theme.swatch());

    let label_style = theme.body_style;
    let detail_style = theme.text_style(TextVariant::Muted);
    let text_origin_x = swatch_rect.origin.x + swatch_rect.size.width + 8.0;
    let text_width = rect.origin.x + rect.size.width - text_origin_x - 8.0;
    let text_block_height = label_style.line_height_px + detail_style.line_height_px + 2.0;
    let text_top = rect.origin.y + (rect.size.height - text_block_height) * 0.5;
    components::label(
        scene,
        Rect::new(
            Point::new(text_origin_x, text_top),
            Size::new(text_width, label_style.line_height_px),
        ),
        label.to_string(),
        label_style,
    );
    scene.text(Text {
        rect: Rect::new(
            Point::new(text_origin_x, text_top + label_style.line_height_px + 2.0),
            Size::new(text_width, detail_style.line_height_px),
        ),
        content: detail.to_string(),
        style: detail_style,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Axis, Primitive};

    fn fresh() -> (Scene, PanelLayout<&'static str>, WgpuTheme, Rect) {
        (
            Scene::new(Size::new(800.0, 600.0)),
            PanelLayout::default(),
            WgpuTheme::default(),
            Rect::new(Point::new(20.0, 30.0), Size::new(260.0, 400.0)),
        )
    }

    #[test]
    fn heading_and_gap_advance_cursor_by_exact_amounts() {
        let (mut scene, mut layout, theme, content) = fresh();
        let mut ui = UiBuilder::<&'static str>::new(&mut scene, &mut layout, &theme, content);
        let start = ui.cursor_y();
        ui.heading("Inspector");
        let after_heading = ui.cursor_y();
        ui.gap(4.0);
        assert_eq!(
            after_heading - start,
            theme.title_style.line_height_px,
            "heading should advance cursor by its line height"
        );
        assert!((ui.cursor_y() - after_heading - 4.0).abs() < f32::EPSILON);
    }

    #[test]
    fn readonly_row_emits_card_with_two_text_primitives() {
        let (mut scene, mut layout, theme, content) = fresh();
        let mut ui = UiBuilder::<&'static str>::new(&mut scene, &mut layout, &theme, content);
        ui.readonly_row("area", "1.4 m²");
        // card quad + label + value => at least 1 quad + 2 texts.
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
        assert!(quads >= 1);
        assert!(texts >= 2);
    }

    #[test]
    fn number_row_pushes_input_stepper_and_slider_hit_regions() {
        let (mut scene, mut layout, theme, content) = fresh();
        let mut ui = UiBuilder::<&'static str>::new(&mut scene, &mut layout, &theme, content);
        ui.number_row(NumberRow {
            label: "height",
            value: "1.20 m",
            focused: false,
            input_target: "input",
            steppers: Some(("minus", "plus")),
            slider: Some((0.5, "track")),
        });
        let targets: Vec<_> = layout.hit_regions.iter().map(|r| r.target).collect();
        assert!(targets.contains(&"input"));
        assert!(targets.contains(&"minus"));
        assert!(targets.contains(&"plus"));
        assert!(targets.contains(&"track"));
    }

    #[test]
    fn number_row_without_steppers_skips_slider_and_buttons() {
        let (mut scene, mut layout, theme, content) = fresh();
        let mut ui = UiBuilder::<&'static str>::new(&mut scene, &mut layout, &theme, content);
        ui.number_row(NumberRow {
            label: "area",
            value: "1.4 m²",
            focused: false,
            input_target: "input",
            steppers: None,
            slider: Some((0.5, "track")),
        });
        let targets: Vec<_> = layout.hit_regions.iter().map(|r| r.target).collect();
        assert_eq!(targets, vec!["input"]);
    }

    #[test]
    fn picker_row_pushes_one_hit_region_per_option() {
        let (mut scene, mut layout, theme, content) = fresh();
        let mut ui = UiBuilder::<&'static str>::new(&mut scene, &mut layout, &theme, content);
        ui.picker_row("kind", &["A", "B", "C"], 1, |i| match i {
            0 => "a",
            1 => "b",
            _ => "c",
        });
        assert_eq!(layout.hit_regions.len(), 3);
        assert_eq!(layout.hit_regions[1].target, "b");
    }

    #[test]
    fn list_row_at_emits_card_swatch_label_and_detail() {
        let (mut scene, mut layout, theme, _) = fresh();
        let rect = Rect::new(Point::new(10.0, 40.0), Size::new(240.0, 44.0));
        let mut ui = UiBuilder::<&'static str>::new(
            &mut scene,
            &mut layout,
            &theme,
            Rect::new(Point::new(0.0, 0.0), Size::new(260.0, 400.0)),
        );
        ui.list_row_at(
            rect,
            ListRow {
                label: "Wall #1",
                detail: "3.2 m · Granite",
                swatch_color: Color::rgba(0.5, 0.5, 0.5, 1.0),
                selected: true,
                target: Some("wall-1"),
            },
        );
        let fill = theme.card_selected().fill;
        assert!(
            scene
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Quad(q) if q.fill == fill)),
            "selected list row should use card_selected fill"
        );
        assert!(scene
            .primitives
            .iter()
            .any(|p| matches!(p, Primitive::Text(t) if t.content == "Wall #1")));
        assert_eq!(layout.hit_regions.len(), 1);
        assert_eq!(layout.hit_regions[0].target, "wall-1");
    }

    #[test]
    fn axis_is_still_accessible_through_prelude_reexport() {
        // Sanity: Axis is part of prelude so builder users can pass it to
        // future stack-based helpers without importing it from `types`.
        let _ = Axis::Vertical;
    }

    #[test]
    fn panel_draws_chrome_and_registers_panel_rect() {
        let (mut scene, mut layout, theme, _) = fresh();
        let rect = Rect::new(Point::new(40.0, 60.0), Size::new(200.0, 120.0));
        {
            let mut ui = UiBuilder::<&'static str>::panel(
                &mut scene,
                &mut layout,
                &theme,
                rect,
                theme.panel_style(),
            );
            ui.heading("Panel");
        }
        assert_eq!(layout.panel_rect, Some(rect));
        // Panel chrome emits at least a shadow + a quad with the panel fill.
        assert!(
            scene
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Shadow(_))),
            "panel chrome should emit a shadow primitive"
        );
    }

    #[test]
    fn titled_panel_places_cursor_below_title_row() {
        let (mut scene, mut layout, theme, _) = fresh();
        let rect = Rect::new(Point::new(0.0, 0.0), Size::new(240.0, 180.0));
        let inner_content_top = {
            let ui = UiBuilder::<&'static str>::titled_panel(
                &mut scene,
                &mut layout,
                &theme,
                rect,
                "Inspector",
                theme.panel_style(),
            );
            ui.cursor_y()
        };
        let padded_top = rect.inset(theme.panel_style().padding).origin.y;
        assert!(
            inner_content_top > padded_top,
            "titled_panel cursor ({inner_content_top}) should start below the padded content top ({padded_top})"
        );
        assert!(
            scene
                .primitives
                .iter()
                .any(|p| matches!(p, Primitive::Text(t) if t.content == "Inspector")),
            "titled_panel should emit the title text"
        );
    }
}
