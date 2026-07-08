#![forbid(unsafe_code)]
//! # mkui — bridge crate selecting the right backend at compile time
//!
//! `mkui` is the *bridge crate* of the mkui workspace. End users depend on
//! `mkui` and pick a backend with a Cargo feature; everything else dispatches
//! through the cross-platform component tree from [`mkui_core`].
//!
//! ## Architectural template
//!
//! Every backend in the workspace follows the same three-layer shape:
//!
//! ```text
//!                  +------------------------+
//!                  |          mkui          |  Bridge: picks a backend via
//!                  |  (this crate, re-       features, hands users a single
//!                  |   exports Mkui)        |  `mkui::Mkui` type.
//!                  +-----------+------------+
//!                              |
//!                +-------------+-------------+
//!                |             |             |
//!         +------v-----+ +-----v------+ +----v-------+
//!         |  mkui-web  | |mkui-console| |  mkui-wgpu |
//!         |  (DOM)     | | (crossterm)| | (scene)    |
//!         +------+-----+ +-----+------+ +-----+------+
//!                |             |             |
//!                +-------------+-------------+
//!                              |
//!                  +-----------v------------+
//!                  |       mkui-core        |  Contract: components, headless
//!                  |     (shared types)     |  logic, theme, layout, events.
//!                  +------------------------+
//! ```
//!
//! - **Contract crate** ([`mkui_core`]) — defines `View` / `Text` / `Button`,
//!   headless logic, theme primitives, layout, input events. Depends on no
//!   backend.
//! - **Backend crates** (`mkui-web`, `mkui-console`, `mkui-wgpu`) — each
//!   exposes the same five-module shape:
//!   `app` (backend state), `renderer` (output surface), `components`
//!   (backend-specific projections of the contract), `high_level` (the
//!   `Mkui` entry point), and `prelude`. Depends on [`mkui_core`] only.
//! - **Bridge crate** (this crate) — picks **exactly one** backend at compile
//!   time via Cargo features (`web`, `console`, or `wgpu`) and re-exports its
//!   `Mkui` type plus the shared contract. Enabling more than one primary
//!   backend feature is a hard compile error (see ADR 0006): there is no
//!   silent precedence, so feature unification cannot quietly swap the backend
//!   a consumer intended.
//!
//! Adding a new backend means copying the five-module template, depending on
//! [`mkui_core`], implementing `Mkui::new() / .child(...) / .run()`, and
//! threading a feature into `mkui`'s `Cargo.toml`. No contract changes
//! required.
//!
//! ## Using the bridge
//!
//! End users typically write:
//!
//! ```ignore
//! use mkui::prelude::*; // brings Mkui + mkui_core::prelude::*
//!
//! fn create_ui() -> Result<Mkui, MkuiError> {
//!     Ok(Mkui::new()?.child(Text::new("hello")))
//! }
//! ```
//!
//! With `--features web`, [`Mkui`] resolves to `mkui_web::Mkui` (DOM
//! rendering via `wasm-bindgen`). With `--features console`, it resolves to
//! `mkui_console::Mkui` (terminal rendering via `crossterm`). With
//! `--features wgpu`, it resolves to `mkui_wgpu::Mkui` (GPU scene rendering).
//! Enabling more than one of these is a compile-time error. Without any
//! backend feature, [`Mkui::new`] returns an [`MkuiError`] explaining what
//! to enable — library consumers get a clear runtime message instead of an
//! opaque link error.
//!
//! The [`run!`] macro takes care of converting backend-specific errors into
//! the `Result` type the host platform expects (`JsValue` on web,
//! `io::Error` on console), so showcase binaries can stay one-liners.

// Used by the gated `Mkui` impl below; on a conflicting feature set that impl
// is excluded, so gate the import too — otherwise it reads as unused and adds
// a warning on top of the one-backend `compile_error!`.
#[cfg(not(any(
    all(feature = "web", feature = "console"),
    all(feature = "web", feature = "wgpu"),
    all(feature = "console", feature = "wgpu"),
)))]
use mkui_core::error::MkuiError;

// One-primary-backend invariant (see ADR 0006 "Cross-binding identity: one
// primary backend"). The `web`, `console`, and `wgpu` features each select a
// different `Mkui` implementation; enabling more than one is ambiguous, and
// Cargo feature unification across a dependency graph could otherwise pick a
// backend the consumer never intended — silently. Make every conflicting pair
// a hard compile error instead of resolving it by hidden cfg precedence.
//
// Critically, the entire valid implementation below (`struct Mkui`, `impl
// Mkui`, the prelude re-export, and the web error-translation helper) is gated
// on `not(<any conflict>)` so that a conflicting feature set produces *only*
// this `compile_error!` — not a wall of E0124 (duplicate `inner` field) and
// E0308 (mismatched types) noise from two backends' bodies being compiled at
// once. A user who enables `web` + `console` must see one clear message.
#[cfg(any(
    all(feature = "web", feature = "console"),
    all(feature = "web", feature = "wgpu"),
    all(feature = "console", feature = "wgpu"),
))]
compile_error!("mkui: enable exactly one primary backend feature: `web`, `console`, or `wgpu`");

/// Cross-backend `Mkui` app.
///
/// The component tree is built from the shared `mkui-core` model and dispatched
/// to the backend selected by exactly one Cargo feature (`web`, `console`, or
/// `wgpu`). Enabling more than one is a compile-time error; enabling none makes
/// [`Mkui::new`] return an initialization error so library consumers get a clear
/// message instead of a link error.
#[cfg(not(any(
    all(feature = "web", feature = "console"),
    all(feature = "web", feature = "wgpu"),
    all(feature = "console", feature = "wgpu"),
)))]
pub struct Mkui {
    #[cfg(feature = "console")]
    inner: mkui_console::prelude::Mkui,
    #[cfg(feature = "web")]
    inner: mkui_web::prelude::Mkui,
    #[cfg(feature = "wgpu")]
    inner: mkui_wgpu::prelude::Mkui,
    #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
    _marker: std::marker::PhantomData<()>,
}

#[cfg(not(any(
    all(feature = "web", feature = "console"),
    all(feature = "web", feature = "wgpu"),
    all(feature = "console", feature = "wgpu"),
)))]
impl Mkui {
    /// Create a new [`Mkui`] app for the backend selected by Cargo features.
    ///
    /// Exactly one primary backend feature (`web`, `console`, or `wgpu`) must
    /// be enabled; the selected backend's initialization runs here. With **no**
    /// backend feature enabled this returns an [`MkuiError`]
    /// explaining what to enable, so library consumers get a clear message
    /// instead of an opaque link error.
    ///
    /// The result is a builder: attach a root component tree with
    /// [`child`](Mkui::child), then start the event loop with
    /// [`run`](Mkui::run). Components come from [`mkui_core`] — for example
    /// [`Text`](mkui_core::components::Text),
    /// [`Button`](mkui_core::components::Button),
    /// [`View`](mkui_core::components::View), and anything implementing
    /// [`Component`](mkui_core::components::Component).
    ///
    /// # Errors
    ///
    /// Returns an [`MkuiError`] if the backend
    /// fails to initialize, or if no backend feature is enabled.
    ///
    /// # Examples
    ///
    /// With a backend feature enabled, `new` yields an app builder:
    ///
    /// ```no_run
    /// use mkui::prelude::*;
    ///
    /// # fn main() -> Result<(), MkuiError> {
    /// let app = Mkui::new()?.child(Text::new("hello"));
    /// app.run()?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// With no backend feature enabled, `new` reports the misconfiguration
    /// instead of panicking:
    ///
    /// ```
    /// # #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
    /// # {
    /// use mkui::prelude::*;
    ///
    /// assert!(Mkui::new().is_err());
    /// # }
    /// ```
    pub fn new() -> Result<Self, MkuiError> {
        #[cfg(feature = "console")]
        {
            let inner = mkui_console::prelude::Mkui::new()?;
            Ok(Self { inner })
        }

        #[cfg(feature = "web")]
        {
            let inner = mkui_web::prelude::Mkui::new().map_err(js_value_to_mkui_error)?;
            Ok(Self { inner })
        }

        #[cfg(feature = "wgpu")]
        {
            let inner = mkui_wgpu::prelude::Mkui::new()?;
            Ok(Self { inner })
        }

        #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
        {
            Err(MkuiError::initialization(
                "No mkui backend feature enabled (enable `web`, `console`, or `wgpu`)",
            ))
        }
    }

    /// Attach a root component to the app's tree, returning the app for
    /// chaining.
    ///
    /// `child` accepts anything implementing
    /// [`Component`](mkui_core::components::Component) — the cross-platform
    /// components from [`mkui_core`] such as
    /// [`View`](mkui_core::components::View),
    /// [`Text`](mkui_core::components::Text), and
    /// [`Button`](mkui_core::components::Button), or your own. Calls chain
    /// builder-style; a typical app nests a
    /// [`View`](mkui_core::components::View) container holding
    /// [`Text`](mkui_core::components::Text) and
    /// [`Button`](mkui_core::components::Button) leaves.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mkui::prelude::*;
    ///
    /// # fn main() -> Result<(), MkuiError> {
    /// let app = Mkui::new()?.child(
    ///     View::new()
    ///         .child(Text::new("Counter"))
    ///         .child(Button::new("Increment")),
    /// );
    /// # let _ = app;
    /// # Ok(())
    /// # }
    /// ```
    pub fn child(self, child: impl mkui_core::components::Component + 'static) -> Self {
        #[cfg(any(feature = "web", feature = "console", feature = "wgpu"))]
        {
            Self {
                inner: self.inner.child(child),
            }
        }
        #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
        {
            let _ = child;
            self
        }
    }

    /// Start the backend's event loop, rendering the app until it exits.
    ///
    /// This hands control to the selected backend — the DOM on `web`, the
    /// terminal on `console`, a GPU window on `wgpu` — and returns when the
    /// loop ends. It is the terminal call in the `new` → `child` → `run`
    /// builder flow. Showcase binaries usually go through the
    /// [`run!`](crate::run) macro instead, which wraps this with
    /// platform-specific error conversion.
    ///
    /// # Errors
    ///
    /// Returns an [`MkuiError`] if the backend's
    /// render loop fails, or if no backend feature is enabled.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mkui::prelude::*;
    ///
    /// # fn main() -> Result<(), MkuiError> {
    /// Mkui::new()?
    ///     .child(Text::new("hello"))
    ///     .run()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn run(self) -> Result<(), MkuiError> {
        #[cfg(feature = "console")]
        {
            self.inner.run()?;
            Ok(())
        }

        #[cfg(feature = "web")]
        {
            self.inner.run().map_err(js_value_to_mkui_error)
        }

        #[cfg(feature = "wgpu")]
        {
            self.inner.run()
        }

        #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
        {
            Err(MkuiError::generic("No mkui backend feature enabled"))
        }
    }
}

/// Convert a `JsValue` from the web backend into a typed `MkuiError`.
///
/// On `wasm32` this hits the `#[from]` impl on `MkuiError::JsValue`. On
/// native (the workspace's CI target) the cfg-gated variant isn't available,
/// so we fall back to a rendered string in `Rendering`. Either way, the
/// bridge never re-stringifies via `format!("{:?}", e)` on user-reachable
/// paths beyond this single translation layer.
#[cfg(all(feature = "web", not(any(feature = "console", feature = "wgpu"))))]
fn js_value_to_mkui_error(value: wasm_bindgen::JsValue) -> MkuiError {
    #[cfg(target_arch = "wasm32")]
    {
        MkuiError::from(value)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        MkuiError::rendering(format!("web backend error: {value:?}"))
    }
}

/// Run a showcase function with platform-specific error conversion.
///
/// `mkui::run!(create_ui, web)` returns `Result<(), wasm_bindgen::JsValue>`;
/// `mkui::run!(create_ui, console)` returns `std::io::Result<()>`.
#[macro_export]
macro_rules! run {
    ($create_ui_fn:expr, web) => {{
        use wasm_bindgen::prelude::*;

        let app = match $create_ui_fn() {
            Ok(app) => app,
            Err(e) => {
                web_sys::console::log_1(&format!("Failed to create app: {}", e).into());
                return Err(wasm_bindgen::JsValue::from_str(&e.to_string()));
            }
        };

        match app.run() {
            Ok(()) => Ok(()),
            Err(e) => {
                web_sys::console::log_1(&format!("Failed to run app: {}", e).into());
                Err(wasm_bindgen::JsValue::from_str(&e.to_string()))
            }
        }
    }};

    ($create_ui_fn:expr, console) => {{
        let app = match $create_ui_fn() {
            Ok(app) => app,
            Err(e) => {
                eprintln!("Failed to create app: {}", e);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ));
            }
        };

        match app.run() {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("Failed to run app: {}", e);
                Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                ))
            }
        }
    }};

    ($create_ui_fn:expr, wgpu) => {{
        // Native wgpu arm: surfaces `MkuiError` via `Box<dyn Error>` so a
        // showcase binary's `main` can return the same type the native
        // example template uses.
        let app = match $create_ui_fn() {
            Ok(app) => app,
            Err(e) => {
                eprintln!("Failed to create app: {}", e);
                return Err(Box::<dyn std::error::Error>::from(e.to_string()));
            }
        };

        match app.run() {
            Ok(()) => Ok::<(), Box<dyn std::error::Error>>(()),
            Err(e) => {
                eprintln!("Failed to run app: {}", e);
                Err(Box::<dyn std::error::Error>::from(e.to_string()))
            }
        }
    }};
}

pub mod prelude {
    // `Mkui` only exists when the one-backend invariant holds; on a conflicting
    // feature set it is gated out so the sole error is the top-level
    // `compile_error!` rather than an additional unresolved-import (E0432).
    #[cfg(not(any(
        all(feature = "web", feature = "console"),
        all(feature = "web", feature = "wgpu"),
        all(feature = "console", feature = "wgpu"),
    )))]
    pub use crate::Mkui;
    pub use mkui_core::prelude::*;
}
