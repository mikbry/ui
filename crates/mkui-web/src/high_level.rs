use mkui_core::components::*;
use mkui_core::headless::ButtonVariant;
use wasm_bindgen::prelude::*;
use std::rc::Rc;
use std::cell::RefCell;
use crate::app::WebApp;
use mkui_core::theme::{ThemeMode, ColorTheme};
use crate::button::WebButton;

// Local trait for web rendering to avoid orphan rule issues
trait WebRenderable {
    fn render_to_web(&self, document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue>;
}

// Implementations for core components
impl WebRenderable for View {
    fn render_to_web(&self, document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
        let element = document.create_element("div")?;
        element.set_class_name(self.class_name());
        
        for child in self.children() {
            let child_element = render_component_to_web(child.as_ref(), document)?;
            element.append_child(&child_element)?;
        }
        
        Ok(element)
    }
}

impl WebRenderable for Text {
    fn render_to_web(&self, document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
        let element = document.create_element("p")?;
        element.set_class_name(self.class_name());
        element.set_text_content(Some(self.content()));
        Ok(element)
    }
}

impl WebRenderable for Button {
    fn render_to_web(&self, _document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
        let mut web_button = WebButton::new(self.content())?
            .variant(self.button_variant().clone());
        
        web_button.attach_events()?;
        
        let element = web_button.element().clone();
        
        // Add custom classes if provided
        if !self.class_name().is_empty() {
            let current_classes = element.class_name();
            element.set_class_name(&format!("{} {}", current_classes, self.class_name()));
        }
        
        // Add click handler if provided
        if let Some(handler) = self.on_press_handler() {
            let handler = Rc::clone(handler);
            let closure = wasm_bindgen::closure::Closure::wrap(Box::new(move |_event: web_sys::Event| {
                handler();
            }) as Box<dyn FnMut(_)>);
            
            element.dyn_ref::<web_sys::HtmlElement>().unwrap().set_onclick(Some(closure.as_ref().unchecked_ref()));
            closure.forget();
        }
        
        Ok(element)
    }
}

// Helper function to render any Component to web
fn render_component_to_web(component: &dyn Component, document: &web_sys::Document) -> Result<web_sys::Element, JsValue> {
    // We need to downcast to concrete types since we can't implement traits on external types
    // This is a workaround for the orphan rule
    
    if let Some(view) = (component as &dyn std::any::Any).downcast_ref::<View>() {
        return view.render_to_web(document);
    }
    
    if let Some(text) = (component as &dyn std::any::Any).downcast_ref::<Text>() {
        return text.render_to_web(document);
    }
    
    if let Some(button) = (component as &dyn std::any::Any).downcast_ref::<Button>() {
        return button.render_to_web(document);
    }
    
    if let Some(theme_selector) = (component as &dyn std::any::Any).downcast_ref::<ThemeSelector>() {
        return theme_selector.render_web(document);
    }
    
    // Fallback for unknown component types
    let placeholder = document.create_element("div")?;
    placeholder.set_text_content(Some("Unsupported Component"));
    Ok(placeholder)
}

// High-level mkui-style components with web rendering
pub struct Mkui {
    app: Rc<RefCell<WebApp>>,
    children: Vec<Box<dyn Component>>,
}

impl Mkui {
    pub fn new() -> Result<Self, JsValue> {
        let app = Rc::new(RefCell::new(WebApp::new("app")?));
        Ok(Self {
            app,
            children: Vec::new(),
        })
    }
    
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
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
        
        // Render all children using the proper rendering system
        for child in &self.children {
            let element = render_component_to_web(child.as_ref(), &document)?;
            wrapper.append_child(&element)?;
        }
        
        // Append to app root
        self.app.borrow().renderer().append_child(&wrapper)?;
        
        Ok(())
    }
}

// TODO: Implement proper component rendering
// The orphan rule prevents implementing Component for external types directly
// This needs to be redesigned with a different architecture

// Theme selector component
pub struct ThemeSelector {
    app: Rc<RefCell<WebApp>>,
}

impl ThemeSelector {
    pub fn new(app: Rc<RefCell<WebApp>>) -> Self {
        Self { app }
    }
}

impl Component for ThemeSelector {
}

// Temporarily implement a custom render method
impl ThemeSelector {
    pub fn render_web(&self, document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
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
        
        // Theme mode buttons using proper Button components
        let create_mode_button = |text: &str, mode: ThemeMode| -> Result<web_sys::Element, JsValue> {
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
            
            button.render_to_web(document)
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
        
        let create_color_button = |theme: ColorTheme| -> Result<web_sys::Element, JsValue> {
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
            
            button.render_to_web(document)
        };
        
        for theme in ColorTheme::all() {
            color_grid.append_child(&create_color_button(theme)?.into())?;
        }
        
        color_section.append_child(&color_grid)?;
        section.append_child(&color_section)?;
        
        Ok(section)
    }
}