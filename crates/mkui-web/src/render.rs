//! Web renderer extension contract.
//!
//! This module implements the boundary described in
//! [`mkui_core::components`]: components are plain values, the backend owns
//! the trait that turns them into DOM, and dispatch happens through a
//! registry keyed by [`TypeId`].
//!
//! ## Adding a custom component
//!
//! ```ignore
//! use mkui_core::components::Component;
//! use mkui_web::render::{WebRenderable, WebRendererRegistry};
//! use mkui_web::Mkui;
//! use wasm_bindgen::JsValue;
//! use web_sys::{Document, Element};
//!
//! struct Card { title: String }
//! impl Component for Card {}
//!
//! impl WebRenderable for Card {
//!     fn render_web(
//!         &self,
//!         document: &Document,
//!         _registry: &WebRendererRegistry,
//!     ) -> Result<Element, JsValue> {
//!         let el = document.create_element("section")?;
//!         el.set_text_content(Some(&self.title));
//!         Ok(el)
//!     }
//! }
//!
//! // At app construction:
//! //   Mkui::new()?.register::<Card>().child(Card { title: "hi".into() }).run()?;
//! ```
//!
//! ## Unsupported components
//!
//! If a component reaches the registry without a registered handler,
//! [`WebRendererRegistry::render`] panics in debug builds and returns an
//! error in release builds. To opt into a placeholder element or any other
//! deliberate fallback, call
//! [`WebRendererRegistry::set_fallback`](WebRendererRegistry::set_fallback).

use std::any::{Any, TypeId};
use std::collections::HashMap;

use mkui_core::components::{Button, Component, Text, View};
use wasm_bindgen::prelude::*;
use web_sys::{Document, Element};

/// Backend-specific render trait implemented by every component the web
/// backend can draw. Built-in `mkui-core` types are implemented inside this
/// crate; downstream crates implement it on their own component types and
/// register them through a [`WebRendererRegistry`].
pub trait WebRenderable: Component {
    /// Render this component into a DOM element. Implementations should use
    /// `registry` to render any child `dyn Component` values so that custom
    /// components nested under built-ins also dispatch correctly.
    fn render_web(
        &self,
        document: &Document,
        registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue>;
}

type WebRenderFn =
    Box<dyn Fn(&dyn Component, &Document, &WebRendererRegistry) -> Result<Element, JsValue>>;

/// Registry of [`WebRenderable`] handlers keyed by [`TypeId`].
///
/// The registry replaces the legacy hard-coded downcast list: any component
/// type registered here can render, and unknown types fail loudly (or hit a
/// deliberate fallback) instead of being silently swapped for a placeholder.
pub struct WebRendererRegistry {
    handlers: HashMap<TypeId, WebRenderFn>,
    fallback: Option<WebRenderFn>,
}

impl WebRendererRegistry {
    /// Create an empty registry. Prefer [`WebRendererRegistry::with_defaults`]
    /// unless you intentionally want to opt out of the built-in components.
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            fallback: None,
        }
    }

    /// Registry pre-populated with the built-in `mkui-core` components
    /// ([`View`], [`Text`], [`Button`]).
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register::<View>();
        reg.register::<Text>();
        reg.register::<Button>();
        reg
    }

    /// Register `T` so the registry can render it. Re-registering a type
    /// overwrites the previous handler.
    pub fn register<T: WebRenderable + 'static>(&mut self) -> &mut Self {
        self.handlers.insert(
            TypeId::of::<T>(),
            Box::new(|component, document, registry| {
                let typed = (component as &dyn Any)
                    .downcast_ref::<T>()
                    .expect("WebRendererRegistry: TypeId matched but downcast failed");
                typed.render_web(document, registry)
            }),
        );
        self
    }

    /// Install a deliberate fallback handler invoked when no renderer is
    /// registered for a component's type. Without a fallback, missing
    /// renderers panic in debug builds and surface a `JsValue` error in
    /// release.
    pub fn set_fallback<F>(&mut self, f: F) -> &mut Self
    where
        F: Fn(&dyn Component, &Document, &WebRendererRegistry) -> Result<Element, JsValue>
            + 'static,
    {
        self.fallback = Some(Box::new(f));
        self
    }

    /// `true` if the registry contains a handler for `component`'s concrete
    /// type. Useful for diagnostics and tests; `render` already handles
    /// missing handlers itself.
    pub fn has_renderer_for(&self, component: &dyn Component) -> bool {
        let any: &dyn Any = component;
        self.handlers.contains_key(&any.type_id())
    }

    /// `true` if a fallback hook has been installed via
    /// [`set_fallback`](Self::set_fallback).
    pub fn has_fallback(&self) -> bool {
        self.fallback.is_some()
    }

    /// Render `component` into a DOM element. Dispatches by `TypeId`, falls
    /// back to the configured fallback hook, and otherwise fails loudly.
    pub fn render(
        &self,
        component: &dyn Component,
        document: &Document,
    ) -> Result<Element, JsValue> {
        let any: &dyn Any = component;
        let type_id = any.type_id();

        if let Some(handler) = self.handlers.get(&type_id) {
            return handler(component, document, self);
        }

        if let Some(fallback) = &self.fallback {
            return fallback(component, document, self);
        }

        let msg = format!(
            "mkui-web: no renderer registered for component type {:?}. \
             Implement WebRenderable for the type and call \
             Mkui::register::<T>() (or set a fallback via Mkui::fallback / \
             WebRendererRegistry::set_fallback).",
            type_id
        );

        debug_assert!(false, "{}", msg);
        Err(JsValue::from_str(&msg))
    }
}

impl Default for WebRendererRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl WebRenderable for View {
    fn render_web(
        &self,
        document: &Document,
        registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue> {
        let element = document.create_element("div")?;
        element.set_class_name(self.class_name());

        for child in self.children() {
            let child_element = registry.render(child.as_ref(), document)?;
            element.append_child(&child_element)?;
        }

        Ok(element)
    }
}

impl WebRenderable for Text {
    fn render_web(
        &self,
        document: &Document,
        _registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue> {
        let element = document.create_element("p")?;
        element.set_class_name(self.class_name());
        element.set_text_content(Some(self.content()));
        Ok(element)
    }
}

impl WebRenderable for Button {
    fn render_web(
        &self,
        _document: &Document,
        _registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue> {
        use std::rc::Rc;

        let mut web_button = crate::components::WebButton::new(self.content())?
            .variant(self.button_variant().clone());

        web_button.attach_events()?;

        let element = web_button.element().clone();

        if !self.class_name().is_empty() {
            let current_classes = element.class_name();
            element.set_class_name(&format!("{} {}", current_classes, self.class_name()));
        }

        if let Some(handler) = self.on_press_handler() {
            let handler = Rc::clone(handler);
            let closure = Closure::wrap(Box::new(move |_event: web_sys::Event| {
                handler();
            }) as Box<dyn FnMut(_)>);

            wasm_bindgen::JsCast::dyn_ref::<web_sys::HtmlElement>(&element)
                .unwrap()
                .set_onclick(Some(closure.as_ref().unchecked_ref()));
            closure.forget();
        }

        Ok(element)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct CustomCard;
    impl Component for CustomCard {}

    impl WebRenderable for CustomCard {
        fn render_web(
            &self,
            _document: &Document,
            _registry: &WebRendererRegistry,
        ) -> Result<Element, JsValue> {
            // Not exercised: these tests run on the host target, which has
            // no DOM. Renderer wiring is verified via `has_renderer_for` and
            // the fallback path is exercised through `set_fallback` below.
            unreachable!("render_web should not be called by these tests")
        }
    }

    #[test]
    fn defaults_cover_builtin_components() {
        let registry = WebRendererRegistry::with_defaults();

        let view: Box<dyn Component> = Box::new(View::new());
        let text: Box<dyn Component> = Box::new(Text::new("hi"));
        let button: Box<dyn Component> = Box::new(Button::new("ok"));

        assert!(registry.has_renderer_for(view.as_ref()));
        assert!(registry.has_renderer_for(text.as_ref()));
        assert!(registry.has_renderer_for(button.as_ref()));
    }

    #[test]
    fn register_admits_a_custom_component_type() {
        let mut registry = WebRendererRegistry::with_defaults();
        let custom: Box<dyn Component> = Box::new(CustomCard);

        assert!(
            !registry.has_renderer_for(custom.as_ref()),
            "custom component must not be supported before registration"
        );

        registry.register::<CustomCard>();

        assert!(
            registry.has_renderer_for(custom.as_ref()),
            "custom component must be supported after registration"
        );
    }

    #[test]
    fn empty_registry_has_no_builtin_handlers() {
        let registry = WebRendererRegistry::new();
        let view: Box<dyn Component> = Box::new(View::new());
        assert!(!registry.has_renderer_for(view.as_ref()));
    }

    #[test]
    fn fallback_hook_is_installed_when_set() {
        // The fallback closure itself takes a `&Document`, which the host
        // target cannot construct, so this test only verifies that the
        // builder wires the hook into the registry. Actual dispatch is
        // covered in a wasm context by `examples/web-showcase`.
        let mut registry = WebRendererRegistry::new();
        assert!(!registry.has_fallback());

        registry
            .set_fallback(|_component, _document, _registry| Err(JsValue::from_str("unsupported")));

        assert!(registry.has_fallback());
    }
}
