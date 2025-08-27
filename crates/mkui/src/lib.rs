// Bridge crate that exposes the appropriate mkui implementation based on features

use mkui_core::error::MkuiError;

// Use a more robust approach that handles feature conflicts gracefully
// If both features are enabled (e.g., by rust-analyzer), prefer console
#[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
pub struct Mkui {
    inner: mkui_console::prelude::Mkui,
}

#[cfg(all(feature = "web", not(feature = "console")))]
pub struct Mkui {
    inner: mkui_web::prelude::Mkui,
}

impl Mkui {
    pub fn new() -> Result<Self, MkuiError> {
        // Prioritize console if both features are enabled
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            match mkui_console::prelude::Mkui::new() {
                Ok(inner) => Ok(Self { inner }),
                Err(io_error) => Err(MkuiError::initialization(format!("Console initialization failed: {}", io_error))),
            }
        }
        
        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            match mkui_web::prelude::Mkui::new() {
                Ok(inner) => Ok(Self { inner }),
                Err(js_error) => Err(MkuiError::initialization(format!("Web initialization failed: {:?}", js_error))),
            }
        }
        
        #[cfg(not(any(feature = "web", feature = "console")))]
        {
            Err(MkuiError::initialization("No mkui backend feature enabled (web or console required)"))
        }
    }
    
    pub fn child(self, child: impl mkui_core::components::Component + 'static) -> Self {
        Self {
            inner: self.inner.child(child),
        }
    }
    
    // Return Result with MkuiError for both platforms - unified interface
    pub fn run(self) -> Result<(), MkuiError> {
        // Prioritize console if both features are enabled
        #[cfg(any(feature = "console", all(feature = "web", feature = "console")))]
        {
            match self.inner.run() {
                Ok(()) => Ok(()),
                Err(io_error) => Err(MkuiError::io(format!("Console run failed: {}", io_error))),
            }
        }
        
        #[cfg(all(feature = "web", not(feature = "console")))]
        {
            match self.inner.run() {
                Ok(()) => Ok(()),
                Err(js_error) => Err(MkuiError::rendering(format!("Web run failed: {:?}", js_error))),
            }
        }
        
        #[cfg(not(any(feature = "web", feature = "console")))]
        {
            Err(MkuiError::generic("No mkui backend feature enabled"))
        }
    }
}

/// Macro to run a showcase with platform-specific error handling
/// Usage: mkui::run!(create_showcase_ui, web) or mkui::run!(create_showcase_ui, console)
#[macro_export]
macro_rules! run {
    ($create_ui_fn:expr, web) => {
        {
            use wasm_bindgen::prelude::*;
            
            // Create the UI
            let app = match $create_ui_fn() {
                Ok(app) => app,
                Err(e) => {
                    web_sys::console::log_1(&format!("Failed to create app: {}", e).into());
                    return Err(wasm_bindgen::JsValue::from_str(&e.to_string()));
                }
            };
            
            // Run the app
            match app.run() {
                Ok(()) => Ok(()),
                Err(e) => {
                    web_sys::console::log_1(&format!("Failed to run app: {}", e).into());
                    Err(wasm_bindgen::JsValue::from_str(&e.to_string()))
                }
            }
        }
    };
    
    ($create_ui_fn:expr, console) => {
        {
            // Create the UI
            let app = match $create_ui_fn() {
                Ok(app) => app,
                Err(e) => {
                    eprintln!("Failed to create app: {}", e);
                    return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
                }
            };
            
            // Run the app
            match app.run() {
                Ok(()) => Ok(()),
                Err(e) => {
                    eprintln!("Failed to run app: {}", e);
                    Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                }
            }
        }
    };
}

// Re-export core components for convenience
pub mod prelude {
    pub use crate::Mkui;
    pub use mkui_core::prelude::*;
}
