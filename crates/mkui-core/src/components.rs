//! Cross-platform component model.
//!
//! Components in `mkui-core` describe *what* should appear on screen. They do
//! not know how to render themselves — each backend (`mkui-web`,
//! `mkui-console`, `mkui-native`, ...) is responsible for turning these
//! values into platform-specific output.
//!
//! The [`Component`] trait is intentionally minimal: it is a marker bound on
//! [`std::any::Any`] so backends can downcast to the concrete component types
//! ([`View`], [`Text`], [`Button`], or any user-defined component) and render
//! them in a backend-specific way.

use std::rc::Rc;

use crate::headless::{ButtonVariant, TextVariant};

/// Marker trait for every renderable component.
///
/// Implementations are simple value types that backends introspect via
/// [`std::any::Any`]. No rendering logic lives here — keeping
/// backend-specific code out of `mkui-core` is the whole point of the crate.
pub trait Component: std::any::Any {
    /// Optional stable identifier used by backends for diffing or focus
    /// tracking. The default implementation returns `None`.
    fn id(&self) -> Option<&str> {
        None
    }
}

/// Main app container — cross-platform.
pub struct Mkui {
    children: Vec<Box<dyn Component>>,
}

impl Mkui {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }

    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn children(&self) -> &Vec<Box<dyn Component>> {
        &self.children
    }
}

impl Default for Mkui {
    fn default() -> Self {
        Self::new()
    }
}

/// Cross-platform `View` container.
pub struct View {
    class: String,
    children: Vec<Box<dyn Component>>,
}

impl View {
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn children(&self) -> &Vec<Box<dyn Component>> {
        &self.children
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for View {}

/// Cross-platform `Text` component.
pub struct Text {
    content: String,
    class: String,
    variant: TextVariant,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            class: String::new(),
            variant: TextVariant::Body,
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn text_variant(&self) -> &TextVariant {
        &self.variant
    }
}

impl Component for Text {}

/// Cross-platform `Button` component.
pub struct Button {
    content: String,
    class: String,
    variant: ButtonVariant,
    on_press: Option<Rc<dyn Fn()>>,
}

impl Button {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            class: String::new(),
            variant: ButtonVariant::Primary,
            on_press: None,
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn on_press<F>(mut self, handler: F) -> Self
    where
        F: Fn() + 'static,
    {
        self.on_press = Some(Rc::new(handler));
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn button_variant(&self) -> &ButtonVariant {
        &self.variant
    }

    pub fn on_press_handler(&self) -> &Option<Rc<dyn Fn()>> {
        &self.on_press
    }
}

impl Component for Button {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_tree_is_constructible_without_any_backend() {
        let app = Mkui::new().child(
            View::new()
                .class("row")
                .child(Text::new("hello"))
                .child(Button::new("ok").variant(ButtonVariant::Primary)),
        );

        assert_eq!(app.children().len(), 1);
    }

    #[test]
    fn components_can_be_downcast_by_backends() {
        let view: Box<dyn Component> = Box::new(View::new().class("row"));
        let text: Box<dyn Component> = Box::new(Text::new("hello"));
        let button: Box<dyn Component> = Box::new(Button::new("ok"));

        assert!((view.as_ref() as &dyn std::any::Any).downcast_ref::<View>().is_some());
        assert!((text.as_ref() as &dyn std::any::Any).downcast_ref::<Text>().is_some());
        assert!((button.as_ref() as &dyn std::any::Any).downcast_ref::<Button>().is_some());
    }

    #[test]
    fn view_exposes_class_and_children() {
        let v = View::new().class("col gap-4").child(Text::new("a"));
        assert_eq!(v.class_name(), "col gap-4");
        assert_eq!(v.children().len(), 1);
    }

    #[test]
    fn button_press_handler_runs() {
        use std::cell::Cell;
        use std::rc::Rc;

        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);
        let button = Button::new("ok").on_press(move || pressed_in.set(true));

        let handler = button.on_press_handler().as_ref().expect("handler");
        handler();

        assert!(pressed.get());
    }
}
