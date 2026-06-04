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
//! - **Bridge crate** (this crate) — picks one backend at compile time via
//!   Cargo features (`web`, `console`) and re-exports its `Mkui` type
//!   plus the shared contract.
//!
//! Adding a new backend means copying the five-module template, depending on
//! [`mkui_core`], implementing `Mkui::new() / .child(...) / .run()`, and
//! threading a feature into [`mkui`]'s `Cargo.toml`. No contract changes
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
//! `mkui_console::Mkui` (terminal rendering via `crossterm`). Without any
//! backend feature, [`Mkui::new`] returns an [`MkuiError`] explaining what
//! to enable — library consumers get a clear runtime message instead of an
//! opaque link error.
//!
//! The [`run!`] macro takes care of converting backend-specific errors into
//! the `Result` type the host platform expects (`JsValue` on web,
//! `io::Error` on console), so showcase binaries can stay one-liners.

use mkui_core::error::MkuiError;

/// Cross-backend `Mkui` app.
///
/// The component tree is built from the shared `mkui-core` model and dispatched
/// to the backend selected by Cargo features (`web` or `console`). Without a
/// backend feature, [`Mkui::new`] returns an initialization error so library
/// consumers get a clear message instead of a link error.
pub struct Mkui {
    #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
    inner: mkui_console::prelude::Mkui,
    #[cfg(all(feature = "web", not(feature = "console")))]
    inner: mkui_web::prelude::Mkui,
    #[cfg(all(feature = "wgpu", not(any(feature = "console", feature = "web"))))]
    inner: mkui_wgpu::prelude::Mkui,
    #[cfg(not(any(feature = "web", feature = "console", feature = "wgpu")))]
    _marker: std::marker::PhantomData<()>,
}

impl Mkui {
    pub fn new() -> Result<Self, MkuiError> {
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            let inner = mkui_console::prelude::Mkui::new()?;
            Ok(Self { inner })
        }

        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            let inner = mkui_web::prelude::Mkui::new().map_err(js_value_to_mkui_error)?;
            Ok(Self { inner })
        }

        #[cfg(all(feature = "wgpu", not(any(feature = "console", feature = "web"))))]
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

    pub fn child(self, child: impl mkui_core::components::Component + 'static) -> Self {
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            Self {
                inner: self.inner.child(child),
            }
        }
        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            Self {
                inner: self.inner.child(child),
            }
        }
        #[cfg(all(feature = "wgpu", not(any(feature = "console", feature = "web"))))]
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

    pub fn run(self) -> Result<(), MkuiError> {
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            self.inner.run()?;
            Ok(())
        }

        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            self.inner.run().map_err(js_value_to_mkui_error)
        }

        #[cfg(all(feature = "wgpu", not(any(feature = "console", feature = "web"))))]
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
#[cfg(all(feature = "web", not(feature = "console")))]
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
    pub use crate::Mkui;
    pub use mkui_core::prelude::*;
}
