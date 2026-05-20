//! Smoke test for the extensible web renderer contract.
//!
//! Demonstrates that a component defined *outside* of `mkui-web` plugs into
//! the renderer through the public `WebRenderable` + `WebRendererRegistry`
//! API, satisfying acceptance criterion #4 from issue mikbry/ui#6.
//!
//! Actual DOM construction is exercised in `examples/web-showcase` under a
//! wasm target; this test runs on the host and only verifies that the
//! registry dispatches by `TypeId` exactly as documented.

use mkui_core::components::{Button, Component, Text, View};
use mkui_web::render::{WebRenderable, WebRendererRegistry};
use wasm_bindgen::JsValue;
use web_sys::{Document, Element};

/// A component owned by an imaginary downstream crate. The web backend has
/// no knowledge of this type — it only learns about it through
/// `WebRendererRegistry::register`.
struct ProductCard {
    title: String,
}

impl Component for ProductCard {}

impl WebRenderable for ProductCard {
    fn render_web(
        &self,
        _document: &Document,
        _registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue> {
        // The host-side test never hits this branch; the wasm showcase does.
        let _ = &self.title;
        unreachable!("render_web is only callable in a wasm context with a real Document")
    }
}

#[test]
fn user_defined_component_plugs_into_registry() {
    let mut registry = WebRendererRegistry::with_defaults();

    let card = ProductCard {
        title: "Beanie".into(),
    };
    let card_ref: &dyn Component = &card;

    assert!(
        !registry.has_renderer_for(card_ref),
        "registry must not silently accept unknown component types"
    );

    registry.register::<ProductCard>();

    assert!(
        registry.has_renderer_for(card_ref),
        "registering a WebRenderable type must make it dispatchable"
    );
}

#[test]
fn builtin_components_remain_supported_after_registering_custom_type() {
    let mut registry = WebRendererRegistry::with_defaults();
    registry.register::<ProductCard>();

    let view: Box<dyn Component> = Box::new(View::new());
    let text: Box<dyn Component> = Box::new(Text::new("hi"));
    let button: Box<dyn Component> = Box::new(Button::new("ok"));

    assert!(registry.has_renderer_for(view.as_ref()));
    assert!(registry.has_renderer_for(text.as_ref()));
    assert!(registry.has_renderer_for(button.as_ref()));
}

#[test]
fn nested_custom_component_is_visible_inside_a_view_tree() {
    let mut registry = WebRendererRegistry::with_defaults();
    registry.register::<ProductCard>();

    let tree = View::new().child(ProductCard {
        title: "Cap".into(),
    });

    let root: &dyn Component = &tree;
    assert!(registry.has_renderer_for(root));

    let view = (root as &dyn std::any::Any)
        .downcast_ref::<View>()
        .expect("root is a View");
    let child = view.children()[0].as_ref();

    assert!(
        registry.has_renderer_for(child),
        "the registry must dispatch nested custom components, not only top-level ones"
    );
}

#[test]
fn fallback_hook_is_a_deliberate_opt_in() {
    let mut registry = WebRendererRegistry::new();
    assert!(!registry.has_fallback());

    registry.set_fallback(|_component, _document, _registry| {
        Err(JsValue::from_str("unsupported in fallback"))
    });

    assert!(
        registry.has_fallback(),
        "set_fallback must persist for later dispatch"
    );
}
