//! Headless slider — a stepped, keyboard/gamepad-operable numeric range.
//!
//! Mirrors [`Toggle`](super::toggle::Toggle): logic-only, identical across
//! every backend. A backend's render layer reads [`Slider::fraction`] (or
//! [`Slider::value`]) each frame and draws accordingly — this module owns no
//! pixels.

use super::traits::{Focusable, HeadlessComponent, KeyboardInteractable};
use crate::event::Event;
use crate::state::State;

/// `PageUp` / `PageDown` move this many regular steps at once.
const PAGE_STEP_FACTOR: f32 = 10.0;

/// State for a slider component.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderState {
    pub min: f32,
    pub max: f32,
    /// Keyboard/gamepad increment size. `value` is always a multiple of
    /// `step` measured from `min` (see [`Slider::set_value`]).
    pub step: f32,
    pub value: f32,
    pub disabled: bool,
    pub focused: bool,
    /// Whether a pointer drag is currently in progress — the signal a
    /// backend's render layer uses to decide whether to show a live value
    /// label (issue #190).
    pub dragging: bool,
}

impl State for SliderState {}

/// Events for a slider component.
#[derive(Debug, Clone)]
pub enum SliderEvent {
    Increment,
    Decrement,
    PageIncrement,
    PageDecrement,
    JumpToMin,
    JumpToMax,
    SetValue(f32),
    SetFraction(f32),
    BeginDrag,
    EndDrag,
    Focus,
    Blur,
    KeyDown(String),
}

impl Event for SliderEvent {}

/// Headless slider component with complete state management and a11y.
pub struct Slider {
    state: SliderState,
    on_change: Option<Box<dyn Fn(f32)>>,
}

impl Slider {
    pub fn builder() -> SliderBuilder {
        SliderBuilder::new()
    }

    pub fn value(&self) -> f32 {
        self.state.value
    }

    /// Current value expressed as a `[0.0, 1.0]` fraction of `min..=max`.
    /// `0.0` when `max <= min` rather than dividing by zero.
    pub fn fraction(&self) -> f32 {
        let span = self.state.max - self.state.min;
        if span <= 0.0 {
            return 0.0;
        }
        (self.state.value - self.state.min) / span
    }

    pub fn is_disabled(&self) -> bool {
        self.state.disabled
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.state.disabled = disabled;
    }

    pub fn is_dragging(&self) -> bool {
        self.state.dragging
    }

    pub fn begin_drag(&mut self) {
        if self.state.disabled {
            return;
        }
        self.state.dragging = true;
    }

    pub fn end_drag(&mut self) {
        self.state.dragging = false;
    }

    /// Clamp to `[min, max]`, then snap to the nearest multiple of `step`
    /// measured from `min` — snapping from a fixed origin on every call
    /// (rather than drifting the running value by `+= step`) keeps repeated
    /// increments exact (e.g. ten 0.1 steps land on exactly 1.0).
    ///
    /// `min` and `max` are returned untouched whenever `value` clamps to
    /// either of them, *before* the step-grid rounding below runs — when
    /// `step` doesn't evenly divide `max - min` (e.g. `min=0`, `max=10`,
    /// `step=3`), the nearest grid point short of `max` (`9`) is itself a
    /// valid, in-range value, so the final `.clamp()` can't catch it; the
    /// range's own endpoints would otherwise be unreachable via `End` /
    /// `jump_to_max` or a full-width drag (`set_fraction(1.0)`).
    fn snap(&self, value: f32) -> f32 {
        let clamped = value.clamp(self.state.min, self.state.max);
        if clamped == self.state.min || clamped == self.state.max {
            return clamped;
        }
        if self.state.step <= 0.0 {
            return clamped;
        }
        let steps = ((clamped - self.state.min) / self.state.step).round();
        (self.state.min + steps * self.state.step).clamp(self.state.min, self.state.max)
    }

    pub fn set_value(&mut self, value: f32) {
        if self.state.disabled {
            return;
        }
        let snapped = self.snap(value);
        // Exact comparison, not an epsilon threshold: `snapped` is always
        // the deterministic output of `snap` for the current `state`, so
        // two different step counts are "the same" only when they produce
        // bit-identical floats. An absolute `f32::EPSILON` threshold instead
        // discarded real changes on a tiny range/step (e.g. `max=0.000001`,
        // `step=0.0000001`) whose representable deltas are themselves
        // smaller than `f32::EPSILON`, leaving `value` stuck forever.
        if self.state.value != snapped {
            self.state.value = snapped;
            if let Some(on_change) = &self.on_change {
                on_change(snapped);
            }
        }
    }

    /// Set the value from a `[0.0, 1.0]` fraction — the pointer-drag entry
    /// point, since a drag naturally produces a fraction of the track width.
    pub fn set_fraction(&mut self, fraction: f32) {
        let fraction = fraction.clamp(0.0, 1.0);
        let value = self.state.min + fraction * (self.state.max - self.state.min);
        self.set_value(value);
    }

    pub fn increment(&mut self) {
        self.set_value(self.state.value + self.state.step);
    }

    pub fn decrement(&mut self) {
        self.set_value(self.state.value - self.state.step);
    }

    pub fn page_increment(&mut self) {
        self.set_value(self.state.value + self.state.step * PAGE_STEP_FACTOR);
    }

    pub fn page_decrement(&mut self) {
        self.set_value(self.state.value - self.state.step * PAGE_STEP_FACTOR);
    }

    pub fn jump_to_min(&mut self) {
        self.set_value(self.state.min);
    }

    pub fn jump_to_max(&mut self) {
        self.set_value(self.state.max);
    }
}

impl HeadlessComponent for Slider {
    type State = SliderState;
    type Event = SliderEvent;

    fn new() -> Self {
        Self {
            state: SliderState {
                min: 0.0,
                max: 1.0,
                step: 0.1,
                value: 0.0,
                disabled: false,
                focused: false,
                dragging: false,
            },
            on_change: None,
        }
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn handle_event(&mut self, event: Self::Event) {
        match event {
            SliderEvent::Increment => self.increment(),
            SliderEvent::Decrement => self.decrement(),
            SliderEvent::PageIncrement => self.page_increment(),
            SliderEvent::PageDecrement => self.page_decrement(),
            SliderEvent::JumpToMin => self.jump_to_min(),
            SliderEvent::JumpToMax => self.jump_to_max(),
            SliderEvent::SetValue(v) => self.set_value(v),
            SliderEvent::SetFraction(f) => self.set_fraction(f),
            SliderEvent::BeginDrag => self.begin_drag(),
            SliderEvent::EndDrag => self.end_drag(),
            SliderEvent::Focus => self.focus(),
            SliderEvent::Blur => self.blur(),
            SliderEvent::KeyDown(key) => self.handle_key_down(&key),
        }
    }
}

impl Focusable for Slider {
    fn focus(&mut self) {
        self.state.focused = true;
    }

    fn blur(&mut self) {
        self.state.focused = false;
    }

    fn is_focused(&self) -> bool {
        self.state.focused
    }
}

/// `ArrowRight`/`ArrowUp` increment and `ArrowLeft`/`ArrowDown` decrement —
/// matching the W3C APG slider pattern. This is also the gamepad hook: a
/// future dpad-to-action mapping (#203) calls the same `increment` /
/// `decrement` / `page_increment` / `page_decrement` methods this match
/// arm does.
impl KeyboardInteractable for Slider {
    fn handle_key_down(&mut self, key: &str) {
        if self.state.disabled {
            return;
        }

        match key {
            "ArrowRight" | "ArrowUp" => self.increment(),
            "ArrowLeft" | "ArrowDown" => self.decrement(),
            "PageUp" => self.page_increment(),
            "PageDown" => self.page_decrement(),
            "Home" => self.jump_to_min(),
            "End" => self.jump_to_max(),
            _ => {}
        }
    }

    fn handle_key_up(&mut self, _key: &str) {
        // No-op for slider.
    }
}

/// Builder for the `Slider` component.
pub struct SliderBuilder {
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    disabled: bool,
    on_change: Option<Box<dyn Fn(f32)>>,
}

impl SliderBuilder {
    pub fn new() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            step: 0.1,
            value: 0.0,
            disabled: false,
            on_change: None,
        }
    }

    pub fn min(mut self, min: f32) -> Self {
        self.min = min;
        self
    }

    pub fn max(mut self, max: f32) -> Self {
        self.max = max;
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step;
        self
    }

    pub fn value(mut self, value: f32) -> Self {
        self.value = value;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change<F: Fn(f32) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    /// Builds the slider. Guards degenerate input rather than producing a
    /// `NaN` fraction or an infinite step count: `max <= min` widens `max`
    /// to `min + 1.0`, and `step <= 0.0` falls back to a 100-step default
    /// across the (possibly-widened) range.
    pub fn build(self) -> Slider {
        let min = self.min;
        let max = if self.max > min { self.max } else { min + 1.0 };
        let step = if self.step > 0.0 {
            self.step
        } else {
            (max - min) / 100.0
        };

        let mut slider = Slider {
            state: SliderState {
                min,
                max,
                step,
                value: self.value,
                disabled: self.disabled,
                focused: false,
                dragging: false,
            },
            on_change: self.on_change,
        };
        // Snap the initial value without firing `on_change` for it.
        slider.state.value = slider.snap(self.value);
        slider
    }
}

impl Default for SliderBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    #[test]
    fn ten_tenth_steps_land_on_exactly_one() {
        let mut slider = Slider::builder()
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .value(0.0)
            .build();
        for _ in 0..10 {
            slider.increment();
        }
        assert!((slider.value() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn increment_clamps_at_max_and_decrement_clamps_at_min() {
        let mut slider = Slider::builder()
            .min(0.0)
            .max(1.0)
            .step(0.1)
            .value(0.95)
            .build();
        slider.increment();
        assert!((slider.value() - 1.0).abs() < f32::EPSILON);
        slider.set_value(0.0);
        slider.decrement();
        assert!((slider.value() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn disabled_slider_ignores_every_mutation() {
        let mut slider = Slider::builder().value(0.5).disabled(true).build();
        slider.increment();
        slider.set_fraction(1.0);
        slider.handle_key_down("ArrowRight");
        assert!((slider.value() - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn arrow_right_increments_via_the_shared_key_contract() {
        // Key::Right.as_str() is the single source of truth for the literal
        // string a backend's keyboard router must forward — pin the slider's
        // match arm against it instead of a bare `"ArrowRight"` literal.
        let mut slider = Slider::builder()
            .min(0.0)
            .max(1.0)
            .step(0.25)
            .value(0.0)
            .build();
        slider.handle_key_down(Key::Right.as_str().unwrap());
        assert!((slider.value() - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn page_up_moves_ten_steps_and_home_end_jump_to_range_bounds() {
        let mut slider = Slider::builder()
            .min(0.0)
            .max(10.0)
            .step(1.0)
            .value(0.0)
            .build();
        slider.handle_key_down("PageUp");
        assert!((slider.value() - 10.0).abs() < f32::EPSILON);
        slider.handle_key_down("Home");
        assert!((slider.value() - 0.0).abs() < f32::EPSILON);
        slider.handle_key_down("End");
        assert!((slider.value() - 10.0).abs() < f32::EPSILON);
    }

    #[test]
    fn on_change_fires_only_when_the_value_actually_changes() {
        use std::cell::Cell;
        use std::rc::Rc;
        let calls = Rc::new(Cell::new(0));
        let calls_in = Rc::clone(&calls);
        let mut slider = Slider::builder()
            .value(0.5)
            .on_change(move |_| calls_in.set(calls_in.get() + 1))
            .build();
        slider.set_value(0.5);
        assert_eq!(calls.get(), 0);
        slider.set_value(0.6);
        assert_eq!(calls.get(), 1);
    }

    #[test]
    fn degenerate_range_and_step_fall_back_instead_of_producing_nan() {
        let slider = Slider::builder().min(5.0).max(5.0).step(0.0).build();
        assert!(slider.fraction().is_finite());
    }

    #[test]
    fn fraction_round_trips_through_set_fraction() {
        let mut slider = Slider::builder()
            .min(0.0)
            .max(1.0)
            .step(0.01)
            .value(0.0)
            .build();
        slider.set_fraction(0.5);
        assert!((slider.fraction() - 0.5).abs() < 0.01);
    }

    /// Regression: when `step` doesn't evenly divide `max - min` (10 / 3),
    /// the nearest step-grid point below `max` (`9`) is itself in range, so
    /// a plain `.clamp()` after rounding can't recover the endpoint —
    /// `max` must be preserved explicitly.
    #[test]
    fn max_is_reachable_when_step_does_not_evenly_divide_the_range() {
        let mut jumped = Slider::builder().min(0.0).max(10.0).step(3.0).build();
        jumped.jump_to_max();
        assert_eq!(
            jumped.value(),
            10.0,
            "End/jump_to_max must land exactly on max even when step doesn't evenly divide the range"
        );

        let mut dragged = Slider::builder().min(0.0).max(10.0).step(3.0).build();
        dragged.set_fraction(1.0);
        assert_eq!(
            dragged.value(),
            10.0,
            "dragging fully to the end must also land exactly on max"
        );
    }

    /// Regression: `min` gets the same treatment as `max` for symmetry,
    /// even though the un-evenly-dividing case can't actually overshoot it.
    #[test]
    fn min_is_reachable_when_step_does_not_evenly_divide_the_range() {
        let mut slider = Slider::builder()
            .min(1.0)
            .max(10.0)
            .step(3.0)
            .value(5.0)
            .build();
        slider.jump_to_min();
        assert_eq!(slider.value(), 1.0);
    }

    /// Regression: an absolute `f32::EPSILON` threshold in `set_value`
    /// silently discarded a representable change on a tiny range/step,
    /// leaving `value` stuck at `0.0` forever.
    #[test]
    fn tiny_steps_on_a_tiny_range_still_register_as_changes() {
        let mut slider = Slider::builder()
            .min(0.0)
            .max(0.000_001)
            .step(0.000_000_1)
            .value(0.0)
            .build();
        slider.increment();
        assert_ne!(
            slider.value(),
            0.0,
            "a representable step must register as a change"
        );

        for _ in 0..20 {
            slider.increment();
        }
        assert!(
            (slider.value() - 0.000_001).abs() < 1e-9,
            "repeated representable increments must still reach max, got {}",
            slider.value()
        );
    }
}
