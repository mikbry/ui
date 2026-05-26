//! Web renderer — walks an [`mkui_runtime::AppTree`] and produces DOM.
//!
//! Sprint 4: the renderer consumes the runtime tree directly. Built-in node
//! kinds (`View`, `Text`, `Button`) render through fixed paths inside this
//! module; [`NodeKind::Custom`] dispatches through [`WebRendererRegistry`]
//! keyed by `type_name`, so downstream crates can plug in `Card`, `Tabs`,
//! `Separator`, … without forking `mkui-web`.
//!
//! ## Adding a custom component
//!
//! ```ignore
//! use mkui_web::render::{CustomWebRenderable, WebRendererRegistry};
//! use mkui_runtime::AppTree;
//! use wasm_bindgen::JsValue;
//! use web_sys::{Document, Element};
//!
//! struct Card;
//! impl CustomWebRenderable for Card {
//!     fn type_name(&self) -> &str { "card" }
//!     fn render_custom(
//!         &self,
//!         document: &Document,
//!         props: &serde_json::Value,
//!         _registry: &WebRendererRegistry,
//!         _tree: &AppTree,
//!     ) -> Result<Element, JsValue> {
//!         let el = document.create_element("section")?;
//!         if let Some(title) = props.get("title").and_then(|v| v.as_str()) {
//!             el.set_text_content(Some(title));
//!         }
//!         Ok(el)
//!     }
//! }
//! ```

use std::collections::HashMap;

use mkui_runtime::{AppTree, ButtonVariant, NodeId, NodeKind};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element};

/// Backend-specific render trait for custom (`NodeKind::Custom`) node
/// types. Built-in `View` / `Text` / `Button` rendering is hard-wired
/// inside [`render_tree`]; only extension types go through this trait.
pub trait CustomWebRenderable: 'static {
    fn type_name(&self) -> &str;
    fn render_custom(
        &self,
        document: &Document,
        props: &serde_json::Value,
        registry: &WebRendererRegistry,
        tree: &AppTree,
    ) -> Result<Element, JsValue>;
}

type CustomRenderFn = Box<
    dyn Fn(
        &Document,
        &serde_json::Value,
        &WebRendererRegistry,
        &AppTree,
    ) -> Result<Element, JsValue>,
>;

/// Registry of custom-component handlers keyed by `type_name`.
///
/// Built-in node kinds bypass this registry; only `NodeKind::Custom` nodes
/// look up their renderer here. Unknown types either hit the configured
/// fallback or surface a `JsValue` error.
pub struct WebRendererRegistry {
    custom: HashMap<String, CustomRenderFn>,
    fallback: Option<CustomRenderFn>,
}

impl WebRendererRegistry {
    pub fn new() -> Self {
        Self {
            custom: HashMap::new(),
            fallback: None,
        }
    }

    /// Equivalent to `new()` — kept for symmetry with the v0.4.x API.
    /// Sprint 4 has no built-in custom components, only the three runtime
    /// node kinds (`View` / `Text` / `Button`) which render directly.
    pub fn with_defaults() -> Self {
        Self::new()
    }

    /// Register a custom renderable.
    pub fn register<T: CustomWebRenderable>(&mut self, component: T) -> &mut Self {
        let type_name = component.type_name().to_string();
        self.custom.insert(
            type_name,
            Box::new(move |document, props, registry, tree| {
                component.render_custom(document, props, registry, tree)
            }),
        );
        self
    }

    /// Install a deliberate fallback handler for `NodeKind::Custom` nodes
    /// whose `type_name` is not registered.
    pub fn set_fallback<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(
                &Document,
                &serde_json::Value,
                &WebRendererRegistry,
                &AppTree,
            ) -> Result<Element, JsValue>
            + 'static,
    {
        self.fallback = Some(Box::new(f));
        self
    }

    pub fn has_renderer_for(&self, type_name: &str) -> bool {
        self.custom.contains_key(type_name)
    }

    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    fn render_custom_node(
        &self,
        document: &Document,
        type_name: &str,
        props: &serde_json::Value,
        tree: &AppTree,
    ) -> Result<Element, JsValue> {
        if let Some(handler) = self.custom.get(type_name) {
            return handler(document, props, self, tree);
        }
        if let Some(fallback) = &self.fallback {
            return fallback(document, props, self, tree);
        }
        let msg = format!(
            "mkui-web: no renderer registered for custom node type {type_name:?}. \
             Register a CustomWebRenderable via Mkui::register or install a fallback."
        );
        debug_assert!(false, "{msg}");
        Err(JsValue::from_str(&msg))
    }
}

impl Default for WebRendererRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Render the [`AppTree`] under `document`, returning the wrapper element
/// containing all top-level children of the runtime root.
pub fn render_tree(
    tree: &AppTree,
    document: &Document,
    registry: &WebRendererRegistry,
    wrapper_class: &str,
) -> Result<Element, JsValue> {
    let wrapper = document.create_element("div")?;
    if !wrapper_class.is_empty() {
        wrapper.set_class_name(wrapper_class);
    }
    let root = tree
        .get(tree.root())
        .ok_or_else(|| JsValue::from_str("AppTree root is missing"))?;
    for child_id in &root.children {
        let element = render_node(tree, *child_id, document, registry)?;
        wrapper.append_child(&element)?;
    }
    Ok(wrapper)
}

fn render_node(
    tree: &AppTree,
    id: NodeId,
    document: &Document,
    registry: &WebRendererRegistry,
) -> Result<Element, JsValue> {
    let node = tree
        .get(id)
        .ok_or_else(|| JsValue::from_str("stale NodeId encountered during render"))?;
    let element = match &node.kind {
        NodeKind::Root => return Err(JsValue::from_str("Root cannot be rendered as a child")),
        NodeKind::View(_) => {
            let el = document.create_element("div")?;
            if !node.class.raw().is_empty() {
                el.set_class_name(node.class.raw());
            }
            for child_id in &node.children {
                let child = render_node(tree, *child_id, document, registry)?;
                el.append_child(&child)?;
            }
            el
        }
        NodeKind::Text(t) => {
            let el = document.create_element("p")?;
            if !node.class.raw().is_empty() {
                el.set_class_name(node.class.raw());
            }
            el.set_text_content(Some(&t.content));
            // Children of a Text node are unusual but the schema permits
            // them — emit them so the DOM still mirrors the tree.
            for child_id in &node.children {
                let child = render_node(tree, *child_id, document, registry)?;
                el.append_child(&child)?;
            }
            el
        }
        NodeKind::Button(b) => render_button(tree, node, b, document)?,
        NodeKind::Custom { type_name, props } => {
            // Custom renderers are responsible for their own children; we
            // do not recurse automatically because a Custom may want to
            // inject DOM around or instead of the children.
            registry.render_custom_node(document, type_name, props, tree)?
        }
    };
    Ok(element)
}

fn render_button(
    tree: &AppTree,
    node: &mkui_runtime::Node,
    b: &mkui_runtime::ButtonProps,
    document: &Document,
) -> Result<Element, JsValue> {
    let button = document
        .create_element("button")?
        .dyn_into::<web_sys::HtmlButtonElement>()?;
    button.set_type("button");
    button.set_text_content(Some(&b.label));

    let variant_class = match b.variant {
        ButtonVariant::Primary => "btn btn-primary",
        ButtonVariant::Secondary => "btn btn-secondary",
        ButtonVariant::Destructive => "btn btn-destructive",
        ButtonVariant::Outline => "btn btn-outline",
        ButtonVariant::Ghost => "btn btn-ghost",
        ButtonVariant::Link => "btn btn-link",
        // ButtonVariant is `#[non_exhaustive]`; fall back to primary.
        _ => "btn btn-primary",
    };
    let class = if node.class.raw().is_empty() {
        variant_class.to_string()
    } else {
        format!("{} {}", variant_class, node.class.raw())
    };
    button.set_class_name(&class);

    if let Some(action_id) = b.on_press {
        // Capture the action id; firing routes through the tree's action
        // registry from inside the renderer's owning Mkui state. The
        // closure path goes through a thread-local pointer because
        // wasm_bindgen closures must be `'static + FnMut(JsValue)`.
        //
        // For Sprint 4 we wire the closure to look the action up by id
        // via the global tree pointer that `high_level::Mkui::run` installs
        // before mounting; see `crate::high_level::with_tree`.
        let action_idx = action_id.index();
        let action_gen = action_id.generation();
        let onclick = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            crate::high_level::fire_action_global(action_idx, action_gen);
        }) as Box<dyn FnMut(_)>);
        button
            .dyn_ref::<web_sys::HtmlElement>()
            .unwrap()
            .set_onclick(Some(onclick.as_ref().unchecked_ref()));
        onclick.forget();
    }

    let _ = tree; // children currently unused on Button
    Ok(button.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_starts_empty_by_default() {
        let registry = WebRendererRegistry::with_defaults();
        assert!(!registry.has_renderer_for("anything"));
    }

    #[test]
    fn fallback_hook_is_installed_when_set() {
        let mut registry = WebRendererRegistry::new();
        assert!(!registry.has_fallback());
        registry.set_fallback(|_document, _props, _registry, _tree| {
            Err(JsValue::from_str("unsupported"))
        });
        assert!(registry.has_fallback());
    }
}
