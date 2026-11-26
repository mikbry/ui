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

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mkui_runtime::{AppTree, ButtonVariant, NodeId, NodeKind, RuntimeSignal};
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
    owner: &Rc<RefCell<AppTree>>,
) -> Result<Element, JsValue> {
    let wrapper = document.create_element("div")?;
    if !wrapper_class.is_empty() {
        wrapper.set_class_name(wrapper_class);
    }
    let root = tree
        .get(tree.root())
        .ok_or_else(|| JsValue::from_str("AppTree root is missing"))?;
    for child_id in &root.children {
        let element = render_node(tree, *child_id, document, registry, owner)?;
        wrapper.append_child(&element)?;
    }
    Ok(wrapper)
}

fn render_node(
    tree: &AppTree,
    id: NodeId,
    document: &Document,
    registry: &WebRendererRegistry,
    owner: &Rc<RefCell<AppTree>>,
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
                let child = render_node(tree, *child_id, document, registry, owner)?;
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
                let child = render_node(tree, *child_id, document, registry, owner)?;
                el.append_child(&child)?;
            }
            el
        }
        NodeKind::Card(_) => {
            // Card chrome comes from the opt-in public DEFAULT_COMPONENT_CSS
            // recipe or application-owned `.card` styles. Preserve user classes.
            let el = document.create_element("div")?;
            let class = if node.class.raw().is_empty() {
                "card".to_string()
            } else {
                format!("card {}", node.class.raw())
            };
            el.set_class_name(&class);
            for child_id in &node.children {
                let child = render_node(tree, *child_id, document, registry, owner)?;
                el.append_child(&child)?;
            }
            el
        }
        NodeKind::Button(b) => render_button(tree, node, b, document, owner)?,
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
    owner: &Rc<RefCell<AppTree>>,
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

    // Native disabled semantics suppress activation. The public recipe keeps
    // pointer-events enabled so the painted button blocks controls beneath it;
    // no onclick handler is installed on disabled buttons.
    button.set_disabled(b.disabled);

    if !b.disabled {
        if let Some(action_id) = b.on_press {
            // Capture the action id *and* a clone of this button's own
            // mount's tree. wasm_bindgen closures must be `'static +
            // FnMut(JsValue)`, so a borrowed `&AppTree` can't cross into
            // the closure — but a thread-local pointer (the pre-#305
            // design) is scoped to the *process*, not the mount: a second
            // `Mkui::run()` in the same document overwrites it, so a stale
            // closure from the first mount resolves its action against the
            // second mount's registry whenever the two reuse the same raw
            // `ActionId`. Cloning `owner` here ties resolution to the exact
            // tree this button was rendered from, regardless of how many
            // other trees are mounted in the document.
            let action_idx = action_id.index();
            let action_gen = action_id.generation();
            let owner = Rc::clone(owner);
            let onclick = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                fire_action(&owner, action_idx, action_gen);
            }) as Box<dyn FnMut(_)>);
            button
                .dyn_ref::<web_sys::HtmlElement>()
                .unwrap()
                .set_onclick(Some(onclick.as_ref().unchecked_ref()));
            onclick.forget();
        }
    }

    let _ = tree; // children currently unused on Button
    Ok(button.into())
}

/// Fire `(index, generation)` against `owner` and propagate its
/// [`mkui_runtime::RuntimeCtx`] signals to that tree.
///
/// ## Listener ownership
///
/// Each button's onclick closure (installed in [`render_button`]) holds a
/// strong `Rc` clone of the tree it was rendered from — not a lookup
/// through shared/global state. That is what scopes dispatch to "the tree
/// that owns the mount": two `Mkui::run()` calls in the same document, even
/// if their trees allocated the same raw `ActionId`, each resolve through
/// their own closure's own `owner`, never each other's.
///
/// ## If the mount has been dropped
///
/// `render_button` always `Closure::forget()`s the onclick handler, which
/// deliberately leaks it for the life of the page — so as long as *any* of
/// a mount's buttons remain wired up, `owner` keeps that mount's tree
/// alive, and there is no path through this API where `owner` itself can
/// go stale. The staleness that can happen instead is per-action: `id`'s
/// generation stops matching the tree's slot once the action has been
/// removed (`ActionRegistry::remove`, e.g. via `AppTree::remove_node`) —
/// expected when a button's DOM node outlives its tree-side removal because
/// there is no live re-render loop yet (#296). `ActionRegistry::fire`
/// already treats a generation mismatch as "nothing to fire": it returns a
/// non-dirty `RuntimeCtx` with no emitted signals, so a stale click is a
/// silent no-op rather than firing a recycled slot's unrelated callback.
fn fire_action(owner: &Rc<RefCell<AppTree>>, index: u32, generation: u32) -> bool {
    let id = mkui_runtime::ActionId::from_raw(index, generation);
    // Fire on an immutable borrow (the registry only needs `&self`), then
    // re-borrow mutably to propagate the dirty bit if needed. Splitting the
    // borrows keeps `RefCell` happy without holding a mutable borrow across
    // `fire`.
    let mut ctx = owner.borrow().actions().fire(id);
    if ctx.is_dirty() {
        owner.borrow_mut().mark_dirty();
    }
    // Drain the signal list so a future re-entry doesn't see stale
    // emissions; today we only ack `RequestRedraw`, but the runtime is set
    // up for additional signals (text input, focus moves).
    ctx.drain_emitted()
        .iter()
        .any(|s| matches!(s, RuntimeSignal::RequestRedraw))
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
