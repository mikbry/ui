//! Web-side component wrappers.
//!
//! Each backend turns the shared [`mkui_core::components`] tree into its own
//! drawable form. For the browser that means concrete `web_sys::Element`
//! wrappers ([`WebButton`], [`WebToggle`]) that pair the headless logic from
//! [`mkui_core::headless`] with DOM construction and event wiring.
//!
//! This module is the web counterpart of `mkui_console::components` and
//! `mkui_wgpu::components` — backend-specific component scaffolding that
//! sits above [`mkui_core`] and below the high-level [`crate::Mkui`]
//! orchestrator.

use crate::utils::document;
use mkui_core::headless::{
    Button as HeadlessButton, ButtonBuilder, ButtonSize, ButtonVariant, Focusable,
    Toggle as HeadlessToggle, ToggleBuilder,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlButtonElement, HtmlInputElement, KeyboardEvent, MouseEvent};

/// Web-side button: pairs the headless [`HeadlessButton`] logic with a
/// concrete `<button>` element and Basecoat-compatible class names.
pub struct WebButton {
    inner: HeadlessButton,
    element: HtmlButtonElement,
}

impl WebButton {
    pub fn new(text: impl Into<String>) -> Result<Self, JsValue> {
        let document = document();
        let text_str = text.into();

        let button = document
            .create_element("button")?
            .dyn_into::<HtmlButtonElement>()?;

        button.set_type("button");
        button.set_text_content(Some(&text_str));

        let inner = ButtonBuilder::new().text(&text_str).build();

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
        let on_click = Closure::wrap(Box::new(move |_event: MouseEvent| {
            web_sys::console::log_1(&"Button clicked!".into());
        }) as Box<dyn FnMut(_)>);

        self.element
            .set_onclick(Some(on_click.as_ref().unchecked_ref()));
        on_click.forget();

        let on_keydown = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if event.key() == " " || event.key() == "Enter" {
                event.prevent_default();
                web_sys::console::log_1(&"Button activated via keyboard!".into());
            }
        }) as Box<dyn FnMut(_)>);

        self.element
            .set_onkeydown(Some(on_keydown.as_ref().unchecked_ref()));
        on_keydown.forget();

        Ok(())
    }

    fn update_classes(&self) {
        let mut classes = Vec::new();

        classes.push("btn");
        let variant_class = match self.inner.variant() {
            ButtonVariant::Primary => "btn-primary",
            ButtonVariant::Secondary => "btn-secondary",
            ButtonVariant::Destructive => "btn-destructive",
            ButtonVariant::Outline => "btn-outline",
            ButtonVariant::Ghost => "btn-ghost",
            ButtonVariant::Link => "btn-link",
            _ => "btn-primary",
        };
        classes.push(variant_class);

        match self.inner.size() {
            ButtonSize::Small => classes.push("btn-sm"),
            ButtonSize::Medium => classes.push("btn-md"),
            ButtonSize::Large => classes.push("btn-lg"),
            _ => classes.push("btn-md"),
        }

        self.element.set_class_name(&classes.join(" "));

        if self.inner.is_loading() {
            self.element
                .set_text_content(Some(&format!("⏳ {}", self.inner.text())));
        } else {
            self.element.set_text_content(Some(self.inner.text()));
        }
    }

    pub fn click(&mut self) {
        self.inner.click();
    }
}

/// Web-side toggle: pairs the headless [`HeadlessToggle`] logic with a
/// real `<input type="checkbox">` element and accessible label layout.
pub struct WebToggle {
    inner: HeadlessToggle,
    element: Element,
    checkbox: HtmlInputElement,
}

impl WebToggle {
    pub fn new(label: impl Into<String>) -> Result<Self, JsValue> {
        let document = document();
        let label_text = label.into();

        let container = document.create_element("div")?;
        container.set_class_name("mkui-toggle");

        let label_elem = document.create_element("label")?;
        label_elem.set_class_name("mkui-toggle-label");

        let checkbox = document
            .create_element("input")?
            .dyn_into::<HtmlInputElement>()?;
        checkbox.set_type("checkbox");
        checkbox.set_class_name("mkui-toggle-input");

        let switch = document.create_element("span")?;
        switch.set_class_name("mkui-toggle-switch");

        let text = document.create_element("span")?;
        text.set_class_name("mkui-toggle-text");
        text.set_text_content(Some(&label_text));

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

        let on_change = Closure::wrap(Box::new(move |_event: MouseEvent| {
            // Toggle state is handled by the browser's default checkbox behavior
        }) as Box<dyn FnMut(_)>);

        self.checkbox
            .set_onchange(Some(on_change.as_ref().unchecked_ref()));
        on_change.forget();

        let on_keydown = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            if event.key() == " " || event.key() == "Enter" {
                event.prevent_default();
                checkbox_clone.set_checked(!checkbox_clone.checked());
            }
        }) as Box<dyn FnMut(_)>);

        self.checkbox
            .set_onkeydown(Some(on_keydown.as_ref().unchecked_ref()));
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
