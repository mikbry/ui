use crate::app::WebApp;
use crate::render::{WebRenderable, WebRendererRegistry};
use mkui_core::components::*;
use mkui_core::headless::ButtonVariant;
use mkui_core::theme::{ColorTheme, ThemeMode};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use web_sys::{Document, Element};

/// High-level web app entry point.
///
/// `Mkui` holds the component tree plus the [`WebRendererRegistry`] that
/// turns each component into DOM. The registry starts with renderers for
/// the built-in `mkui-core` components and can be extended for product
/// components via [`Mkui::register`] without editing `mkui-web` itself.
pub struct Mkui {
    app: Rc<RefCell<WebApp>>,
    children: Vec<Box<dyn Component>>,
    registry: WebRendererRegistry,
}

impl Mkui {
    pub fn new() -> Result<Self, JsValue> {
        let app = Rc::new(RefCell::new(WebApp::new("app")?));
        Ok(Self {
            app,
            children: Vec::new(),
            registry: WebRendererRegistry::with_defaults(),
        })
    }

    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Register a custom component type with the underlying
    /// [`WebRendererRegistry`]. The type must implement [`WebRenderable`].
    pub fn register<T: WebRenderable + 'static>(mut self) -> Self {
        self.registry.register::<T>();
        self
    }

    /// Install a deliberate fallback handler invoked when a component
    /// reaches the renderer without a registered handler. Without a
    /// fallback, missing handlers panic in debug builds and return a
    /// `JsValue` error in release.
    pub fn fallback<F>(mut self, f: F) -> Self
    where
        F: Fn(&dyn Component, &Document, &WebRendererRegistry) -> Result<Element, JsValue>
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

        // Create main wrapper
        let wrapper = document.create_element("div")?;
        wrapper.set_class_name("min-h-screen flex flex-col bg-background text-foreground");

        for child in &self.children {
            let element = self.registry.render(child.as_ref(), &document)?;
            wrapper.append_child(&element)?;
        }

        // Append to app root
        self.app.borrow().renderer().append_child(&wrapper)?;

        Ok(())
    }
}

/// Theme selector component shipped with mkui-web. Implemented as a
/// [`WebRenderable`] so it dispatches through the same registry path as
/// user-defined components.
pub struct ThemeSelector {
    app: Rc<RefCell<WebApp>>,
}

impl ThemeSelector {
    pub fn new(app: Rc<RefCell<WebApp>>) -> Self {
        Self { app }
    }
}

impl Component for ThemeSelector {}

impl WebRenderable for ThemeSelector {
    fn render_web(
        &self,
        document: &Document,
        registry: &WebRendererRegistry,
    ) -> Result<Element, JsValue> {
        let section = document.create_element("div")?;
        section.set_class_name("rounded-lg border bg-card text-card-foreground shadow-sm p-6");

        // Header
        let header = document.create_element("div")?;
        header.set_class_name("mb-6");

        let title = document.create_element("h2")?;
        title.set_class_name("text-2xl font-semibold leading-none tracking-tight");
        title.set_text_content(Some("Theme Customization"));
        header.append_child(&title)?;

        let desc = document.create_element("p")?;
        desc.set_class_name("text-sm text-muted-foreground mt-2");
        desc.set_text_content(Some("Choose your preferred theme mode and color scheme"));
        header.append_child(&desc)?;

        section.append_child(&header)?;

        // Theme mode section
        let mode_section = document.create_element("div")?;
        mode_section.set_class_name("space-y-2 mb-6");

        let mode_label = document.create_element("label")?;
        mode_label.set_class_name("text-sm font-medium leading-none");
        mode_label.set_text_content(Some("Theme Mode"));
        mode_section.append_child(&mode_label)?;

        let mode_container = document.create_element("div")?;
        mode_container.set_class_name("flex flex-wrap gap-4");

        let create_mode_button = |text: &str, mode: ThemeMode| -> Result<Element, JsValue> {
            let is_active = self.app.borrow().get_theme_mode() == mode;
            let variant = if is_active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Outline
            };

            let app_clone = Rc::clone(&self.app);
            let button = Button::new(text)
                .variant(variant)
                .class("h-9 px-4 py-2")
                .on_press(move || {
                    let _ = app_clone.borrow_mut().set_theme_mode(mode);
                    web_sys::window().unwrap().location().reload().ok();
                });

            registry.render(&button, document)
        };

        mode_container.append_child(&create_mode_button("Light", ThemeMode::Light)?.into())?;
        mode_container.append_child(&create_mode_button("Dark", ThemeMode::Dark)?.into())?;
        mode_container.append_child(&create_mode_button("System", ThemeMode::System)?.into())?;
        mode_section.append_child(&mode_container)?;
        section.append_child(&mode_section)?;

        // Color theme section
        let color_section = document.create_element("div")?;
        color_section.set_class_name("space-y-2");

        let color_label = document.create_element("label")?;
        color_label.set_class_name("text-sm font-medium leading-none");
        color_label.set_text_content(Some("Color Theme"));
        color_section.append_child(&color_label)?;

        let color_grid = document.create_element("div")?;
        color_grid.set_class_name("grid grid-cols-3 sm:grid-cols-4 md:grid-cols-6 gap-4");

        let create_color_button = |theme: ColorTheme| -> Result<Element, JsValue> {
            let theme_name = format!("{:?}", theme);
            let is_active = self.app.borrow().get_color_theme() == &theme;
            let variant = if is_active {
                ButtonVariant::Primary
            } else {
                ButtonVariant::Outline
            };

            let app_clone = Rc::clone(&self.app);
            let button = Button::new(&theme_name)
                .variant(variant)
                .class("h-6 px-3 py-1 text-xs m-1")
                .on_press(move || {
                    let _ = app_clone.borrow_mut().set_color_theme(theme.clone());
                    web_sys::window().unwrap().location().reload().ok();
                });

            registry.render(&button, document)
        };

        for theme in ColorTheme::all() {
            color_grid.append_child(&create_color_button(theme)?.into())?;
        }

        color_section.append_child(&color_grid)?;
        section.append_child(&color_section)?;

        Ok(section)
    }
}
