//! Headless segmented select — a small, fixed set of mutually-exclusive
//! options (language picker, a small enum choice) selectable by index.
//!
//! Mirrors [`Toggle`](super::toggle::Toggle) and
//! [`Slider`](super::slider::Slider): logic-only, identical across every
//! backend. A backend's render layer reads [`SegmentedSelect::selected`]
//! each frame and draws accordingly.

use super::traits::{Focusable, HeadlessComponent, KeyboardInteractable};
use crate::event::Event;
use crate::state::State;

/// State for a segmented-select component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentedSelectState {
    pub selected: usize,
    pub option_count: usize,
    pub disabled: bool,
    pub focused: bool,
}

impl State for SegmentedSelectState {}

/// Events for a segmented-select component.
#[derive(Debug, Clone)]
pub enum SegmentedSelectEvent {
    Select(usize),
    SelectNext,
    SelectPrevious,
    SelectFirst,
    SelectLast,
    Focus,
    Blur,
    KeyDown(String),
}

impl Event for SegmentedSelectEvent {}

/// Headless segmented-select component with complete state management and
/// a11y.
pub struct SegmentedSelect {
    state: SegmentedSelectState,
    on_change: Option<Box<dyn Fn(usize)>>,
}

impl SegmentedSelect {
    pub fn builder() -> SegmentedSelectBuilder {
        SegmentedSelectBuilder::new()
    }

    pub fn selected(&self) -> usize {
        self.state.selected
    }

    pub fn option_count(&self) -> usize {
        self.state.option_count
    }

    pub fn is_disabled(&self) -> bool {
        self.state.disabled
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.state.disabled = disabled;
    }

    pub fn select(&mut self, index: usize) {
        if self.state.disabled || self.state.option_count == 0 {
            return;
        }
        let index = index.min(self.state.option_count - 1);
        if self.state.selected != index {
            self.state.selected = index;
            if let Some(on_change) = &self.on_change {
                on_change(index);
            }
        }
    }

    /// Moves to the next option. Clamps at the last option rather than
    /// wrapping back to the first — matching `select_previous`'s symmetric
    /// clamp at `0`, and avoiding a surprising jump from the last option
    /// straight back to the first on one more key press.
    pub fn select_next(&mut self) {
        if self.state.option_count == 0 {
            return;
        }
        self.select((self.state.selected + 1).min(self.state.option_count - 1));
    }

    /// Moves to the previous option, clamping at `0` (see
    /// [`Self::select_next`] for the no-wrap rationale).
    pub fn select_previous(&mut self) {
        self.select(self.state.selected.saturating_sub(1));
    }

    pub fn select_first(&mut self) {
        self.select(0);
    }

    pub fn select_last(&mut self) {
        if self.state.option_count > 0 {
            self.select(self.state.option_count - 1);
        }
    }
}

impl HeadlessComponent for SegmentedSelect {
    type State = SegmentedSelectState;
    type Event = SegmentedSelectEvent;

    fn new() -> Self {
        Self {
            state: SegmentedSelectState {
                selected: 0,
                option_count: 0,
                disabled: false,
                focused: false,
            },
            on_change: None,
        }
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn handle_event(&mut self, event: Self::Event) {
        match event {
            SegmentedSelectEvent::Select(index) => self.select(index),
            SegmentedSelectEvent::SelectNext => self.select_next(),
            SegmentedSelectEvent::SelectPrevious => self.select_previous(),
            SegmentedSelectEvent::SelectFirst => self.select_first(),
            SegmentedSelectEvent::SelectLast => self.select_last(),
            SegmentedSelectEvent::Focus => self.focus(),
            SegmentedSelectEvent::Blur => self.blur(),
            SegmentedSelectEvent::KeyDown(key) => self.handle_key_down(&key),
        }
    }
}

impl Focusable for SegmentedSelect {
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

/// `ArrowRight`/`ArrowDown` select the next option, `ArrowLeft`/`ArrowUp`
/// the previous — matching the W3C APG "Tabs"/radio-group pattern most
/// segmented controls borrow from. This is also the gamepad hook: a future
/// dpad-to-action mapping (#203) calls the same `select_next` /
/// `select_previous` methods this match arm does.
impl KeyboardInteractable for SegmentedSelect {
    fn handle_key_down(&mut self, key: &str) {
        if self.state.disabled {
            return;
        }

        match key {
            "ArrowRight" | "ArrowDown" => self.select_next(),
            "ArrowLeft" | "ArrowUp" => self.select_previous(),
            "Home" => self.select_first(),
            "End" => self.select_last(),
            _ => {}
        }
    }

    fn handle_key_up(&mut self, _key: &str) {
        // No-op for segmented select.
    }
}

/// Builder for the `SegmentedSelect` component.
pub struct SegmentedSelectBuilder {
    option_count: usize,
    selected: usize,
    disabled: bool,
    on_change: Option<Box<dyn Fn(usize)>>,
}

impl SegmentedSelectBuilder {
    pub fn new() -> Self {
        Self {
            option_count: 0,
            selected: 0,
            disabled: false,
            on_change: None,
        }
    }

    pub fn option_count(mut self, option_count: usize) -> Self {
        self.option_count = option_count;
        self
    }

    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn on_change<F: Fn(usize) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }

    /// Builds the component, clamping an out-of-range initial `selected`
    /// into `0..option_count` rather than leaving it dangling.
    pub fn build(self) -> SegmentedSelect {
        let selected = if self.option_count == 0 {
            0
        } else {
            self.selected.min(self.option_count - 1)
        };
        SegmentedSelect {
            state: SegmentedSelectState {
                selected,
                option_count: self.option_count,
                disabled: self.disabled,
                focused: false,
            },
            on_change: self.on_change,
        }
    }
}

impl Default for SegmentedSelectBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Key;

    #[test]
    fn select_next_and_previous_clamp_instead_of_wrapping() {
        let mut select = SegmentedSelect::builder().option_count(3).build();
        assert_eq!(select.selected(), 0);
        select.select_previous();
        assert_eq!(select.selected(), 0, "must clamp at the first option");
        select.select_next();
        select.select_next();
        assert_eq!(select.selected(), 2);
        select.select_next();
        assert_eq!(select.selected(), 2, "must clamp at the last option");
    }

    #[test]
    fn arrow_keys_move_selection_via_the_shared_key_contract() {
        let mut select = SegmentedSelect::builder().option_count(3).build();
        select.handle_key_down(Key::Right.as_str().unwrap());
        assert_eq!(select.selected(), 1);
        select.handle_key_down(Key::Left.as_str().unwrap());
        assert_eq!(select.selected(), 0);
    }

    #[test]
    fn home_and_end_jump_to_the_first_and_last_option() {
        let mut select = SegmentedSelect::builder()
            .option_count(4)
            .selected(1)
            .build();
        select.handle_key_down("End");
        assert_eq!(select.selected(), 3);
        select.handle_key_down("Home");
        assert_eq!(select.selected(), 0);
    }

    #[test]
    fn disabled_select_ignores_every_mutation() {
        let mut select = SegmentedSelect::builder()
            .option_count(3)
            .selected(1)
            .disabled(true)
            .build();
        select.select(2);
        select.handle_key_down("ArrowRight");
        assert_eq!(select.selected(), 1);
    }

    #[test]
    fn zero_options_never_panics() {
        let mut select = SegmentedSelect::builder().build();
        select.select_next();
        select.select_previous();
        select.select(5);
        assert_eq!(select.selected(), 0);
    }

    #[test]
    fn out_of_range_initial_selection_clamps_at_build() {
        let select = SegmentedSelect::builder()
            .option_count(3)
            .selected(99)
            .build();
        assert_eq!(select.selected(), 2);
    }

    #[test]
    fn on_change_fires_only_when_the_selection_actually_changes() {
        use std::cell::Cell;
        use std::rc::Rc;
        let calls = Rc::new(Cell::new(0));
        let calls_in = Rc::clone(&calls);
        let mut select = SegmentedSelect::builder()
            .option_count(3)
            .on_change(move |_| calls_in.set(calls_in.get() + 1))
            .build();
        select.select(0);
        assert_eq!(calls.get(), 0);
        select.select(1);
        assert_eq!(calls.get(), 1);
    }
}
