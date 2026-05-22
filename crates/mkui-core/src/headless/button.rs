use super::traits::{Focusable, HeadlessComponent, KeyboardInteractable};
use crate::event::Event;
use crate::state::State;

/// State for a button component
#[derive(Debug, Clone, Default)]
pub struct ButtonState {
    pub disabled: bool,
    pub focused: bool,
    pub pressed: bool,
    pub loading: bool,
}

impl State for ButtonState {}

/// Events for button component
#[derive(Debug, Clone)]
pub enum ButtonEvent {
    Click,
    Press,
    Release,
    Focus,
    Blur,
    KeyDown(String),
    KeyUp(String),
}

impl Event for ButtonEvent {}

/// Button variants for styling
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Destructive,
    Outline,
    Ghost,
    Link,
}

/// Button sizes
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum ButtonSize {
    Small,
    Medium,
    Large,
}

/// Headless button component with complete state management and a11y
pub struct Button {
    state: ButtonState,
    text: String,
    variant: ButtonVariant,
    size: ButtonSize,
    on_click: Option<Box<dyn Fn()>>,
}

impl Button {
    pub fn builder() -> ButtonBuilder {
        ButtonBuilder::new()
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn variant(&self) -> &ButtonVariant {
        &self.variant
    }

    pub fn size(&self) -> &ButtonSize {
        &self.size
    }

    pub fn is_disabled(&self) -> bool {
        self.state.disabled
    }

    pub fn set_disabled(&mut self, disabled: bool) {
        self.state.disabled = disabled;
    }

    pub fn is_loading(&self) -> bool {
        self.state.loading
    }

    pub fn set_loading(&mut self, loading: bool) {
        self.state.loading = loading;
    }

    pub fn is_pressed(&self) -> bool {
        self.state.pressed
    }

    pub fn click(&mut self) {
        if self.state.disabled || self.state.loading {
            return;
        }

        if let Some(on_click) = &self.on_click {
            on_click();
        }
    }

    pub fn press(&mut self) {
        if self.state.disabled || self.state.loading {
            return;
        }
        self.state.pressed = true;
    }

    pub fn release(&mut self) {
        self.state.pressed = false;
        self.click();
    }
}

impl HeadlessComponent for Button {
    type State = ButtonState;
    type Event = ButtonEvent;

    fn new() -> Self {
        Self {
            state: ButtonState::default(),
            text: String::new(),
            variant: ButtonVariant::Primary,
            size: ButtonSize::Medium,
            on_click: None,
        }
    }

    fn state(&self) -> &Self::State {
        &self.state
    }

    fn handle_event(&mut self, event: Self::Event) {
        match event {
            ButtonEvent::Click => self.click(),
            ButtonEvent::Press => self.press(),
            ButtonEvent::Release => self.release(),
            ButtonEvent::Focus => self.focus(),
            ButtonEvent::Blur => self.blur(),
            ButtonEvent::KeyDown(key) => self.handle_key_down(&key),
            ButtonEvent::KeyUp(key) => self.handle_key_up(&key),
        }
    }
}

impl Focusable for Button {
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

impl KeyboardInteractable for Button {
    fn handle_key_down(&mut self, key: &str) {
        if self.state.disabled || self.state.loading {
            return;
        }

        match key {
            " " | "Enter" => {
                self.press();
            }
            _ => {}
        }
    }

    fn handle_key_up(&mut self, key: &str) {
        if self.state.disabled || self.state.loading {
            return;
        }

        match key {
            " " | "Enter" => {
                self.release();
            }
            _ => {}
        }
    }
}

/// Builder for Button component
pub struct ButtonBuilder {
    text: String,
    variant: ButtonVariant,
    size: ButtonSize,
    disabled: bool,
    loading: bool,
    on_click: Option<Box<dyn Fn()>>,
}

impl ButtonBuilder {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            variant: ButtonVariant::Primary,
            size: ButtonSize::Medium,
            disabled: false,
            loading: false,
            on_click: None,
        }
    }

    pub fn text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn loading(mut self, loading: bool) -> Self {
        self.loading = loading;
        self
    }

    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.on_click = Some(Box::new(f));
        self
    }

    pub fn build(self) -> Button {
        Button {
            state: ButtonState {
                disabled: self.disabled,
                loading: self.loading,
                focused: false,
                pressed: false,
            },
            text: self.text,
            variant: self.variant,
            size: self.size,
            on_click: self.on_click,
        }
    }
}

impl Default for ButtonBuilder {
    fn default() -> Self {
        Self::new()
    }
}
