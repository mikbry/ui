//! `mkui` is the bridge crate: it exposes a single [`Mkui`] entry point that
//! resolves to whichever backend is enabled by Cargo features. All component
//! and contract types come from `mkui-core`.

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
    #[cfg(not(any(feature = "web", feature = "console")))]
    _marker: std::marker::PhantomData<()>,
}

impl Mkui {
    pub fn new() -> Result<Self, MkuiError> {
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            mkui_console::prelude::Mkui::new()
                .map(|inner| Self { inner })
                .map_err(|e| MkuiError::initialization(format!("Console initialization failed: {}", e)))
        }

        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            mkui_web::prelude::Mkui::new()
                .map(|inner| Self { inner })
                .map_err(|e| MkuiError::initialization(format!("Web initialization failed: {:?}", e)))
        }

        #[cfg(not(any(feature = "web", feature = "console")))]
        {
            Err(MkuiError::initialization(
                "No mkui backend feature enabled (enable `web` or `console`)",
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
        #[cfg(not(any(feature = "web", feature = "console")))]
        {
            let _ = child;
            self
        }
    }

    pub fn run(self) -> Result<(), MkuiError> {
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            self.inner
                .run()
                .map_err(|e| MkuiError::io(format!("Console run failed: {}", e)))
        }

        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            self.inner
                .run()
                .map_err(|e| MkuiError::rendering(format!("Web run failed: {:?}", e)))
        }

        #[cfg(not(any(feature = "web", feature = "console")))]
        {
            Err(MkuiError::generic("No mkui backend feature enabled"))
        }
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
                return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
            }
        };

        match app.run() {
            Ok(()) => Ok(()),
            Err(e) => {
                eprintln!("Failed to run app: {}", e);
                Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
            }
        }
    }};
}

pub mod prelude {
    pub use crate::Mkui;
    pub use mkui_core::prelude::*;
}
