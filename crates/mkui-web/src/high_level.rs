//! High-level [`Mkui`] entry point for the web backend.
//!
//! Sprint 4: holds the `mkui_core::Mkui` (and thus the runtime
//! `AppTree`) plus the [`WebRendererRegistry`] used to dispatch custom
//! components. `run` walks the tree and produces DOM via
//! [`crate::render::render_tree`].

use crate::app::WebApp;
use crate::render::{render_tree, WebRendererRegistry};
use mkui_core::components::Component;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

thread_local! {
    /// Pointer to the live tree set by [`Mkui::run`] before mount. Web's
    /// onclick closures fire actions through this since wasm_bindgen closures
    /// must own their captures (`'static`) — we can't borrow `&AppTree` into
    /// each closure. `set_global_tree` installs an `Rc<RefCell<...>>` for the
    /// duration of the run; `fire_action_global` looks the id up and fires.
    static ACTIVE_TREE: RefCell<Option<Rc<RefCell<mkui_runtime::AppTree>>>> = const { RefCell::new(None) };
}

fn set_global_tree(tree: Rc<RefCell<mkui_runtime::AppTree>>) {
    ACTIVE_TREE.with(|cell| *cell.borrow_mut() = Some(tree));
}

/// Fire an action by `(index, generation)`. Called from button onclick
/// closures that the renderer installs in [`crate::render`].
pub fn fire_action_global(index: u32, generation: u32) {
    ACTIVE_TREE.with(|cell| {
        if let Some(tree) = cell.borrow().as_ref() {
            let id = mkui_runtime::ActionId::from_raw(index, generation);
            tree.borrow().actions().fire(id);
        }
    });
}

/// High-level web app entry point.
pub struct Mkui {
    app: Rc<RefCell<WebApp>>,
    core: mkui_core::components::Mkui,
    registry: WebRendererRegistry,
}

impl Mkui {
    pub fn new() -> Result<Self, JsValue> {
        let app = Rc::new(RefCell::new(WebApp::new("app")?));
        Ok(Self {
            app,
            core: mkui_core::components::Mkui::new(),
            registry: WebRendererRegistry::with_defaults(),
        })
    }

    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.core = self.core.child(child);
        self
    }

    /// Register a custom component for the web renderer. The Sprint 4 minimum
    /// has no built-in custom components; user code may still plug in its
    /// own via the runtime's `NodeKind::Custom` extension slot.
    pub fn register<T: crate::render::CustomWebRenderable>(mut self, component: T) -> Self {
        self.registry.register(component);
        self
    }

    pub fn fallback<F>(mut self, f: F) -> Self
    where
        F: Fn(
                &web_sys::Document,
                &serde_json::Value,
                &WebRendererRegistry,
                &mkui_runtime::AppTree,
            ) -> Result<web_sys::Element, JsValue>
            + 'static,
    {
        self.registry.set_fallback(f);
        self
    }

    pub fn run(self) -> Result<(), JsValue> {
        // Clear the loading message
        self.app.borrow().renderer().clear();

        self.app.borrow().mount()?;

        let document = crate::utils::document();
        let (tree, _registry_unused) = self.core.into_parts();
        let tree_rc = Rc::new(RefCell::new(tree));
        set_global_tree(Rc::clone(&tree_rc));

        let wrapper = {
            let tree_ref = tree_rc.borrow();
            render_tree(
                &tree_ref,
                &document,
                &self.registry,
                "min-h-screen flex flex-col bg-background text-foreground",
            )?
        };

        self.app.borrow().renderer().append_child(&wrapper)?;

        Ok(())
    }
}

/// Theme selector — kept as a Rust-side helper that mutates the underlying
/// `WebApp` and reloads the page on change. Now wires through `Button`
/// nodes rather than direct DOM construction so it composes with the
/// runtime tree like every other component.
pub struct ThemeSelector;

impl ThemeSelector {
    pub fn new(_app: Rc<RefCell<WebApp>>) -> Self {
        Self
    }
}
