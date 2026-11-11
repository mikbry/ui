//! Cross-platform component model.
//!
//! `mkui-core` keeps a thin ergonomic builder layer (`View` / `Text` /
//! `Button`) so user code reads naturally:
//!
//! ```ignore
//! Mkui::new().child(
//!     View::new()
//!         .class("flex")
//!         .child(Text::new("hello").variant(TextVariant::Heading1))
//!         .child(Button::new("ok").on_press(|| println!("pressed"))),
//! );
//! ```
//!
//! Under the hood, `Mkui::child` *lowers* the builder values into an
//! [`mkui_runtime::AppTree`] via the [`LoweringRegistry`]. After lowering,
//! every backend walks the same `AppTree` (web / console / wgpu / C / Python)
//! and the parity gate (issue #51 §2) holds.
//!
//! ## Why both builders and the AppTree
//!
//! The Rust public API was load-bearing for v0.4.1 consumers — notably
//! `examples/showcase-common/src/lib.rs`, which must compile byte-unchanged
//! per acceptance criterion #9. The runtime tree is the *storage* layer;
//! the builders are *sugar on handles* (Codex Q2). One model, two
//! ergonomic frontends (Rust + FFI).
//!
//! ## Extending the registry
//!
//! Custom component types implement [`LowerToTree`] and register through
//! [`LoweringRegistry::register`]. Sprint 4 ships a `TestWidget` extension
//! proof in the parity test suite (`crates/mkui-runtime/tests/parity.rs`).
//! Real shadcn-parity components (Separator, Tabs, …) are deferred to
//! Sprint 6+ per the issue's "Out of scope" section.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use mkui_runtime::{
    ActionId, AppTree, ButtonVariant, ClassParseError, NodeId, StyleClass, TextVariant,
};
use serde_json::json;

/// Marker trait for every renderable component.
///
/// `Component` is `Any` so the [`LoweringRegistry`] can dispatch by
/// [`TypeId`]. User-defined components implement
/// `Component` (the marker) plus [`LowerToTree`] (the lowering) and
/// register the type in the registry.
pub trait Component: Any {
    /// Optional stable identifier used by backends for diffing or focus
    /// tracking. The default implementation returns `None`.
    fn id(&self) -> Option<&str> {
        None
    }
}

/// Lowering trait — converts a builder value into one or more nodes in an
/// [`AppTree`] and returns the id of the inserted root for that builder.
///
/// Implementations take `&mut self` so internal `Vec<Box<dyn Component>>`
/// children can be moved out via `std::mem::take` without consuming the box
/// — this avoids the trait-object upcast (`Box<dyn Component>` →
/// `Box<dyn Any>`) which is unstable on the workspace MSRV (Rust 1.84;
/// stabilised in 1.86).
///
/// Downstream components implement this on their own types and register
/// them through a [`LoweringRegistry`].
pub trait LowerToTree: Component {
    /// Lower `self` under `parent`, returning the id of the inserted root.
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError>;
}

type LowerFn = Box<
    dyn Fn(
        &mut dyn Component,
        &mut AppTree,
        NodeId,
        &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError>,
>;

/// Registry of [`LowerToTree`] handlers keyed by [`TypeId`]. Mirrors the
/// `WebRendererRegistry` extension pattern at the lowering boundary so the
/// same shape works for every binding.
pub struct LoweringRegistry {
    handlers: HashMap<TypeId, LowerFn>,
}

impl LoweringRegistry {
    /// Empty registry. Prefer [`LoweringRegistry::with_defaults`].
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    /// Registry pre-populated with lowering for the built-in `mkui-core`
    /// component types ([`View`], [`Text`], [`Button`]).
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        reg.register::<View>();
        reg.register::<Text>();
        reg.register::<Button>();
        reg.register::<MenuList>();
        reg
    }

    /// Register `T` so the registry can lower it. Re-registering overwrites.
    pub fn register<T: LowerToTree + 'static>(&mut self) -> &mut Self {
        self.handlers.insert(
            TypeId::of::<T>(),
            Box::new(|comp, tree, parent, registry| {
                // Same downcast pattern `mkui-web`'s `WebRendererRegistry`
                // uses: cast `&mut dyn Component` to `&mut dyn Any` (works
                // because `Component: Any`) and then downcast to the
                // concrete type the handler was registered for.
                let any: &mut dyn Any = comp;
                let typed = any
                    .downcast_mut::<T>()
                    .expect("LoweringRegistry: TypeId matched but downcast failed");
                typed.lower_to_tree(tree, parent, registry)
            }),
        );
        self
    }

    pub fn has_lowering_for<T: 'static>(&self) -> bool {
        self.handlers.contains_key(&TypeId::of::<T>())
    }

    /// Lower a boxed builder value into `tree` under `parent`.
    ///
    /// The handler downcasts `&mut dyn Component` to the concrete type, so
    /// the box can move data out of internal fields (e.g. `View::children`)
    /// via `std::mem::take` rather than consuming the box itself.
    ///
    /// Errors:
    /// - The component's type was never registered → returns a
    ///   `ClassParseError::UnknownToken` carrying the missing type-id. We
    ///   surface as a class-parse-shaped error so callers have a single
    ///   error type to handle. Future sprints may add a richer `LowerError`
    ///   if extension volume grows.
    /// - The component's class string contains an unknown utility.
    pub fn lower_boxed(
        &self,
        mut boxed: Box<dyn Component>,
        tree: &mut AppTree,
        parent: NodeId,
    ) -> Result<NodeId, ClassParseError> {
        let any_ref: &dyn Any = &*boxed;
        let type_id = any_ref.type_id();
        if let Some(handler) = self.handlers.get(&type_id) {
            handler(&mut *boxed, tree, parent, self)
        } else {
            Err(ClassParseError::UnknownToken(format!(
                "<unregistered component type {type_id:?}>"
            )))
        }
    }
}

impl Default for LoweringRegistry {
    fn default() -> Self {
        Self::with_defaults()
    }
}

/// Top-level app container. Holds the [`AppTree`] and the
/// [`LoweringRegistry`] used to convert builder values into nodes.
pub struct Mkui {
    tree: AppTree,
    registry: LoweringRegistry,
}

impl Mkui {
    pub fn new() -> Self {
        Self {
            tree: AppTree::new(),
            registry: LoweringRegistry::with_defaults(),
        }
    }

    /// Build a `Mkui` around a pre-existing [`AppTree`]. Used by FFI
    /// bindings (`mkui-c`, `mkui-py`) that own their `AppTree` directly
    /// and need to hand it to a backend's `run` loop without losing it
    /// in the process.
    ///
    /// The lowering registry resets to defaults — any custom registrations
    /// the caller wants must be re-applied via [`Mkui::register`].
    pub fn with_tree(tree: AppTree) -> Self {
        Self {
            tree,
            registry: LoweringRegistry::with_defaults(),
        }
    }

    /// Append a child to this UI tree.
    ///
    /// `.child()` keeps the user-facing builder API infallible (matching the
    /// v0.4.1 surface), so a class-parse error is a panic rather than a
    /// `Result`. The panic is `#[track_caller]`, so the blame points at your
    /// `.child(...)` call site instead of mkui-core internals.
    ///
    /// # Panics
    ///
    /// Panics on a class-parse error (e.g. a typo in a Tailwind-like utility
    /// class). The panic message carries the offending token so the typo is
    /// obvious from the backtrace. For the fallible variant that surfaces
    /// [`ClassParseError`] instead of panicking, use [`Mkui::try_child`].
    ///
    /// # Examples
    ///
    /// ```
    /// use mkui_core::prelude::{Mkui, View};
    ///
    /// let app = Mkui::new().child(View::new().class("flex-col gap-4"));
    /// assert_eq!(app.children_len(), 1);
    /// ```
    #[track_caller]
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        let boxed: Box<dyn Component> = Box::new(child);
        let root = self.tree.root();
        if let Err(err) = self.registry.lower_boxed(boxed, &mut self.tree, root) {
            panic!("mkui-core: failed to append child — {err}");
        }
        self
    }

    /// Falliblevariant of [`Mkui::child`] that returns the parse error
    /// instead of panicking. Tests and FFI bridges that want to surface
    /// the error use this; the showcase API stays panic-on-bad-class.
    pub fn try_child(mut self, child: impl Component + 'static) -> Result<Self, ClassParseError> {
        let boxed: Box<dyn Component> = Box::new(child);
        let root = self.tree.root();
        self.registry.lower_boxed(boxed, &mut self.tree, root)?;
        Ok(self)
    }

    /// Register a custom component type with the lowering registry.
    pub fn register<T: LowerToTree + 'static>(mut self) -> Self {
        self.registry.register::<T>();
        self
    }

    /// Read access to the constructed tree. Backends consume this.
    pub fn tree(&self) -> &AppTree {
        &self.tree
    }

    /// Mutable tree access. Backends that need to fire actions or rebuild
    /// nodes after construction use this.
    pub fn tree_mut(&mut self) -> &mut AppTree {
        &mut self.tree
    }

    /// Read access to the lowering registry.
    pub fn registry(&self) -> &LoweringRegistry {
        &self.registry
    }

    /// Consume `self` and yield (tree, registry) — used by backend bridges
    /// that want to take ownership and append more nodes themselves.
    pub fn into_parts(self) -> (AppTree, LoweringRegistry) {
        (self.tree, self.registry)
    }

    /// Number of children directly under the root. Kept for backwards
    /// compatibility with tests that asserted on the old `.children().len()`
    /// shape; new code should walk `tree()` instead.
    pub fn children_len(&self) -> usize {
        self.tree
            .get(self.tree.root())
            .map(|n| n.children.len())
            .unwrap_or(0)
    }
}

impl Default for Mkui {
    fn default() -> Self {
        Self::new()
    }
}

// -------------------------------------------------------------------------
// View — flex container
// -------------------------------------------------------------------------

/// Cross-platform `View` container.
pub struct View {
    class: String,
    children: Vec<Box<dyn Component>>,
}

impl View {
    pub fn new() -> Self {
        Self {
            class: String::new(),
            children: Vec::new(),
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    /// Number of direct children. Replaces the legacy `&Vec<Box<dyn …>>`
    /// accessor — exposing the raw vec is no longer useful because the
    /// builder is consumed during lowering.
    pub fn children_len(&self) -> usize {
        self.children.len()
    }
}

impl Default for View {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for View {}

impl LowerToTree for View {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let class = std::mem::take(&mut self.class);
        let children = std::mem::take(&mut self.children);
        let style = StyleClass::from_str(class);
        // Validate eagerly so showcase typos surface at construction.
        let _ = style.parse()?;
        let id = tree.push_view_unchecked(parent, style);
        for child in children {
            registry.lower_boxed(child, tree, id)?;
        }
        Ok(id)
    }
}

// -------------------------------------------------------------------------
// Text — typographic node
// -------------------------------------------------------------------------

/// Cross-platform `Text` component.
pub struct Text {
    content: String,
    class: String,
    variant: TextVariant,
}

impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            class: String::new(),
            variant: TextVariant::Body,
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn variant(mut self, variant: TextVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn text_variant(&self) -> &TextVariant {
        &self.variant
    }
}

impl Component for Text {}

impl LowerToTree for Text {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        _registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let content = std::mem::take(&mut self.content);
        let class = std::mem::take(&mut self.class);
        let variant = self.variant;
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(tree.push_text_unchecked(parent, content, variant, style))
    }
}

// -------------------------------------------------------------------------
// Button — pressable
// -------------------------------------------------------------------------

/// Cross-platform `Button` component.
pub struct Button {
    content: String,
    class: String,
    variant: ButtonVariant,
    on_press: Option<Rc<dyn Fn()>>,
}

impl Button {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            class: String::new(),
            variant: ButtonVariant::Primary,
            on_press: None,
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// `on_press` accepts an `Fn()` for ergonomic parity with the v0.4.1 API.
    /// During lowering the closure is wrapped into an `FnMut(&mut RuntimeCtx)`
    /// that calls the user code and then marks the tree dirty — actions
    /// always emit `RequestRedraw` (issue #51 §7).
    pub fn on_press<F>(mut self, handler: F) -> Self
    where
        F: Fn() + 'static,
    {
        self.on_press = Some(Rc::new(handler));
        self
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn button_variant(&self) -> &ButtonVariant {
        &self.variant
    }

    pub fn on_press_handler(&self) -> &Option<Rc<dyn Fn()>> {
        &self.on_press
    }
}

impl Component for Button {}

impl LowerToTree for Button {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        _registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let content = std::mem::take(&mut self.content);
        let class = std::mem::take(&mut self.class);
        let variant = self.variant;
        let on_press = self.on_press.take();
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        let action_id: Option<ActionId> = on_press.map(|handler| {
            // Wrap the v0.4.1-style `Fn()` into the runtime's
            // `FnMut(&mut RuntimeCtx)` shape. Every action marks the tree
            // dirty + emits RequestRedraw — that's the contract from the
            // issue's §7. Renderers observe the signal on the next frame.
            tree.actions_mut().register_local(move |ctx| {
                handler();
                ctx.mark_dirty();
            })
        });
        Ok(tree.push_button_unchecked(parent, content, variant, style, action_id))
    }
}

// -------------------------------------------------------------------------
// MenuList — keyboard/gamepad-selectable list (#189, C4 of #173)
// -------------------------------------------------------------------------

/// Cross-platform `MenuList` component — a title-screen/pause-style list
/// with one current selection.
///
/// Unlike [`View`]/[`Text`]/[`Button`], `MenuList` lowers through the
/// [`NodeKind::Custom`](mkui_runtime::NodeKind::Custom) extension slot
/// (`type_name = "menu_list"`) rather than a dedicated `NodeKind` variant —
/// `NodeKind` is not `#[non_exhaustive]`, so a new built-in variant would
/// break every backend's exhaustive match (console, web, wgpu, C, Python,
/// the snapshot code). `Custom` is the designated extension point for
/// exactly this (see the `TestWidget` proof in
/// `crates/mkui-core/tests/parity.rs`); each backend decides for itself
/// whether/how to render a `"menu_list"` node (`mkui-console` does, as of
/// this issue — see `mkui_console::components::walk_tree`).
///
/// This is the tree-lowering builder; the keyboard/gamepad navigation
/// state machine lives in [`crate::headless::MenuList`] and is a separate
/// type, matching how this [`Button`] and [`crate::headless::Button`] are
/// two independent types for two independent jobs. `selected_index` here
/// is only the *construction-time* default — an interactive backend (e.g.
/// `mkui-console`) owns the live navigation cursor itself and uses this
/// only to seed where that cursor starts.
pub struct MenuList {
    items: Vec<String>,
    selected_index: Option<usize>,
    class: String,
    on_select: Option<Rc<dyn Fn(usize)>>,
}

impl MenuList {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            selected_index: None,
            class: String::new(),
            on_select: None,
        }
    }

    /// Append an item. The first item appended becomes the selection by
    /// default, so a freshly-built menu always has a visible selection.
    pub fn item(mut self, label: impl Into<String>) -> Self {
        self.items.push(label.into());
        if self.selected_index.is_none() {
            self.selected_index = Some(0);
        }
        self
    }

    /// Override the selection. Validated once, at lowering time, against
    /// the *final* item count — so this can be called before or after
    /// `.item(...)` calls, in either order. An index that ends up out of
    /// range is ignored, falling back to the same default `.item()`
    /// establishes (the first item, if any).
    pub fn selected_index(mut self, index: usize) -> Self {
        self.selected_index = Some(index);
        self
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    /// `on_select` fires with the activated item's index when a backend's
    /// confirm input (Enter/Space on keyboard, equivalently gamepad) lands
    /// on that item — the `MenuList` analogue of [`Button::on_press`].
    /// Lowering registers one [`ActionId`] per item (each closing over its
    /// own fixed index), the same "one id, one fixed outcome" shape every
    /// other action in this crate uses; a backend's render loop is what
    /// decides *which* item's action to fire based on its own live cursor.
    pub fn on_select<F>(mut self, handler: F) -> Self
    where
        F: Fn(usize) + 'static,
    {
        self.on_select = Some(Rc::new(handler));
        self
    }

    pub fn items(&self) -> &[String] {
        &self.items
    }

    pub fn selected(&self) -> Option<usize> {
        self.selected_index
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }
}

impl Default for MenuList {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for MenuList {}

impl LowerToTree for MenuList {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        _registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let items = std::mem::take(&mut self.items);
        let class = std::mem::take(&mut self.class);
        let on_select = self.on_select.take();

        let item_actions: Vec<Option<ActionId>> = items
            .iter()
            .enumerate()
            .map(|(index, _)| {
                on_select.as_ref().map(|handler| {
                    let handler = Rc::clone(handler);
                    // Every action marks dirty + emits RequestRedraw
                    // (issue #51 §7), matching Button::on_press above even
                    // though confirming a menu item doesn't itself change
                    // the tree's structure.
                    tree.actions_mut().register_local(move |ctx| {
                        handler(index);
                        ctx.mark_dirty();
                    })
                })
            })
            .collect();

        // Validated here (not in `selected_index()`) so that builder method
        // can be called in either order relative to `.item(...)`: an index
        // that is out of range against the *final* item count falls back
        // to the first item (matching what `.item()` defaults to), rather
        // than silently keeping a now-meaningless value.
        let selected_index = match self.selected_index {
            Some(index) if index < items.len() => Some(index),
            _ if items.is_empty() => None,
            _ => Some(0),
        };

        let props = json!({
            "items": items,
            "selected_index": selected_index,
            "item_actions": item_actions,
        });
        tree.push_custom(parent, "menu_list", props, class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_runtime::NodeKind;

    #[test]
    fn component_tree_is_constructible_without_any_backend() {
        let app = Mkui::new().child(
            View::new()
                .class("flex")
                .child(Text::new("hello"))
                .child(Button::new("ok").variant(ButtonVariant::Primary)),
        );

        // Root + outer View + Text + Button = 4 live nodes.
        assert_eq!(app.tree().len(), 4);
        assert_eq!(app.children_len(), 1);
    }

    #[test]
    fn lowering_preserves_class_and_children() {
        let app = Mkui::new().child(
            View::new()
                .class("flex items-center")
                .child(Text::new("a"))
                .child(Text::new("b")),
        );
        let root = app.tree().get(app.tree().root()).unwrap();
        assert_eq!(root.children.len(), 1);
        let view = app.tree().get(root.children[0]).unwrap();
        assert!(matches!(view.kind, NodeKind::View(_)));
        assert_eq!(view.class.raw(), "flex items-center");
        assert!(view.resolved.flex);
        assert!(view.resolved.items_center);
        assert_eq!(view.children.len(), 2);
    }

    #[test]
    fn button_on_press_registers_an_action_that_marks_dirty() {
        use std::cell::Cell;
        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);

        let app = Mkui::new().child(Button::new("ok").on_press(move || pressed_in.set(true)));

        // Walk to the button node + grab its action id.
        let root = app.tree().get(app.tree().root()).unwrap();
        let button = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Button(b) = &button.kind else {
            panic!("expected Button")
        };
        let action = b.on_press.expect("action must be registered");

        let ctx = app.tree().actions().fire(action);
        assert!(pressed.get(), "user closure must fire");
        assert!(ctx.is_dirty(), "every action must mark dirty (§7)");
    }

    #[test]
    fn try_child_surfaces_a_class_parse_error() {
        let result = Mkui::new().try_child(View::new().class("totally-bogus-utility"));
        let Err(err) = result else {
            panic!("try_child must reject unknown class")
        };
        assert!(matches!(err, ClassParseError::UnknownToken(_)));
    }

    #[test]
    #[should_panic(expected = "failed to append child")]
    fn child_panics_on_unknown_class() {
        Mkui::new().child(View::new().class("totally-bogus-utility"));
    }

    #[test]
    fn child_panic_message_names_the_offending_token() {
        // The sharpened diagnostic must surface the bogus class string itself,
        // not just a generic "lowering failed" line (issue #69).
        let result = std::panic::catch_unwind(|| {
            Mkui::new().child(View::new().class("flex-col totally-bogus-utility gap-4"));
        });
        let err = result.expect_err("child must panic on an unknown class token");
        let msg = err
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| err.downcast_ref::<&str>().copied())
            .expect("panic payload must be a string");
        assert!(
            msg.contains("totally-bogus-utility"),
            "panic message must name the offending token, got: {msg}"
        );
    }

    #[test]
    fn menu_list_lowers_to_a_custom_node_with_items_and_selection() {
        let app = Mkui::new().child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .selected_index(1),
        );

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { type_name, props } = &node.kind else {
            panic!("expected a Custom node")
        };
        assert_eq!(type_name, "menu_list");
        assert_eq!(
            props["items"],
            serde_json::json!(["New Game", "Continue", "Quit"])
        );
        assert_eq!(props["selected_index"], serde_json::json!(1));
    }

    #[test]
    fn menu_list_defaults_to_selecting_the_first_item() {
        let app = Mkui::new().child(MenuList::new().item("A").item("B"));

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { props, .. } = &node.kind else {
            panic!("expected a Custom node")
        };
        assert_eq!(props["selected_index"], serde_json::json!(0));
    }

    #[test]
    fn menu_list_out_of_range_selection_is_ignored() {
        let app = Mkui::new().child(MenuList::new().item("A").item("B").selected_index(5));

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { props, .. } = &node.kind else {
            panic!("expected a Custom node")
        };
        assert_eq!(
            props["selected_index"],
            serde_json::json!(0),
            "an out-of-range selected_index() call must not override the default"
        );
    }

    #[test]
    fn menu_list_on_select_registers_one_action_per_item_fired_with_its_own_index() {
        use std::cell::RefCell;

        let fired = Rc::new(RefCell::new(Vec::new()));
        let fired_in = Rc::clone(&fired);

        let app = Mkui::new().child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .on_select(move |index| fired_in.borrow_mut().push(index)),
        );

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { props, .. } = &node.kind else {
            panic!("expected a Custom node")
        };
        let item_actions = props["item_actions"]
            .as_array()
            .expect("item_actions must be an array");
        assert_eq!(item_actions.len(), 3, "one action id per item");
        assert!(
            item_actions.iter().all(|v| !v.is_null()),
            "every item must get an action id when on_select is set"
        );

        // Firing the action registered for item 1 ("Continue") must call the
        // handler with index 1 — regardless of which item a backend's own
        // live cursor currently has selected, matching the "one id, one
        // fixed outcome" contract documented on `MenuList::on_select`.
        let continue_action: mkui_runtime::ActionId =
            serde_json::from_value(item_actions[1].clone()).unwrap();
        app.tree().actions().fire(continue_action);
        assert_eq!(*fired.borrow(), vec![1]);

        let quit_action: mkui_runtime::ActionId =
            serde_json::from_value(item_actions[2].clone()).unwrap();
        app.tree().actions().fire(quit_action);
        assert_eq!(*fired.borrow(), vec![1, 2]);
    }

    #[test]
    fn menu_list_without_on_select_has_no_item_actions() {
        let app = Mkui::new().child(MenuList::new().item("A").item("B"));

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { props, .. } = &node.kind else {
            panic!("expected a Custom node")
        };
        let item_actions = props["item_actions"]
            .as_array()
            .expect("item_actions must be an array");
        assert!(item_actions.iter().all(|v| v.is_null()));
    }

    #[test]
    fn menu_list_selected_index_before_items_is_validated_at_lowering_time() {
        // `.selected_index()` is validated once, at `lower_to_tree` time,
        // against the *final* item count — not against however many items
        // happened to exist at the moment it was called. Calling it before
        // any `.item(...)` must not silently get dropped.
        let app = Mkui::new().child(
            MenuList::new()
                .selected_index(2)
                .item("New Game")
                .item("Continue")
                .item("Quit"),
        );

        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Custom { props, .. } = &node.kind else {
            panic!("expected a Custom node")
        };
        assert_eq!(props["selected_index"], serde_json::json!(2));
    }
}
