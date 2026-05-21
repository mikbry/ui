use crate::event::Event;
use crate::state::State;

/// Core trait for all headless components
pub trait HeadlessComponent {
    type State: State;
    type Event: Event;

    fn new() -> Self;
    fn state(&self) -> &Self::State;
    fn handle_event(&mut self, event: Self::Event);
}

/// Trait for focusable components
pub trait Focusable {
    fn focus(&mut self);
    fn blur(&mut self);
    fn is_focused(&self) -> bool;
}

/// Trait for components with keyboard interaction
pub trait KeyboardInteractable {
    fn handle_key_down(&mut self, key: &str);
    fn handle_key_up(&mut self, key: &str);
}
