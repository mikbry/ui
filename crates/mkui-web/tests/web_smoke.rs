//! Smoke tests for the `mkui-web` backend contract.
//!
//! The DOM-touching paths in `mkui-web` only work in a real browser, but the
//! contract surface — the prelude re-exports, the [`WebRendererRegistry`]
//! dispatch, and the shared component tree — is pure Rust and can be checked
//! natively. Catching a drift here means the showcase will not silently lose
//! a `View` / `Text` / `Button` branch before anyone notices in the
//! browser-side tests.
//!
//! Actual `web_sys` rendering is covered by `examples/web-showcase` under a
//! wasm target; this file is what `cargo test --workspace` exercises.

use std::any::Any;

use mkui_web::prelude::*;

#[test]
fn registry_with_defaults_dispatches_every_core_component() {
    let registry = WebRendererRegistry::with_defaults();

    let view: Box<dyn Component> = Box::new(View::new().class("row"));
    let text: Box<dyn Component> = Box::new(Text::new("hello"));
    let button: Box<dyn Component> = Box::new(Button::new("ok"));

    assert!(registry.has_renderer_for(view.as_ref()));
    assert!(registry.has_renderer_for(text.as_ref()));
    assert!(registry.has_renderer_for(button.as_ref()));
}

#[test]
fn registry_does_not_silently_accept_unknown_components() {
    struct Custom;
    impl Component for Custom {}

    let registry = WebRendererRegistry::with_defaults();
    let custom: Box<dyn Component> = Box::new(Custom);

    assert!(
        !registry.has_renderer_for(custom.as_ref()),
        "unknown components must fail loudly through the registry, not be silently dropped"
    );
    assert!(!registry.has_fallback());
}

#[test]
fn nested_view_tree_preserves_text_and_button_children() {
    // Mirrors the structure of `examples/web-showcase` so the shared
    // component contract stays composable through the prelude even after
    // upstream refactors of the dispatch layer.
    let tree = View::new()
        .class("page")
        .child(Text::new("Title").variant(TextVariant::Heading1))
        .child(
            View::new()
                .class("actions")
                .child(Button::new("Primary").variant(ButtonVariant::Primary))
                .child(Button::new("Cancel").variant(ButtonVariant::Outline)),
        );

    assert_eq!(tree.children().len(), 2);

    let mut button_count = 0;
    let mut text_count = 0;
    fn walk(component: &dyn Component, buttons: &mut usize, texts: &mut usize) {
        let any: &dyn Any = component;
        if let Some(view) = any.downcast_ref::<View>() {
            for child in view.children() {
                walk(child.as_ref(), buttons, texts);
            }
        } else if any.downcast_ref::<Button>().is_some() {
            *buttons += 1;
        } else if any.downcast_ref::<Text>().is_some() {
            *texts += 1;
        }
    }
    walk(&tree as &dyn Component, &mut button_count, &mut text_count);
    assert_eq!(button_count, 2);
    assert_eq!(text_count, 1);
}
