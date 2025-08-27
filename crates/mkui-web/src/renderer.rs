use wasm_bindgen::prelude::*;
use web_sys::Element;
use crate::utils::document;

pub struct WebRenderer {
    root: Element,
}

impl WebRenderer {
    pub fn new(root_id: &str) -> Result<Self, JsValue> {
        let document = document();
        let root = document
            .get_element_by_id(root_id)
            .ok_or_else(|| JsValue::from_str(&format!("Element with id '{}' not found", root_id)))?;
        
        Ok(Self { root })
    }
    
    pub fn root(&self) -> &Element {
        &self.root
    }
    
    pub fn clear(&self) {
        self.root.set_inner_html("");
    }
    
    pub fn append_child(&self, child: &Element) -> Result<(), JsValue> {
        self.root.append_child(child)?;
        Ok(())
    }
}