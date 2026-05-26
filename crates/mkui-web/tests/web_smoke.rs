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
