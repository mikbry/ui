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
        NodeKind::TextField(f) => render_text_field(node, f, document, owner)?,
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

/// Convert a UTF-16 code-unit offset (the unit JS string indices and a
/// native `<input>`'s `selectionStart`/`selectionEnd` use) into a UTF-8
/// byte offset into `s` (#312 — the unit `mkui_runtime::TextFieldProps`
/// carries). Required because the two units diverge for any character
/// outside the Basic Latin range (accents, CJK, emoji): treating a
/// `selectionStart` value as a byte offset directly would misplace the
/// caret, or — worse — land mid-character and panic a later byte slice.
fn utf16_offset_to_byte_offset(s: &str, utf16_offset: usize) -> usize {
    let mut utf16_count = 0usize;
    for (byte_idx, ch) in s.char_indices() {
        if utf16_count >= utf16_offset {
            return byte_idx;
        }
        utf16_count += ch.len_utf16();
    }
    s.len()
}

/// Read `input`'s current value and selection and write them into
/// `owner`'s `TextField` node at `(index, generation)` (#312). Called from
/// a `TextField`'s `oninput`/`onblur` closures installed in
/// [`render_text_field`] — same per-mount `owner` clone [`fire_action`]
/// uses, for the same `'static` closure-capture reason (#305).
///
/// `selectionStart`/`selectionEnd` are always reported in UTF-16 order
/// (`start <= end`) regardless of which end the user is actually dragging
/// from; `selectionDirection` says which end is the live caret, so this
/// re-derives the directional `(caret, anchor)` pair
/// `mkui_runtime::TextFieldProps` carries, then converts both from UTF-16
/// code units to UTF-8 bytes via `utf16_offset_to_byte_offset`. Returns
/// `true` if anything in the tree actually changed.
fn sync_text_field(
    owner: &Rc<RefCell<AppTree>>,
    index: u32,
    generation: u32,
    input: &web_sys::HtmlInputElement,
) -> bool {
    let id = NodeId::from_raw(index, generation);
    let value = input.value();
    let start = input.selection_start().ok().flatten().unwrap_or(0) as usize;
    let end = input
        .selection_end()
        .ok()
        .flatten()
        .map(|v| v as usize)
        .unwrap_or(start);
    let backward = input.selection_direction().ok().flatten().as_deref() == Some("backward");
    let (caret_utf16, anchor_utf16) = if start == end {
        (start, None)
    } else if backward {
        (start, Some(end))
    } else {
        (end, Some(start))
    };
    let caret = utf16_offset_to_byte_offset(&value, caret_utf16);
    let anchor = anchor_utf16.map(|a| utf16_offset_to_byte_offset(&value, a));
    owner
        .borrow_mut()
        .set_text_field_value(id, &value, caret, anchor)
}

/// Focus the `TextField` node at `(index, generation)` as `owner`'s one
/// live [`mkui_runtime::AppTree::focused_text_field`] (#312). Called from
/// a `TextField`'s `onfocus` closure installed in [`render_text_field`].
fn focus_text_field(owner: &Rc<RefCell<AppTree>>, index: u32, generation: u32) -> bool {
    let id = NodeId::from_raw(index, generation);
    owner.borrow_mut().focus_text_field(id)
}

/// Blur whichever `TextField` is focused in `owner`, if any (#312). Called
/// from a `TextField`'s `onblur` closure — a singleton clear, same as
/// [`mkui_runtime::AppTree::blur_text_field`] itself, since only one field
/// is ever focused at a time.
fn blur_text_field(owner: &Rc<RefCell<AppTree>>) -> bool {
    owner.borrow_mut().blur_text_field()
}

/// Renders [`mkui_runtime::NodeKind::TextField`] (#312) through a real
/// `<input>` — a native element gets focus/caret/selection, keyboard
/// editing, and IME composition for free, which is exactly the "mkui-web
/// becomes a renderer of the contract" promise ADR 0015 describes for
/// `Card`, now for an interactive leaf instead of a static container.
/// `disabled`/`invalid` map to the real `disabled` attribute and the
/// shadcn `aria-invalid` convention; `oninput`/`onfocus`/`onblur` write the
/// live value/caret/selection (and the tree's singleton
/// `focused_text_field`) back into the exact tree this node was rendered
/// from — a cloned `owner` [`render_button`]'s `onclick` already uses,
/// not the pre-#305 thread-local dance, so two mounted trees that happen
/// to allocate the same raw `NodeId` never cross-write each other's state.
fn render_text_field(
    node: &mkui_runtime::Node,
    field: &mkui_runtime::TextFieldProps,
    document: &Document,
    owner: &Rc<RefCell<AppTree>>,
) -> Result<Element, JsValue> {
    let input = document
        .create_element("input")?
        .dyn_into::<web_sys::HtmlInputElement>()?;
    input.set_type("text");
    input.set_value(&field.value);
    input.set_placeholder(&field.placeholder);
    input.set_disabled(field.disabled);

    let class = if node.class.raw().is_empty() {
        "text-field".to_string()
    } else {
        format!("text-field {}", node.class.raw())
    };
    input.set_class_name(&class);

    let html: &web_sys::HtmlElement = input.dyn_ref().unwrap();
    if field.invalid {
        html.set_attribute("aria-invalid", "true")?;
    } else {
        html.remove_attribute("aria-invalid")?;
    }

    // Disabled native inputs never fire `input`/`focus` — no listeners to
    // install, same as `render_button` skipping `onclick` on a disabled
    // button.
    if !field.disabled {
        let node_idx = node.id.index();
        let node_gen = node.id.generation();

        let owner_for_input = Rc::clone(owner);
        let input_for_input = input.clone();
        let oninput = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            sync_text_field(&owner_for_input, node_idx, node_gen, &input_for_input);
        }) as Box<dyn FnMut(_)>);
        input.set_oninput(Some(oninput.as_ref().unchecked_ref()));
        oninput.forget();

        let owner_for_focus = Rc::clone(owner);
        let onfocus = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            focus_text_field(&owner_for_focus, node_idx, node_gen);
        }) as Box<dyn FnMut(_)>);
        input.set_onfocus(Some(onfocus.as_ref().unchecked_ref()));
        onfocus.forget();

        let owner_for_blur = Rc::clone(owner);
        let input_for_blur = input.clone();
        let onblur = Closure::wrap(Box::new(move |_event: web_sys::Event| {
            sync_text_field(&owner_for_blur, node_idx, node_gen, &input_for_blur);
            blur_text_field(&owner_for_blur);
        }) as Box<dyn FnMut(_)>);
        input.set_onblur(Some(onblur.as_ref().unchecked_ref()));
        onblur.forget();
    }

    Ok(input.into())
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

    // `utf16_offset_to_byte_offset` is pure Rust — no DOM, so it's one of
    // the few `TextField` (#312) code paths this crate can unit test
    // natively (see `tests/web_smoke.rs`'s module doc for why the
    // DOM-touching paths can't).

    #[test]
    fn ascii_utf16_and_byte_offsets_coincide() {
        assert_eq!(utf16_offset_to_byte_offset("hello", 3), 3);
    }

    #[test]
    fn multi_byte_char_before_the_offset_shifts_the_byte_offset_forward() {
        // "é" is 1 UTF-16 code unit but 2 UTF-8 bytes.
        assert_eq!(utf16_offset_to_byte_offset("é is nice", 1), 2);
        assert_eq!(utf16_offset_to_byte_offset("é is nice", 0), 0);
    }

    #[test]
    fn an_offset_past_the_end_clamps_to_the_strings_byte_length() {
        assert_eq!(utf16_offset_to_byte_offset("hi", 999), 2);
    }

    #[test]
    fn a_surrogate_pair_char_counts_as_two_utf16_units() {
        // "😀" (U+1F600) is one Unicode scalar, 2 UTF-16 code units, 4
        // UTF-8 bytes. An offset of 1 falls between the two UTF-16 units
        // of that single surrogate pair — not a boundary a byte offset
        // could ever land on — so it rounds forward to the next real char
        // boundary (4) rather than returning a mid-character byte index.
        let s = "😀x";
        assert_eq!(utf16_offset_to_byte_offset(s, 0), 0);
        assert_eq!(utf16_offset_to_byte_offset(s, 1), 4);
        assert_eq!(utf16_offset_to_byte_offset(s, 2), 4);
    }
}
