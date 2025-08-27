use super::traits::{HeadlessComponent, Focusable, KeyboardInteractable};
use crate::state::State;
use crate::event::Event;

/// State for a toggle component
#[derive(Debug, Clone, Default)]
pub struct ToggleState {
    pub checked: bool,
    pub disabled: bool,
    pub focused: bool,
}

impl State for ToggleState {}

/// Events for toggle component
#[derive(Debug, Clone)]
pub enum ToggleEvent {
    Toggle,
    Check,
    Uncheck,
    Focus,
    Blur,
    KeyDown(String),
}

impl Event for ToggleEvent {}

/// Headless toggle component with complete state management and a11y
pub struct Toggle {
    state: ToggleState,
    on_change: Option<Box<dyn Fn(bool)>>,
}

impl Toggle {
    pub fn builder() -> ToggleBuilder {
        ToggleBuilder::new()
    }
    
    pub fn is_checked(&self) -> bool {
        self.state.checked
    }
    
    pub fn set_checked(&mut self, checked: bool) {
        if self.state.disabled {
            return;
        }
        
        if self.state.checked != checked {
            self.state.checked = checked;
            if let Some(on_change) = &self.on_change {
                on_change(checked);
            }
        }
    }
    
    pub fn toggle(&mut self) {
        self.set_checked(!self.state.checked);
    }
    
    pub fn is_disabled(&self) -> bool {
        self.state.disabled
    }
    
    pub fn set_disabled(&mut self, disabled: bool) {
        self.state.disabled = disabled;
    }
}

impl HeadlessComponent for Toggle {
    type State = ToggleState;
    type Event = ToggleEvent;
    
    fn new() -> Self {
        Self {
            state: ToggleState::default(),
            on_change: None,
        }
    }
    
    fn state(&self) -> &Self::State {
        &self.state
    }
    
    fn handle_event(&mut self, event: Self::Event) {
        match event {
            ToggleEvent::Toggle => self.toggle(),
            ToggleEvent::Check => self.set_checked(true),
            ToggleEvent::Uncheck => self.set_checked(false),
            ToggleEvent::Focus => self.focus(),
            ToggleEvent::Blur => self.blur(),
            ToggleEvent::KeyDown(key) => self.handle_key_down(&key),
        }
    }
}

impl Focusable for Toggle {
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

impl KeyboardInteractable for Toggle {
    fn handle_key_down(&mut self, key: &str) {
        if self.state.disabled {
            return;
        }
        
        match key {
            " " | "Enter" => self.toggle(),
            _ => {}
        }
    }
    
    fn handle_key_up(&mut self, _key: &str) {
        // No-op for toggle
    }
}

/// Builder for Toggle component
pub struct ToggleBuilder {
    checked: bool,
    disabled: bool,
    on_change: Option<Box<dyn Fn(bool)>>,
}

impl ToggleBuilder {
    pub fn new() -> Self {
        Self {
            checked: false,
            disabled: false,
            on_change: None,
        }
    }
    
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }
    
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
    
    pub fn on_change<F: Fn(bool) + 'static>(mut self, f: F) -> Self {
        self.on_change = Some(Box::new(f));
        self
    }
    
    pub fn build(self) -> Toggle {
        Toggle {
            state: ToggleState {
                checked: self.checked,
                disabled: self.disabled,
                focused: false,
            },
            on_change: self.on_change,
        }
    }
}