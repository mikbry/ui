//! Smoke test for the extensible web renderer contract.
//!
//! Sprint 4 reshape: extension components now live in the runtime as
//! `NodeKind::Custom { type_name, props }` and the web registry dispatches
//! by `type_name`. The downstream component implements
//! [`CustomWebRenderable`] and registers itself. Built-in `View` / `Text` /
//! `Button` rendering bypasses the registry entirely.

use mkui_runtime::AppTree;
use mkui_web::render::{CustomWebRenderable, WebRendererRegistry};
use serde_json::Value;
use wasm_bindgen::JsValue;
use web_sys::{Document, Element};

/// A component owned by an imaginary downstream crate.
struct ProductCard;

impl CustomWebRenderable for ProductCard {
    fn type_name(&self) -> &str {
        "product_card"
    }

    fn render_custom(
        &self,
        _document: &Document,
        _props: &Value,
        _registry: &WebRendererRegistry,
        _tree: &AppTree,
    ) -> Result<Element, JsValue> {
        // The host-side test never hits this branch; the wasm showcase does.
        unreachable!("render_custom is only callable in a wasm context with a real Document")
    }
}

#[test]
fn user_defined_component_plugs_into_registry_by_type_name() {
    let mut registry = WebRendererRegistry::with_defaults();
    assert!(!registry.has_renderer_for("product_card"));
    registry.register(ProductCard);
    assert!(registry.has_renderer_for("product_card"));
}

#[test]
fn fallback_hook_is_a_deliberate_opt_in() {
    let mut registry = WebRendererRegistry::new();
    assert!(!registry.has_fallback());
    registry.set_fallback(|_doc, _props, _registry, _tree| Err(JsValue::from_str("unsupported")));
    assert!(registry.has_fallback());
}
