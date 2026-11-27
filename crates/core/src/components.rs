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
//! proof in the parity test suite (`crates/runtime/tests/parity.rs`).
//! Real shadcn-parity components (Separator, Tabs, …) are deferred to
//! Sprint 6+ per the issue's "Out of scope" section.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::rc::Rc;

use mkui_runtime::{
    ActionId, AppTree, ButtonVariant, ClassParseError, NodeId, StyleClass, TextVariant,
};
use serde_json::json;

// Re-exported so a caller building an `Image` (e.g. `Image::new().fit(...)`)
// does not need a direct `mkui-runtime` dependency just to name these
// types — the same "mkui-core re-exports the runtime-level type its
// builder's public API needs" shape `headless::button`/`headless::text`
// already establish for `ButtonVariant`/`TextVariant`.
pub use mkui_runtime::{ImageFit, ImageSource, TextureHandle};

use crate::image::{decode_rgba8, sniff_format, ImageDecodeError};

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
        reg.register::<Card>();
        reg.register::<ScrollArea>();
        reg.register::<TextField>();
        reg.register::<Image>();
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
    ///
    /// An unrecognized utility in the component's class string is *not* an
    /// error (#269) — it is dropped (and logged) by `StyleClass::parse`,
    /// which never fails on it.
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
    /// v0.4.1 surface). Since #269, an unrecognized utility class in the
    /// child's class string is *not* a reason to panic — `StyleClass::parse`
    /// drops it (logging a warning) and every other class on the string
    /// still applies. The panic below is `#[track_caller]`, so on the rare
    /// genuine error it still blames your `.child(...)` call site instead of
    /// mkui-core internals.
    ///
    /// # Panics
    ///
    /// Panics only if `child`'s concrete type was never registered with this
    /// `Mkui`'s [`LoweringRegistry`] (see [`Mkui::register`]) — a
    /// programming error, not a class-string typo. For the fallible variant
    /// that surfaces [`ClassParseError`] instead of panicking, use
    /// [`Mkui::try_child`].
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

    /// Fallible variant of [`Mkui::child`] that returns an error instead of
    /// panicking. Since #269 the only remaining error source is
    /// `child`'s type not being registered with this `Mkui` — an
    /// unrecognized utility class is never an error (see [`Mkui::child`]).
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
    disabled: bool,
}

impl Button {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            class: String::new(),
            variant: ButtonVariant::Primary,
            on_press: None,
            disabled: false,
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

    /// Mark this button disabled (#245): backends suppress UI activation
    /// even if `on_press` was set. Wgpu and the public web CSS recipe dim
    /// its chrome and keep its painted surface blocking controls beneath it.
    /// Direct action-registry dispatch remains available.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
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

    pub fn is_disabled(&self) -> bool {
        self.disabled
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
        let disabled = self.disabled;
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
        Ok(tree.push_button_with_disabled_unchecked(
            parent, content, variant, style, action_id, disabled,
        ))
    }
}

// -------------------------------------------------------------------------
// Card — flex container with themed surface chrome (#270)
// -------------------------------------------------------------------------

/// Cross-platform `Card` container — a [`View`]-shaped flex container with
/// themed card chrome (fill, border, rounded corners) every backend paints
/// through its own native surface primitive. See ADR 0015 for the
/// promotion process this is the first proof of beyond `Button`.
pub struct Card {
    class: String,
    children: Vec<Box<dyn Component>>,
}

impl Card {
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

    pub fn children_len(&self) -> usize {
        self.children.len()
    }
}

impl Default for Card {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for Card {}

impl LowerToTree for Card {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let class = std::mem::take(&mut self.class);
        let children = std::mem::take(&mut self.children);
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        let id = tree.push_card_unchecked(parent, style);
        for child in children {
            registry.lower_boxed(child, tree, id)?;
        }
        Ok(id)
    }
}

// -------------------------------------------------------------------------
// ScrollArea — viewport-clipping container (#328, ADR 0015)
// -------------------------------------------------------------------------

/// Cross-platform `ScrollArea` container — a [`View`]-shaped flex container
/// whose children always lay out at their own natural height regardless of
/// this box's own height budget, so content taller than the box becomes
/// overflow for the box to show/scroll instead of something to compress.
/// Every backend renders it through its own native scroll primitive
/// (`mkui-web`: a real `overflow-y: auto`; `mkui-wgpu`: the existing
/// `components::scroll_area`/`scrollbar` chrome). See ADR 0015 for the
/// promotion process this follows (`Card`/`TextField`/`Image` before it).
pub struct ScrollArea {
    class: String,
    children: Vec<Box<dyn Component>>,
}

impl ScrollArea {
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

    pub fn children_len(&self) -> usize {
        self.children.len()
    }
}

impl Default for ScrollArea {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for ScrollArea {}

impl LowerToTree for ScrollArea {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let class = std::mem::take(&mut self.class);
        let children = std::mem::take(&mut self.children);
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        let id = tree.push_scroll_area_unchecked(parent, style);
        for child in children {
            registry.lower_boxed(child, tree, id)?;
        }
        Ok(id)
    }
}

// -------------------------------------------------------------------------
// TextField — single-line editable text input (#312, ADR 0015)
// -------------------------------------------------------------------------

/// Cross-platform `TextField` component — a single-line editable text
/// input, promoted from `mkui-wgpu`'s wgpu-local `text_field`/
/// `TextEditState` (#185) into the shared contract, following the same
/// promotion process `Card` proved (#270/#301, ADR 0015).
///
/// `on_change`/a payload-carrying callback is deliberately absent:
/// `mkui_runtime::ActionRegistry` actions are `FnMut(&mut RuntimeCtx)` with
/// no argument slot for the new value, so there's no existing mechanism to
/// hang one on without inventing a new, payload-carrying action shape.
/// The current value lives in the tree itself (`NodeKind::TextField`) —
/// any code holding a `NodeId` and the tree can read it back directly, the
/// same way an app reads back any other node's state. Scope decision:
/// deferred to a follow-up issue if a reactive callback turns out to be
/// needed.
pub struct TextField {
    value: String,
    placeholder: String,
    class: String,
    disabled: bool,
    invalid: bool,
}

impl TextField {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            placeholder: String::new(),
            class: String::new(),
            disabled: false,
            invalid: false,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    /// Mark this field disabled: backends suppress focus/edit (`mkui-web`
    /// sets the native `disabled` attribute; `mkui-wgpu`/`mkui-console`
    /// refuse to focus it and dim its chrome).
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Mark this field invalid — the shadcn `aria-invalid` convention
    /// (#312): `mkui-web` sets the real `aria-invalid` attribute;
    /// `mkui-wgpu`/`mkui-console` give it their own documented visual cue.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn placeholder_text(&self) -> &str {
        &self.placeholder
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    pub fn is_invalid(&self) -> bool {
        self.invalid
    }
}

impl Component for TextField {}

impl LowerToTree for TextField {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        _registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let value = std::mem::take(&mut self.value);
        let placeholder = std::mem::take(&mut self.placeholder);
        let class = std::mem::take(&mut self.class);
        let disabled = self.disabled;
        let invalid = self.invalid;
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(tree.push_text_field_unchecked(parent, value, placeholder, style, disabled, invalid))
    }
}

// -------------------------------------------------------------------------
// Image — GPU-texture or decoded-bytes source, contain/cover fit (#196)
// -------------------------------------------------------------------------

/// Cross-platform `Image` component — sources from either a GPU texture
/// already in VRAM (a host-supplied [`TextureHandle`]) or encoded PNG/WebP
/// bytes, decoded once at construction time via [`crate::image`]. See
/// ADR 0015 for the promotion process this follows (leaf, not container;
/// corner radius comes from each backend's own theme, same as
/// [`Card`]'s chrome, rather than a field here).
pub struct Image {
    class: String,
    source: Option<ImageSource>,
    fit: ImageFit,
    width: u32,
    height: u32,
}

impl Image {
    /// Build from already-encoded PNG or WebP bytes (#196): sniffs the
    /// format and decodes it once, up front, to validate the bytes and
    /// recover `width`/`height` — construction fails on bad bytes rather
    /// than surfacing a decode error later at lowering or render time. The
    /// node's source keeps the *original encoded bytes*, not the decoded
    /// pixels, so a backend with its own decoder (a browser's native
    /// `<img>`) never pays for a redundant decode.
    pub fn from_encoded(bytes: impl Into<Vec<u8>>) -> Result<Self, ImageDecodeError> {
        let bytes = bytes.into();
        let format = sniff_format(&bytes)?;
        let decoded = decode_rgba8(&bytes, format)?;
        Ok(Self {
            class: String::new(),
            source: Some(ImageSource::Encoded { format, bytes }),
            fit: ImageFit::Contain,
            width: decoded.width,
            height: decoded.height,
        })
    }

    /// Build from a GPU texture a host already registered with its own
    /// renderer (e.g. `mkui_wgpu::Renderer::register_texture`). `width`/
    /// `height` are the host's own — this crate never touches a real GPU
    /// resource to ask a texture its size.
    pub fn from_texture(handle: TextureHandle, width: u32, height: u32) -> Self {
        Self {
            class: String::new(),
            source: Some(ImageSource::Texture(handle)),
            fit: ImageFit::Contain,
            width,
            height,
        }
    }

    pub fn class(mut self, class: impl Into<String>) -> Self {
        self.class = class.into();
        self
    }

    /// Default is [`ImageFit::Contain`] — never crops, matching this crate's
    /// general bias toward the non-destructive default where CSS itself
    /// defaults to `fill` (distorting the aspect ratio, which this
    /// component does not offer at all).
    pub fn fit(mut self, fit: ImageFit) -> Self {
        self.fit = fit;
        self
    }

    pub fn class_name(&self) -> &str {
        &self.class
    }

    pub fn image_fit(&self) -> ImageFit {
        self.fit
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }
}

impl Component for Image {}

impl LowerToTree for Image {
    fn lower_to_tree(
        &mut self,
        tree: &mut AppTree,
        parent: NodeId,
        _registry: &LoweringRegistry,
    ) -> Result<NodeId, ClassParseError> {
        let class = std::mem::take(&mut self.class);
        let source = self
            .source
            .take()
            .expect("Image: lower_to_tree called more than once");
        let style = StyleClass::from_str(class);
        let _ = style.parse()?;
        Ok(tree.push_image_unchecked(parent, source, self.fit, self.width, self.height, style))
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
/// `crates/core/tests/parity.rs`); each backend decides for itself
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
    fn button_disabled_lowers_into_the_tree() {
        let app = Mkui::new().child(Button::new("Save").disabled(true));
        let root = app.tree().get(app.tree().root()).unwrap();
        let button = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Button(b) = &button.kind else {
            panic!("expected Button")
        };
        assert!(b.disabled);
    }

    #[test]
    fn button_defaults_to_not_disabled() {
        let app = Mkui::new().child(Button::new("Save"));
        let root = app.tree().get(app.tree().root()).unwrap();
        let button = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Button(b) = &button.kind else {
            panic!("expected Button")
        };
        assert!(!b.disabled);
    }

    #[test]
    fn card_lowers_to_a_card_node_with_its_children() {
        let app = Mkui::new().child(
            Card::new()
                .class("p-6")
                .child(Text::new("a"))
                .child(Button::new("ok")),
        );
        let root = app.tree().get(app.tree().root()).unwrap();
        let card = app.tree().get(root.children[0]).unwrap();
        assert!(matches!(card.kind, NodeKind::Card(_)));
        assert_eq!(card.class.raw(), "p-6");
        assert_eq!(card.children.len(), 2);
    }

    fn tiny_png_bytes() -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(&[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
        }
        bytes
    }

    #[test]
    fn image_from_encoded_recovers_dimensions_from_the_png_header() {
        let image = Image::from_encoded(tiny_png_bytes()).unwrap();
        assert_eq!(image.width(), 2);
        assert_eq!(image.height(), 1);
        assert_eq!(
            image.image_fit(),
            ImageFit::Contain,
            "default fit is Contain"
        );
    }

    #[test]
    fn image_from_encoded_rejects_bytes_that_do_not_decode() {
        assert!(Image::from_encoded(b"not an image".to_vec()).is_err());
    }

    #[test]
    fn image_lowers_to_an_image_node_carrying_its_source_and_fit() {
        let app = Mkui::new().child(
            Image::from_encoded(tiny_png_bytes())
                .unwrap()
                .class("rounded-lg")
                .fit(ImageFit::Cover),
        );
        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Image(img) = &node.kind else {
            panic!("expected Image")
        };
        assert_eq!(img.fit, ImageFit::Cover);
        assert_eq!(img.width, 2);
        assert_eq!(img.height, 1);
        assert!(matches!(
            img.source,
            mkui_runtime::ImageSource::Encoded { .. }
        ));
        assert_eq!(node.class.raw(), "rounded-lg");
    }

    #[test]
    fn image_from_texture_carries_the_host_supplied_handle_and_size() {
        let app = Mkui::new().child(Image::from_texture(TextureHandle(42), 64, 32));
        let root = app.tree().get(app.tree().root()).unwrap();
        let node = app.tree().get(root.children[0]).unwrap();
        let NodeKind::Image(img) = &node.kind else {
            panic!("expected Image")
        };
        assert_eq!(img.width, 64);
        assert_eq!(img.height, 32);
        assert!(matches!(
            img.source,
            mkui_runtime::ImageSource::Texture(TextureHandle(42))
        ));
    }

    #[test]
    fn try_child_succeeds_on_an_unknown_class() {
        // #269: an unrecognized utility class is dropped, not an error.
        let app = Mkui::new()
            .try_child(View::new().class("totally-bogus-utility"))
            .expect("try_child must not reject an unknown class");
        assert_eq!(app.children_len(), 1);
    }

    #[test]
    fn child_does_not_panic_on_unknown_class() {
        // #269 acceptance criterion: an unrecognized token does not panic —
        // the component still gets appended.
        let app = Mkui::new().child(View::new().class("totally-bogus-utility"));
        assert_eq!(app.children_len(), 1);
    }

    #[test]
    fn child_still_applies_every_other_class_around_an_unknown_token() {
        // #269 acceptance criterion: the component still renders with its
        // other classes applied, regardless of where the unknown token sits
        // in the string.
        let app = Mkui::new().child(View::new().class("flex-col totally-bogus-utility gap-4"));
        let root = app.tree().get(app.tree().root()).unwrap();
        let view = app.tree().get(root.children[0]).unwrap();
        assert!(view.resolved.flex_column);
        assert_eq!(view.resolved.gap, Some(4));
        assert_eq!(view.resolved.unknown_count, 1);
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
