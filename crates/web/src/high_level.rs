//! High-level [`Mkui`] entry point for the web backend.
//!
//! Sprint 4: holds the `mkui_core::Mkui` (and thus the runtime
//! `AppTree`) plus the [`WebRendererRegistry`] used to dispatch custom
//! components. `run` walks the tree and produces DOM via
//! [`crate::render::render_tree`].
//!
//! ## Action dispatch and the redraw flag
//!
//! `wasm_bindgen` closures must own their captures (`'static`), so we
//! can't borrow `&AppTree` into each button's onclick. `run()` builds the
//! tree, wraps it in an `Rc<RefCell<AppTree>>`, and hands that handle down
//! through [`crate::render::render_tree`]; each button's onclick closure
//! clones its own strong reference from it (see `crate::render::fire_action`
//! for the ownership/drop contract this gives us — in particular, why that
//! replaced an earlier `thread_local!`-based design that resolved every
//! mount's buttons against whichever tree was *most recently* mounted,
//! misrouting a first app's clicks once a second app mounted and happened
//! to reuse the same raw `ActionId` (#305)). **Crucially we capture the
//! returned `RuntimeCtx`** — `ActionRegistry::fire` produces `dirty` +
//! `RuntimeSignal::RequestRedraw` on the way out, and dropping that context
//! (the round-7 anti-pattern) means the renderer never knows when to
//! redraw. We mark the tree dirty and emit a debug log per `RequestRedraw`;
//! full re-render is a Sprint 5+ follow-up (the substrate carries the
//! signal correctly now).

use crate::app::WebApp;
use crate::render::{render_tree, WebRendererRegistry};
use mkui_core::components::Component;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;

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

        let wrapper = {
            let tree_ref = tree_rc.borrow();
            render_tree(
                &tree_ref,
                &document,
                &self.registry,
                "min-h-screen flex flex-col bg-background text-foreground",
                &tree_rc,
            )?
        };

        self.app.borrow().renderer().append_child(&wrapper)?;

        Ok(())
    }
}
