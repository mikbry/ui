//! Browser fixture for issue #278 — a side-by-side comparison of the
//! production `mkui-web` DOM renderer against a pinned, hand-vendored
//! shadcn/ui reference (`index.html` + `shadcn.html`, see README.md).
//!
//! `card()` and `buttons()` build the shared `mkui-core` component tree used
//! by both the host-side contract test below (`cargo test --workspace`,
//! no browser) and the `#[wasm_bindgen(start)]` mount further down. Keeping
//! the fixture as plain functions over `mkui_core::prelude::Mkui` lets the
//! host test assert node kinds/variants/disabled flags without a wasm
//! runtime, while `start()` feeds the identical shape into `mkui_web::Mkui`
//! to produce the actual DOM `shadcn.html`'s hand-written markup is
//! compared against.
#[cfg(any(test, target_arch = "wasm32"))]
use mkui_core::prelude::*;

/// Card: header (title + description), content, and a two-button footer —
/// the shadcn `Card`/`CardHeader`/`CardTitle`/`CardDescription`/
/// `CardContent`/`CardFooter` slots, mirrored in `shadcn.html`.
#[cfg(any(test, target_arch = "wasm32"))]
fn card() -> Card {
    Card::new()
        .class("parity-card")
        .child(
            View::new()
                .class("parity-card-header")
                .child(Text::new("Team members").class("parity-card-title"))
                .child(
                    Text::new("Invite teammates to collaborate.").class("parity-card-description"),
                ),
        )
        .child(Text::new("3 members · 2 pending invites").class("parity-card-content"))
        .child(
            View::new()
                .class("parity-card-footer")
                .child(
                    Button::new("Cancel")
                        .variant(ButtonVariant::Outline)
                        .class("parity-btn"),
                )
                .child(
                    Button::new("Invite")
                        .variant(ButtonVariant::Primary)
                        .class("parity-btn"),
                ),
        )
}

/// Button row covering the four states #278 asks for: default, outline,
/// disabled, and a fourth button both `mkui.html` and `shadcn.html` give
/// programmatic keyboard focus on load so every screenshot already shows
/// the `:focus-visible` ring (see README.md's screenshot workflow).
#[cfg(any(test, target_arch = "wasm32"))]
fn buttons() -> View {
    View::new()
        .class("parity-button-row")
        .child(
            Button::new("Default")
                .variant(ButtonVariant::Primary)
                .class("parity-btn"),
        )
        .child(
            Button::new("Outline")
                .variant(ButtonVariant::Outline)
                .class("parity-btn"),
        )
        .child(
            Button::new("Disabled")
                .variant(ButtonVariant::Primary)
                .disabled(true)
                .class("parity-btn"),
        )
        .child(
            Button::new("Keyboard focus")
                .variant(ButtonVariant::Primary)
                .class("parity-btn parity-btn-focus-target"),
        )
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{buttons, card};
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(start)]
    pub fn start() -> Result<(), JsValue> {
        let document = web_sys::window().unwrap().document().unwrap();
        let style = document.create_element("style")?;
        style.set_text_content(Some(mkui_web::DEFAULT_COMPONENT_CSS));
        document.head().unwrap().append_child(&style)?;
        mkui_web::Mkui::new()?.child(card()).child(buttons()).run()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_runtime::NodeKind;

    fn fixture() -> Mkui {
        Mkui::new().child(card()).child(buttons())
    }

    #[test]
    fn fixture_has_one_card_and_six_buttons() {
        let app = fixture();
        let mut cards = 0;
        let mut button_count = 0;
        for node in app.tree().nodes() {
            match &node.kind {
                NodeKind::Card(_) => cards += 1,
                NodeKind::Button(_) => button_count += 1,
                _ => {}
            }
        }
        assert_eq!(cards, 1, "exactly one Card node");
        assert_eq!(button_count, 6, "card footer (2) + button row (4)");
    }

    #[test]
    fn exactly_one_button_is_disabled() {
        let app = fixture();
        let disabled_count = app
            .tree()
            .nodes()
            .filter(|node| matches!(&node.kind, NodeKind::Button(b) if b.disabled))
            .count();
        assert_eq!(disabled_count, 1, "only the 'Disabled' button is disabled");
    }

    #[test]
    fn button_row_covers_default_outline_disabled_and_focus_states() {
        let app = fixture();
        let labels: Vec<(&str, ButtonVariant, bool)> = app
            .tree()
            .nodes()
            .filter_map(|node| match &node.kind {
                NodeKind::Button(b) => Some((b.label.as_str(), b.variant, b.disabled)),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&("Default", ButtonVariant::Primary, false)));
        assert!(labels.contains(&("Outline", ButtonVariant::Outline, false)));
        assert!(labels.contains(&("Disabled", ButtonVariant::Primary, true)));
        assert!(labels.contains(&("Keyboard focus", ButtonVariant::Primary, false)));
    }

    #[test]
    fn card_footer_has_an_outline_cancel_and_primary_invite_action() {
        let app = fixture();
        let labels: Vec<(&str, ButtonVariant)> = app
            .tree()
            .nodes()
            .filter_map(|node| match &node.kind {
                NodeKind::Button(b) => Some((b.label.as_str(), b.variant)),
                _ => None,
            })
            .collect();
        assert!(labels.contains(&("Cancel", ButtonVariant::Outline)));
        assert!(labels.contains(&("Invite", ButtonVariant::Primary)));
    }
}
