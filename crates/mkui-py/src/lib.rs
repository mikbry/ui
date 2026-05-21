//! Python bindings for mkui UI library
//!
//! This crate provides Python bindings using PyO3, allowing Python applications
//! to create cross-platform UIs with mkui.

use mkui::prelude::*;
use mkui_core::components::{Button, Text, View};
use mkui_core::headless::ButtonVariant;
use pyo3::prelude::*;

/// Python wrapper for MkuiError
#[pyclass]
#[derive(Debug, Clone)]
pub struct PyMkuiError {
    inner: String,
}

impl From<MkuiError> for PyMkuiError {
    fn from(error: MkuiError) -> Self {
        Self {
            inner: error.to_string(),
        }
    }
}

#[pymethods]
impl PyMkuiError {
    fn __str__(&self) -> &str {
        &self.inner
    }

    fn __repr__(&self) -> String {
        format!("MkuiError('{}')", self.inner)
    }
}

impl std::fmt::Display for PyMkuiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl std::error::Error for PyMkuiError {}

/// Button variant enumeration for Python
#[pyclass(eq, eq_int)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PyButtonVariant {
    Primary = 0,
    Secondary = 1,
    Destructive = 2,
    Outline = 3,
    Ghost = 4,
    Link = 5,
}

impl From<PyButtonVariant> for ButtonVariant {
    fn from(variant: PyButtonVariant) -> Self {
        match variant {
            PyButtonVariant::Primary => ButtonVariant::Primary,
            PyButtonVariant::Secondary => ButtonVariant::Secondary,
            PyButtonVariant::Destructive => ButtonVariant::Destructive,
            PyButtonVariant::Outline => ButtonVariant::Outline,
            PyButtonVariant::Ghost => ButtonVariant::Ghost,
            PyButtonVariant::Link => ButtonVariant::Link,
        }
    }
}

/// Python wrapper for mkui applications
#[pyclass]
pub struct App {
    inner: Option<Mkui>,
}

#[pymethods]
impl App {
    /// Create a new mkui application
    #[new]
    fn new() -> PyResult<Self> {
        match Mkui::new() {
            Ok(app) => Ok(Self { inner: Some(app) }),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "Failed to create mkui app: {}",
                e
            ))),
        }
    }

    /// Add a view component to the application
    #[pyo3(signature = (class_name=None))]
    fn add_view(&mut self, class_name: Option<&str>) -> PyResult<()> {
        if let Some(app) = self.inner.take() {
            let view = View::new().class(class_name.unwrap_or(""));
            self.inner = Some(app.child(view));
            Ok(())
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "App is not initialized",
            ))
        }
    }

    /// Add a text component to the application
    #[pyo3(signature = (content, class_name=None))]
    fn add_text(&mut self, content: &str, class_name: Option<&str>) -> PyResult<()> {
        if let Some(app) = self.inner.take() {
            let text = Text::new(content).class(class_name.unwrap_or(""));
            self.inner = Some(app.child(text));
            Ok(())
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "App is not initialized",
            ))
        }
    }

    /// Add a button component to the application
    #[pyo3(signature = (text, variant=None, class_name=None))]
    fn add_button(
        &mut self,
        text: &str,
        variant: Option<PyButtonVariant>,
        class_name: Option<&str>,
    ) -> PyResult<()> {
        if let Some(app) = self.inner.take() {
            let button_variant = variant.unwrap_or(PyButtonVariant::Primary).into();
            let button = Button::new(text)
                .class(class_name.unwrap_or(""))
                .variant(button_variant);
            self.inner = Some(app.child(button));
            Ok(())
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "App is not initialized",
            ))
        }
    }

    /// Run the application (console mode)
    fn run_console(&mut self) -> PyResult<()> {
        if let Some(app) = self.inner.take() {
            match app.run() {
                Ok(_) => Ok(()),
                Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                    "App run failed: {}",
                    e
                ))),
            }
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(
                "App is not initialized",
            ))
        }
    }

    /// Method chaining support - return self for fluent API
    #[pyo3(signature = (class_name=None))]
    fn view(&mut self, class_name: Option<&str>) -> PyResult<()> {
        self.add_view(class_name)
    }

    /// Method chaining support - return self for fluent API
    #[pyo3(signature = (content, class_name=None))]
    fn text(&mut self, content: &str, class_name: Option<&str>) -> PyResult<()> {
        self.add_text(content, class_name)
    }

    /// Method chaining support - return self for fluent API
    #[pyo3(signature = (text, variant=None, class_name=None))]
    fn button(
        &mut self,
        text: &str,
        variant: Option<PyButtonVariant>,
        class_name: Option<&str>,
    ) -> PyResult<()> {
        self.add_button(text, variant, class_name)
    }
}

/// Higher-level API function for creating apps
#[pyfunction]
fn create_app() -> PyResult<App> {
    App::new()
}

/// Get the mkui version
#[pyfunction]
fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Convenience function that mimics the Rust mkui::run! macro
#[pyfunction]
fn run_showcase() -> PyResult<()> {
    // Import the common showcase
    match showcase_common::create_showcase_ui() {
        Ok(app) => match app.run() {
            Ok(_) => Ok(()),
            Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
                "Showcase run failed: {}",
                e
            ))),
        },
        Err(e) => Err(PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(format!(
            "Failed to create showcase: {}",
            e
        ))),
    }
}

/// Python module definition
#[pymodule]
fn mkui_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<App>()?;
    m.add_class::<PyButtonVariant>()?;
    m.add_class::<PyMkuiError>()?;
    m.add_function(wrap_pyfunction!(create_app, m)?)?;
    m.add_function(wrap_pyfunction!(version, m)?)?;
    m.add_function(wrap_pyfunction!(run_showcase, m)?)?;

    // Add button variant constants
    m.add("BUTTON_PRIMARY", PyButtonVariant::Primary)?;
    m.add("BUTTON_SECONDARY", PyButtonVariant::Secondary)?;
    m.add("BUTTON_DESTRUCTIVE", PyButtonVariant::Destructive)?;
    m.add("BUTTON_OUTLINE", PyButtonVariant::Outline)?;
    m.add("BUTTON_GHOST", PyButtonVariant::Ghost)?;
    m.add("BUTTON_LINK", PyButtonVariant::Link)?;

    Ok(())
}
