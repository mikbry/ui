use std::rc::Rc;
use crate::headless::{ButtonVariant, TextVariant};

/// High-level component trait for cross-platform rendering
pub trait Component: std::any::Any {
    #[cfg(feature = "web")]
    fn render_web(&self, _document: &web_sys::Document) -> Result<web_sys::Element, wasm_bindgen::JsValue> {
        unimplemented!("Web rendering not implemented for this component")
    }
    
    #[cfg(feature = "console")]
    fn render_console(&self, _theme: &crate::theme::ConsoleTheme) -> Box<dyn ratatui::widgets::Widget + '_> {
        unimplemented!("Console rendering not implemented for this component")
    }
}

/// Main app container - cross-platform
pub struct Mkui {
    children: Vec<Box<dyn Component>>,
}

impl Mkui {
    pub fn new() -> Self {
        Self {
            children: Vec::new(),
        }
    }
    
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }
    
    pub fn children(&self) -> &Vec<Box<dyn Component>> {
        &self.children
    }
}

/// Cross-platform View container
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
    
    pub fn children(&self) -> &Vec<Box<dyn Component>> {
        &self.children
    }
}

impl Component for View {}

/// Cross-platform Text component
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

/// Cross-platform Button component
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
    
    pub fn on_press<F>(mut self, handler: F) -> Self 
    where 
        F: Fn() + 'static 
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