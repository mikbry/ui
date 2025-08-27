use mkui_core::headless::{Button as HeadlessButton, ButtonBuilder, ButtonVariant, ButtonSize};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlButtonElement, MouseEvent, KeyboardEvent};
use crate::utils::document;

pub struct WebButton {
    inner: HeadlessButton,
    element: HtmlButtonElement,
}

impl WebButton {
    pub fn new(text: impl Into<String>) -> Result<Self, JsValue> {
        let document = document();
        let text_str = text.into();
        
        // Create button element
        let button = document.create_element("button")?
            .dyn_into::<HtmlButtonElement>()?;
        
        button.set_type("button");
        button.set_text_content(Some(&text_str));
        
        let inner = ButtonBuilder::new()
            .text(&text_str)
            .build();
        
        let web_button = Self {
            inner,
            element: button,
        };
        
        web_button.update_classes();
        Ok(web_button)
    }
    
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(variant)
            .size(self.inner.size().clone())
            .build();
        self.update_classes();
        self
    }
    
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(self.inner.variant().clone())
            .size(size)
            .build();
        self.update_classes();
        self
    }
    
    pub fn on_click<F: Fn() + 'static>(mut self, f: F) -> Self {
        self.inner = ButtonBuilder::new()
            .text(self.inner.text())
            .variant(self.inner.variant().clone())
            .size(self.inner.size().clone())
            .on_click(f)
            .build();
        self
    }
    
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.inner.set_disabled(disabled);
        self.element.set_disabled(disabled);
        self.update_classes();
        self
    }
    
    pub fn loading(mut self, loading: bool) -> Self {
        self.inner.set_loading(loading);
        self.update_classes();
        self
    }
    
    pub fn element(&self) -> &Element {
        self.element.as_ref()
    }
    
    pub fn attach_events(&mut self) -> Result<(), JsValue> {
        // Handle click events
        let on_click = Closure::wrap(Box::new(move |_event: MouseEvent| {
            // For now, just log - we'll need a different approach for callbacks
            web_sys::console::log_1(&"Button clicked!".into());
        }) as Box<dyn FnMut(_)>);
        
        self.element.set_onclick(Some(on_click.as_ref().unchecked_ref()));
        on_click.forget();
        
        // Handle keyboard events
        let on_keydown = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if event.key() == " " || event.key() == "Enter" {
                event.prevent_default();
                web_sys::console::log_1(&"Button activated via keyboard!".into());
            }
        }) as Box<dyn FnMut(_)>);
        
        self.element.set_onkeydown(Some(on_keydown.as_ref().unchecked_ref()));
        on_keydown.forget();
        
        Ok(())
    }
    
    fn update_classes(&self) {
        // Generate Basecoat-compatible CSS classes
        let mut classes = Vec::new();
        
        classes.push("btn");
        // Base variant class
        let variant_class = match self.inner.variant() {
            ButtonVariant::Primary => "btn-primary", // Default variant
            ButtonVariant::Secondary => "btn-secondary",
            ButtonVariant::Destructive => "btn-destructive",
            ButtonVariant::Outline => "btn-outline",
            ButtonVariant::Ghost => "btn-ghost",
            ButtonVariant::Link => "btn-link",
        };
        classes.push(variant_class);
        
        // Size modifiers
        match self.inner.size() {
            ButtonSize::Small => classes.push("btn-sm"),
            ButtonSize::Medium => classes.push("btn-md"), // Default size
            ButtonSize::Large => classes.push("btn-lg"),
        }
        
        // State classes
        if self.inner.is_loading() {
            // Add loading state class if needed
        }
        
        self.element.set_class_name(&classes.join(" "));
        
        // Update content for loading state
        if self.inner.is_loading() {
            self.element.set_text_content(Some(&format!("⏳ {}", self.inner.text())));
        } else {
            self.element.set_text_content(Some(self.inner.text()));
        }
    }
    
    pub fn click(&mut self) {
        self.inner.click();
    }
}