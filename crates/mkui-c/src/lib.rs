//! C/C++ bindings for mkui UI library
//! 
//! This crate provides a C-compatible API for the mkui library,
//! allowing C and C++ applications to create cross-platform UIs.

use std::ffi::{CStr, CString, c_char, c_int};
use std::ptr;
use mkui::prelude::*;
use mkui_core::components::{View, Text, Button};
use mkui_core::headless::ButtonVariant;

// Ensure we have the proper forward declaration for C
#[repr(C)]
pub struct MkuiAppOpaque {
    _private: [u8; 0],
}

/// Opaque handle to a Mkui application instance
/// This struct is opaque to C code - only pointers to it are used
pub struct MkuiApp {
    inner: Option<Mkui>,
}

/// Error codes for mkui operations
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub enum MkuiErrorCode {
    Success = 0,
    InitializationFailed = 1,
    InvalidParameter = 2,
    RuntimeError = 3,
    OutOfMemory = 4,
}

/// Result type for C API functions
#[repr(C)]
pub struct MkuiResult {
    pub code: MkuiErrorCode,
    pub message: *const c_char,
}

impl MkuiResult {
    fn success() -> Self {
        Self {
            code: MkuiErrorCode::Success,
            message: ptr::null(),
        }
    }

    fn error(code: MkuiErrorCode, message: &str) -> Self {
        let c_message = CString::new(message).unwrap_or_else(|_| {
            CString::new("Failed to create error message").unwrap()
        });
        Self {
            code,
            message: c_message.into_raw(),
        }
    }
}

/// Initialize a new mkui application
#[no_mangle]
pub extern "C" fn mkui_app_new() -> *mut MkuiApp {
    match Mkui::new() {
        Ok(app) => {
            Box::into_raw(Box::new(MkuiApp { inner: Some(app) }))
        }
        Err(_) => ptr::null_mut(),
    }
}

/// Free a mkui application
#[no_mangle]
pub extern "C" fn mkui_app_free(app: *mut MkuiApp) {
    if !app.is_null() {
        unsafe {
            let _ = Box::from_raw(app);
        }
    }
}

/// Add a view component to the application
#[no_mangle]
pub extern "C" fn mkui_app_add_view(app: *mut MkuiApp, class_name: *const c_char) -> MkuiResult {
    if app.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "App pointer is null");
    }

    let class_str = if class_name.is_null() {
        ""
    } else {
        match unsafe { CStr::from_ptr(class_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid class name"),
        }
    };

    unsafe {
        let app_ref = &mut *app;
        if let Some(mkui_app) = app_ref.inner.take() {
            let view = View::new().class(class_str);
            app_ref.inner = Some(mkui_app.child(view));
            MkuiResult::success()
        } else {
            MkuiResult::error(MkuiErrorCode::RuntimeError, "App is not initialized")
        }
    }
}

/// Add a text component to the application
#[no_mangle]
pub extern "C" fn mkui_app_add_text(
    app: *mut MkuiApp, 
    content: *const c_char, 
    class_name: *const c_char
) -> MkuiResult {
    if app.is_null() || content.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid parameters");
    }

    let content_str = match unsafe { CStr::from_ptr(content) }.to_str() {
        Ok(s) => s,
        Err(_) => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid content"),
    };

    let class_str = if class_name.is_null() {
        ""
    } else {
        match unsafe { CStr::from_ptr(class_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid class name"),
        }
    };

    unsafe {
        let app_ref = &mut *app;
        if let Some(mkui_app) = app_ref.inner.take() {
            let text = Text::new(content_str).class(class_str);
            app_ref.inner = Some(mkui_app.child(text));
            MkuiResult::success()
        } else {
            MkuiResult::error(MkuiErrorCode::RuntimeError, "App is not initialized")
        }
    }
}

/// Add a button component to the application
#[no_mangle]
pub extern "C" fn mkui_app_add_button(
    app: *mut MkuiApp,
    text: *const c_char,
    class_name: *const c_char,
    variant: c_int,
) -> MkuiResult {
    if app.is_null() || text.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid parameters");
    }

    let text_str = match unsafe { CStr::from_ptr(text) }.to_str() {
        Ok(s) => s,
        Err(_) => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid text"),
    };

    let class_str = if class_name.is_null() {
        ""
    } else {
        match unsafe { CStr::from_ptr(class_name) }.to_str() {
            Ok(s) => s,
            Err(_) => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid class name"),
        }
    };

    let button_variant = match variant {
        0 => ButtonVariant::Primary,
        1 => ButtonVariant::Secondary,
        2 => ButtonVariant::Destructive,
        3 => ButtonVariant::Outline,
        4 => ButtonVariant::Ghost,
        5 => ButtonVariant::Link,
        _ => return MkuiResult::error(MkuiErrorCode::InvalidParameter, "Invalid button variant"),
    };

    unsafe {
        let app_ref = &mut *app;
        if let Some(mkui_app) = app_ref.inner.take() {
            let button = Button::new(text_str)
                .class(class_str)
                .variant(button_variant);
            app_ref.inner = Some(mkui_app.child(button));
            MkuiResult::success()
        } else {
            MkuiResult::error(MkuiErrorCode::RuntimeError, "App is not initialized")
        }
    }
}

/// Run the mkui application (console version)
#[no_mangle]
pub extern "C" fn mkui_app_run_console(app: *mut MkuiApp) -> MkuiResult {
    if app.is_null() {
        return MkuiResult::error(MkuiErrorCode::InvalidParameter, "App pointer is null");
    }

    unsafe {
        let app_ref = &mut *app;
        if let Some(mkui_app) = app_ref.inner.take() {
            match mkui_app.run() {
                Ok(_) => MkuiResult::success(),
                Err(e) => MkuiResult::error(MkuiErrorCode::RuntimeError, &e.to_string()),
            }
        } else {
            MkuiResult::error(MkuiErrorCode::RuntimeError, "App is not initialized")
        }
    }
}

/// Free an error message returned by mkui functions
#[no_mangle]
pub extern "C" fn mkui_free_error_message(message: *mut c_char) {
    if !message.is_null() {
        unsafe {
            let _ = CString::from_raw(message);
        }
    }
}

/// Get version information
#[no_mangle]
pub extern "C" fn mkui_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// Button variant constants for C
#[no_mangle]
pub static MKUI_BUTTON_PRIMARY: c_int = 0;
#[no_mangle]
pub static MKUI_BUTTON_SECONDARY: c_int = 1;
#[no_mangle]
pub static MKUI_BUTTON_DESTRUCTIVE: c_int = 2;
#[no_mangle]
pub static MKUI_BUTTON_OUTLINE: c_int = 3;
#[no_mangle]
pub static MKUI_BUTTON_GHOST: c_int = 4;
#[no_mangle]
pub static MKUI_BUTTON_LINK: c_int = 5;