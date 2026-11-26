//! Smoke tests for the `mkui-web` backend contract.
//!
//! The DOM-touching paths only work in a real browser, but the contract
//! surface — the prelude re-exports + the `WebRendererRegistry` — is pure
//! Rust and can be checked natively. Backend-level construction (`Mkui::new`)
//! is browser-only; we exercise the lowering through `mkui_core::Mkui`
//! which is host-runnable.

use mkui_runtime::NodeKind;
use mkui_web::prelude::*;

#[test]
fn registry_starts_without_custom_renderers() {
    let registry = WebRendererRegistry::with_defaults();
    // Built-in node kinds bypass the registry; only Custom nodes look it up.
    assert!(!registry.has_renderer_for("anything"));
    assert!(!registry.has_fallback());
}

#[test]
fn nested_view_tree_lowers_into_runtime_with_correct_counts() {
    let core = mkui_core::components::Mkui::new()
        .child(Text::new("Title").variant(TextVariant::Heading1))
        .child(
            View::new()
                .child(Button::new("Primary").variant(ButtonVariant::Primary))
                .child(Button::new("Cancel").variant(ButtonVariant::Outline)),
        );

    let mut views = 0;
    let mut texts = 0;
    let mut buttons = 0;
    for node in core.tree().nodes() {
        match node.kind {
            NodeKind::View(_) => views += 1,
            NodeKind::Text(_) => texts += 1,
            NodeKind::Button(_) => buttons += 1,
            _ => {}
        }
    }
    assert_eq!(views, 1);
    assert_eq!(texts, 1);
    assert_eq!(buttons, 2);
}

#[test]
fn card_and_disabled_button_lower_into_runtime_correctly() {
    // #270 + #245: the Card promotion and the button-disabled fold-in,
    // exercised through the same lowering path `render_tree` consumes
    // (DOM construction itself needs a real browser — see the module doc).
    let core = mkui_core::components::Mkui::new().child(
        Card::new()
            .class("p-6")
            .child(Text::new("inside"))
            .child(Button::new("Disabled").disabled(true)),
    );

    let mut cards = 0;
    let mut disabled_buttons = 0;
    for node in core.tree().nodes() {
        match &node.kind {
            NodeKind::Card(_) => cards += 1,
            NodeKind::Button(b) if b.disabled => disabled_buttons += 1,
            _ => {}
        }
    }
    assert_eq!(cards, 1);
    assert_eq!(disabled_buttons, 1);
}

#[test]
fn text_field_lowers_into_runtime_with_its_props() {
    // #312: exercised through the same lowering path `render_tree`
    // consumes — DOM construction itself needs a real browser (see the
    // module doc).
    let core = mkui_core::components::Mkui::new().child(
        TextField::new("hi")
            .placeholder("Email")
            .disabled(false)
            .invalid(true),
    );

    let mut fields = 0;
    for node in core.tree().nodes() {
        if let NodeKind::TextField(f) = &node.kind {
            fields += 1;
            assert_eq!(f.value, "hi");
            assert_eq!(f.placeholder, "Email");
            assert!(!f.disabled);
            assert!(f.invalid);
        }
    }
    assert_eq!(fields, 1);
}
