//! Backend-agnostic smoke tests for the `mkui-core` component contract.
//!
//! These tests exist so the shared component model — used by every backend
//! crate (`mkui-web`, `mkui-console`, `mkui-native`, …) — catches basic
//! regressions in construction and traversal before they leak into a
//! downstream backend or example.

use std::any::Any;
use std::cell::Cell;
use std::rc::Rc;

use mkui_core::prelude::*;

/// Walk a component tree and count `View` / `Text` / `Button` nodes the way a
/// backend would. Mirrors the downcast pattern in `mkui-web::high_level` and
/// `mkui-console::high_level`, so if the contract changes shape this helper
/// (and the assertions below) will surface the break.
#[derive(Default)]
struct NodeCounts {
    views: usize,
    texts: usize,
    buttons: usize,
}

fn count_nodes(component: &dyn Component, counts: &mut NodeCounts) {
    let any: &dyn Any = component;

    if let Some(view) = any.downcast_ref::<View>() {
        counts.views += 1;
        for child in view.children() {
            count_nodes(child.as_ref(), counts);
        }
        return;
    }

    if any.downcast_ref::<Text>().is_some() {
        counts.texts += 1;
        return;
    }

    if any.downcast_ref::<Button>().is_some() {
        counts.buttons += 1;
    }
}

#[test]
fn deeply_nested_tree_is_traversable() {
    let app = Mkui::new()
        .child(
            View::new().class("page").child(
                View::new()
                    .class("row")
                    .child(Text::new("title"))
                    .child(View::new().class("col").child(Button::new("ok"))),
            ),
        )
        .child(Text::new("footer"));

    let mut counts = NodeCounts::default();
    for child in app.children() {
        count_nodes(child.as_ref(), &mut counts);
    }

    assert_eq!(counts.views, 3, "expected 3 nested views");
    assert_eq!(counts.texts, 2, "expected title + footer");
    assert_eq!(counts.buttons, 1, "expected the single primary button");
}

#[test]
fn view_preserves_class_through_construction() {
    let v = View::new()
        .class("p-4 gap-2 flex-col")
        .child(Text::new("a"))
        .child(Text::new("b"));

    assert_eq!(v.class_name(), "p-4 gap-2 flex-col");
    assert_eq!(v.children().len(), 2);
}

#[test]
fn text_records_content_class_and_variant() {
    let t = Text::new("hello")
        .class("text-2xl font-bold")
        .variant(TextVariant::Heading1);

    assert_eq!(t.content(), "hello");
    assert_eq!(t.class_name(), "text-2xl font-bold");
    assert_eq!(t.text_variant(), &TextVariant::Heading1);
}

#[test]
fn button_round_trips_every_variant() {
    let variants = [
        ButtonVariant::Primary,
        ButtonVariant::Secondary,
        ButtonVariant::Destructive,
        ButtonVariant::Outline,
        ButtonVariant::Ghost,
        ButtonVariant::Link,
    ];

    for variant in variants {
        let button = Button::new("ok").variant(variant.clone());
        assert_eq!(button.content(), "ok");
        assert_eq!(button.button_variant(), &variant);
        assert!(button.on_press_handler().is_none());
    }
}

#[test]
fn button_handler_is_shared_via_rc_and_remains_callable() {
    let pressed = Rc::new(Cell::new(0u32));
    let pressed_in = Rc::clone(&pressed);

    let button = Button::new("click").on_press(move || {
        pressed_in.set(pressed_in.get() + 1);
    });

    let handler = button
        .on_press_handler()
        .as_ref()
        .expect("on_press should be stored")
        .clone();

    handler();
    handler();

    assert_eq!(pressed.get(), 2);
}

#[test]
fn mkui_root_accepts_multiple_children() {
    let app = Mkui::new()
        .child(Text::new("a"))
        .child(Text::new("b"))
        .child(View::new().class("group").child(Button::new("go")));

    assert_eq!(app.children().len(), 3);
}

#[test]
fn headless_button_state_machine_matches_contract() {
    let pressed = Rc::new(Cell::new(false));
    let pressed_in = Rc::clone(&pressed);

    let mut headless = HeadlessButton::builder()
        .text("ok")
        .on_click(move || pressed_in.set(true))
        .build();

    assert!(!headless.is_pressed());
    assert!(!headless.is_focused());

    headless.press();
    assert!(headless.is_pressed());
    headless.release();
    assert!(!headless.is_pressed());
    assert!(pressed.get(), "release must propagate to on_click");

    headless.set_disabled(true);
    pressed.set(false);
    headless.click();
    assert!(
        !pressed.get(),
        "disabled headless button must swallow clicks"
    );
}

#[test]
fn headless_toggle_only_fires_on_change() {
    let changes = Rc::new(Cell::new(0u32));
    let changes_in = Rc::clone(&changes);

    let mut toggle = Toggle::builder()
        .on_change(move |_| changes_in.set(changes_in.get() + 1))
        .build();

    assert!(!toggle.is_checked());
    toggle.set_checked(true);
    toggle.set_checked(true); // duplicate must not refire
    toggle.toggle();
    assert!(!toggle.is_checked());
    assert_eq!(changes.get(), 2);
}
