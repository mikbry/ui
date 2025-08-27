use mkui_core::headless::{Toggle as HeadlessToggle, ToggleBuilder, Focusable};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlInputElement, MouseEvent, KeyboardEvent};
use crate::utils::document;

pub struct WebToggle {
    inner: HeadlessToggle,
    element: Element,
    checkbox: HtmlInputElement,
}

impl WebToggle {
    pub fn new(label: impl Into<String>) -> Result<Self, JsValue> {
        let document = document();
        let label_text = label.into();
        
        // Create container div
        let container = document.create_element("div")?;
        container.set_class_name("mkui-toggle");
        
        // Create label element
        let label_elem = document.create_element("label")?;
        label_elem.set_class_name("mkui-toggle-label");
        
        // Create checkbox input
        let checkbox = document.create_element("input")?
            .dyn_into::<HtmlInputElement>()?;
        checkbox.set_type("checkbox");
        checkbox.set_class_name("mkui-toggle-input");
        
        // Create visual toggle switch
        let switch = document.create_element("span")?;
        switch.set_class_name("mkui-toggle-switch");
        
        // Create label text
        let text = document.create_element("span")?;
        text.set_class_name("mkui-toggle-text");
        text.set_text_content(Some(&label_text));
        
        // Assemble elements
        label_elem.append_child(&checkbox)?;
        label_elem.append_child(&switch)?;
        label_elem.append_child(&text)?;
        container.append_child(&label_elem)?;
        
        let inner = ToggleBuilder::new().build();
        
        Ok(Self {
            inner,
            element: container,
            checkbox,
        })
    }
    
    pub fn checked(mut self, checked: bool) -> Self {
        self.inner.set_checked(checked);
        self.checkbox.set_checked(checked);
        self.update_classes();
        self
    }
    
    pub fn element(&self) -> &Element {
        &self.element
    }
    
    pub fn attach_events(&mut self) -> Result<(), JsValue> {
        let checkbox_clone = self.checkbox.clone();
        
        // Handle click events
        let on_change = Closure::wrap(Box::new(move |_event: MouseEvent| {
            // Toggle state is handled by the browser's default checkbox behavior
        }) as Box<dyn FnMut(_)>);
        
        self.checkbox.set_onchange(Some(on_change.as_ref().unchecked_ref()));
        on_change.forget(); // Keep closure alive
        
        // Handle keyboard events
        let on_keydown = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if event.key() == " " || event.key() == "Enter" {
                event.prevent_default();
                checkbox_clone.set_checked(!checkbox_clone.checked());
            }
        }) as Box<dyn FnMut(_)>);
        
        self.checkbox.set_onkeydown(Some(on_keydown.as_ref().unchecked_ref()));
        on_keydown.forget();
        
        Ok(())
    }
    
    fn update_classes(&self) {
        let mut classes = vec!["mkui-toggle"];
        
        if self.inner.is_checked() {
            classes.push("mkui-toggle--checked");
        }
        
        if self.inner.is_disabled() {
            classes.push("mkui-toggle--disabled");
        }
        
        if self.inner.is_focused() {
            classes.push("mkui-toggle--focused");
        }
        
        self.element.set_class_name(&classes.join(" "));
    }
    
    pub fn sync_state(&mut self) {
        let checked = self.checkbox.checked();
        self.inner.set_checked(checked);
        self.update_classes();
    }
}