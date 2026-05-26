//! Backend-agnostic smoke tests for the `mkui-core` component contract.
//!
//! These tests exist so the shared component model — used by every backend
//! crate (`mkui-web`, `mkui-console`, `mkui-native`, …) — catches basic
//! regressions in construction and traversal before they leak into a
//! downstream backend or example.
//!
//! Sprint 4 reshape: the contract is now "build a `Mkui`, walk the
//! `AppTree`". Backends consume the runtime tree (per ADR 0005), not a
//! `Vec<Box<dyn Component>>`. These tests assert the new shape; the
//! v0.4.x downcast-style traversal moved into `mkui-runtime`'s tree walker.

use std::cell::Cell;
use std::rc::Rc;

use mkui_core::prelude::*;
use mkui_runtime::NodeKind;

/// Walk an `AppTree` and count View / Text / Button nodes the way a
/// renderer would. Mirrors the walk pattern in `mkui-console` and
/// `mkui-web` post-runtime-rewire.
#[derive(Default)]
struct NodeCounts {
    views: usize,
    texts: usize,
    buttons: usize,
}

fn count_nodes(app: &Mkui) -> NodeCounts {
    let mut counts = NodeCounts::default();
    for node in app.tree().nodes() {
        match &node.kind {
            NodeKind::View(_) => counts.views += 1,
            NodeKind::Text(_) => counts.texts += 1,
            NodeKind::Button(_) => counts.buttons += 1,
            NodeKind::Root | NodeKind::Custom { .. } => {}
        }
    }
    counts
}

#[test]
fn deeply_nested_tree_is_traversable() {
    let app = Mkui::new()
        .child(
            View::new().child(
                View::new()
                    .child(Text::new("title"))
                    .child(View::new().child(Button::new("ok"))),
            ),
        )
        .child(Text::new("footer"));

    let counts = count_nodes(&app);
    assert_eq!(counts.views, 3, "expected 3 nested views");
    assert_eq!(counts.texts, 2, "expected title + footer");
    assert_eq!(counts.buttons, 1, "expected the single primary button");
}

#[test]
fn view_preserves_class_through_construction() {
    let app = Mkui::new().child(
        View::new()
            .class("p-6 gap-4 flex-col")
            .child(Text::new("a"))
            .child(Text::new("b")),
    );

    let root = app.tree().get(app.tree().root()).unwrap();
    let view = app.tree().get(root.children[0]).unwrap();
    assert_eq!(view.class.raw(), "p-6 gap-4 flex-col");
    assert_eq!(view.children.len(), 2);
}

#[test]
fn text_records_content_class_and_variant() {
    let app = Mkui::new().child(
        Text::new("hello")
            .class("text-2xl font-bold")
            .variant(TextVariant::Heading1),
    );

    let root = app.tree().get(app.tree().root()).unwrap();
    let text_id = root.children[0];
    let text = app.tree().get(text_id).unwrap();
    let NodeKind::Text(t) = &text.kind else {
        panic!("expected Text")
    };
    assert_eq!(t.content, "hello");
    assert_eq!(text.class.raw(), "text-2xl font-bold");
    assert_eq!(t.variant, TextVariant::Heading1);
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
        let app = Mkui::new().child(Button::new("ok").variant(variant));
        let root = app.tree().get(app.tree().root()).unwrap();
        let NodeKind::Button(b) = &app.tree().get(root.children[0]).unwrap().kind else {
            panic!("expected Button")
        };
        assert_eq!(b.label, "ok");
        assert_eq!(b.variant, variant);
        assert!(b.on_press.is_none(), "no handler attached");
    }
}

#[test]
fn button_handler_fires_through_action_registry() {
    let pressed = Rc::new(Cell::new(0u32));
    let pressed_in = Rc::clone(&pressed);

    let app = Mkui::new().child(Button::new("click").on_press(move || {
        pressed_in.set(pressed_in.get() + 1);
    }));

    let root = app.tree().get(app.tree().root()).unwrap();
    let NodeKind::Button(b) = &app.tree().get(root.children[0]).unwrap().kind else {
        panic!("expected Button")
    };
    let action = b.on_press.expect("handler should be registered");
    app.tree().actions().fire(action);
    app.tree().actions().fire(action);
    assert_eq!(pressed.get(), 2, "handler must fire twice");
}

#[test]
fn mkui_root_accepts_multiple_children() {
    let app = Mkui::new()
        .child(Text::new("a"))
        .child(Text::new("b"))
        .child(View::new().child(Button::new("go")));

    assert_eq!(app.children_len(), 3);
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
