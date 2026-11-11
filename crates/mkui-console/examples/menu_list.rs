//! Renders a title-screen-style `MenuList` to the terminal (issue #189).
//!
//! `cargo run -p mkui-console --example menu_list`
//!
//! The list has no buttons, so `Mkui::run` takes its non-interactive,
//! single-render path (see `high_level::Mkui::run`) and prints once rather
//! than opening the alt-screen navigation loop — this example exists to
//! make the selection indicator visible, not to demonstrate live
//! navigation (that's covered by `mkui_core::headless::MenuList`'s own
//! keyboard/gamepad tests).
//!
//! `Continue` is pre-selected (`selected_index(1)`) rather than the
//! default first item, so the highlighted row in the terminal output is
//! visibly driven by that call, not just always row zero.

use mkui_console::Mkui;
use mkui_core::components::MenuList;

fn main() -> std::io::Result<()> {
    println!("mkui: rendering a MenuList (issue #189) — Continue is pre-selected\n");

    Mkui::new()?
        .child(
            MenuList::new()
                .item("New Game")
                .item("Continue")
                .item("Quit")
                .selected_index(1),
        )
        .run()
}
