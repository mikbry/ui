//! Renders a title-screen-style `MenuList` to the terminal, interactively
//! (issue #189).
//!
//! `cargo run -p mkui-console --example menu_list`
//!
//! A `MenuList`-only tree now takes `Mkui::run`'s full alt-screen
//! navigation loop, not the old non-interactive single-render path: Up/
//! Down (or gamepad-equivalent input mapped to the same keys by a host)
//! move the highlighted row, Enter/Space confirms it, and `q`/Esc quits.
//! The last-confirmed item prints after quitting, proving `on_select`
//! actually fired for whichever row was highlighted at the time — not
//! just the construction-time default.
//!
//! `Continue` is pre-selected (`selected_index(1)`) rather than the
//! default first item, so the initial highlighted row is visibly driven
//! by that call, not just always row zero.

use std::cell::RefCell;
use std::rc::Rc;

use mkui_console::Mkui;
use mkui_core::components::MenuList;

const ITEMS: [&str; 3] = ["New Game", "Continue", "Quit"];

fn main() -> std::io::Result<()> {
    let chosen = Rc::new(RefCell::new(None::<&'static str>));
    let chosen_in = Rc::clone(&chosen);

    println!(
        "mkui: MenuList demo (issue #189) — \"Continue\" is pre-selected.\n\
         Up/Down to move, Enter/Space to confirm, q to quit.\n"
    );

    let mut menu = MenuList::new().selected_index(1).on_select(move |index| {
        *chosen_in.borrow_mut() = ITEMS.get(index).copied();
    });
    for item in ITEMS {
        menu = menu.item(item);
    }

    Mkui::new()?.child(menu).run()?;

    match chosen.borrow().as_ref() {
        Some(label) => println!("Last confirmed: {label}"),
        None => println!("Nothing confirmed."),
    }
    Ok(())
}
