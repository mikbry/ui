use showcase_common::create_showcase_ui;
use wasm_bindgen::prelude::*;

pub fn run_showcase() -> Result<(), JsValue> {
    web_sys::console::log_1(&"mkui: Creating web app...".into());
    mkui::run!(create_showcase_ui, web)
}

#[wasm_bindgen(start)]
pub fn main() {
    if let Err(e) = run_showcase() {
        web_sys::console::log_1(&format!("mkui: Error during initialization: {:?}", e).into());
    }
}
