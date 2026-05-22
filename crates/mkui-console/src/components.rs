//! Console-side component wrappers.
//!
//! Each backend turns the shared [`mkui_core::components`] tree into its own
//! drawable form. For the terminal that means a flat list of [`Line`]s plus
//! the [`ConsoleButton`] records (label, variant, optional handler) that
//! [`crate::high_level::Mkui`] navigates and renders.
//!
//! This module is the console counterpart of `mkui_web::components` and
//! `mkui_wgpu::components` — backend-specific component scaffolding that
//! sits above [`mkui_core`] and below the high-level [`crate::Mkui`]
//! orchestrator.
//!
//! Styling decisions come from the *typed* [`TextVariant`] /
//! [`ButtonVariant`] values on each component — never from sniffing
//! showcase-specific class strings.

use mkui_core::components::{Button, Component, Text, View};
use mkui_core::headless::{ButtonVariant, TextVariant};
use std::rc::Rc;

/// Console-side projection of a [`mkui_core::components::Button`].
///
/// Holds the label, the variant the styling needs, and the original
/// `on_press` handler so navigation can fire it without going back through
/// the original component tree.
#[derive(Clone)]
pub struct ConsoleButton {
    pub label: String,
    pub variant: ButtonVariant,
    pub on_press: Option<Rc<dyn Fn()>>,
}

/// One row in the flattened render plan produced from a component tree.
///
/// The console backend can't paint nested boxes, so it flattens the tree
/// into a sequence of lines. [`Line::Button`] references the matching
/// [`ConsoleButton`] in the parent app by index.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Heading(String),
    Body(String),
    Muted(String),
    Spacer,
    Button(usize),
}

/// Single-pass walk over the shared component tree.
///
/// Emits flat lines for the terminal renderer and collects interactive
/// buttons into the parallel array `Line::Button(index)` points into.
/// Text styling comes from the typed [`TextVariant`] on each
/// [`mkui_core::components::Text`] — the backend never inspects class
/// strings.
pub fn walk_component(
    component: &dyn Component,
    layout: &mut Vec<Line>,
    buttons: &mut Vec<ConsoleButton>,
) {
    let any = component as &dyn std::any::Any;

    if let Some(view) = any.downcast_ref::<View>() {
        for child in view.children() {
            walk_component(child.as_ref(), layout, buttons);
        }
        return;
    }

    if let Some(text) = any.downcast_ref::<Text>() {
        let content = text.content().to_string();
        let line = match text.text_variant() {
            TextVariant::Heading1 | TextVariant::Heading2 | TextVariant::Heading3 => {
                Line::Heading(content)
            }
            TextVariant::Caption | TextVariant::Label => Line::Muted(content),
            TextVariant::Body | TextVariant::Code => Line::Body(content),
            _ => Line::Body(content),
        };
        layout.push(line);
        layout.push(Line::Spacer);
        return;
    }

    if let Some(button) = any.downcast_ref::<Button>() {
        let index = buttons.len();
        buttons.push(ConsoleButton {
            label: button.content().to_string(),
            variant: button.button_variant().clone(),
            on_press: button.on_press_handler().clone(),
        });
        layout.push(Line::Button(index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn walk_component_preserves_on_press_handler() {
        let pressed = Rc::new(Cell::new(0u32));
        let pressed_in = Rc::clone(&pressed);

        let button = Button::new("ok").on_press(move || {
            pressed_in.set(pressed_in.get() + 1);
        });

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&button, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        let captured = buttons[0]
            .on_press
            .as_ref()
            .expect("handler must be captured, not dropped");
        captured();
        captured();

        assert_eq!(
            pressed.get(),
            2,
            "captured handler must be the same one the user supplied"
        );
    }

    #[test]
    fn walk_component_recurses_into_nested_views() {
        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);

        let tree = View::new()
            .class("row")
            .child(View::new().child(Button::new("deep").on_press(move || pressed_in.set(true))));

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&tree, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        buttons[0].on_press.as_ref().expect("handler")();
        assert!(pressed.get());
    }

    #[test]
    fn walk_component_handles_buttons_without_handlers() {
        let button = Button::new("no handler");
        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&button, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        assert!(buttons[0].on_press.is_none());
    }

    #[test]
    fn text_variant_drives_line_style_not_class_string() {
        // The backend must classify text by its typed `TextVariant`, not by
        // sniffing showcase-specific Tailwind class strings — that coupling
        // is what the "real component renderer" issue removes.
        let tree = View::new()
            .child(
                Text::new("title")
                    .variant(TextVariant::Heading1)
                    .class("text-4xl"),
            )
            .child(
                Text::new("note")
                    .variant(TextVariant::Caption)
                    .class("text-xs"),
            )
            .child(Text::new("body").class("text-base"));

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&tree, &mut layout, &mut buttons);

        let lines: Vec<&Line> = layout
            .iter()
            .filter(|l| !matches!(l, Line::Spacer))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(*lines[0], Line::Heading("title".into()));
        assert_eq!(*lines[1], Line::Muted("note".into()));
        assert_eq!(*lines[2], Line::Body("body".into()));
    }
}
