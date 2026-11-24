//! Browser smoke fixture using only the public web API and CSS recipe.
use mkui_core::components::{Button, Card, Text, View};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    let document = web_sys::window().unwrap().document().unwrap();
    let style = document.create_element("style")?;
    style.set_text_content(Some(mkui_web::DEFAULT_COMPONENT_CSS));
    document.head().unwrap().append_child(&style)?;
    mkui_web::Mkui::new()?
        .child(Card::new().child(Text::new("Default card")))
        .child(
            Card::new()
                .class("consumer-card")
                .child(Text::new("Override card")),
        )
        .child(
            View::new()
                .class("overlap")
                .child(
                    Button::new("Enabled below")
                        .class("under")
                        .on_press(|| record("enabled")),
                )
                .child(
                    Button::new("Disabled above")
                        .class("over")
                        .disabled(true)
                        .on_press(|| record("disabled")),
                ),
        )
        .run()
}

fn record(name: &str) {
    let document = web_sys::window().unwrap().document().unwrap();
    let log = document.get_element_by_id("events").unwrap();
    log.set_text_content(Some(&format!(
        "{}{};",
        log.text_content().unwrap_or_default(),
        name
    )));
}
