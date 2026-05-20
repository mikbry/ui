//! High-level [`Mkui`] entry point for the console backend.
//!
//! Mirrors the shape of [`mkui_web::high_level::Mkui`] and
//! [`mkui_wgpu::high_level::Mkui`]: build a tree of [`mkui_core::components`]
//! with `.child(...)`, then call `.run()` to draw it. Internally this
//! delegates to the [`crate::app::ConsoleApp`] state, the
//! [`crate::renderer::ConsoleRenderer`] output surface, and the
//! [`crate::components::walk_component`] tree walker.
//!
//! Styling decisions come from the *typed* [`TextVariant`] /
//! [`ButtonVariant`] values on each component — the backend never inspects
//! showcase-specific class strings.

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    style::Stylize,
};
use mkui_core::components::Component;
use mkui_core::headless::ButtonVariant;

use crate::app::ConsoleApp;
use crate::components::{walk_component, ConsoleButton, Line};
use crate::renderer::ConsoleRenderer;

/// Console backend for the shared `mkui-core` component tree.
///
/// The renderer walks any `Box<dyn Component>` tree built from `mkui-core` —
/// the same model the web and native backends consume — and produces a
/// terminal rendering. It does not know about showcase-specific class
/// strings or layouts; styling comes from the typed `TextVariant` /
/// `ButtonVariant` values on each component.
pub struct Mkui {
    app: ConsoleApp,
    children: Vec<Box<dyn Component>>,
    layout: Vec<Line>,
    buttons: Vec<ConsoleButton>,
}

impl Mkui {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            app: ConsoleApp::new()?,
            children: Vec::new(),
            layout: Vec::new(),
            buttons: Vec::new(),
        })
    }

    /// Matches the web `Mkui` API: append a component to the tree.
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn run(mut self) -> std::io::Result<()> {
        self.build_layout();

        if self.buttons.is_empty() {
            // No interactive controls — render once without the
            // "Press q/Esc" footer and return. The footer would advertise
            // a key the program never reads.
            return self.render(false);
        }

        self.app.renderer().enter_alt_screen()?;

        loop {
            let current_size = self.app.renderer().size();
            if current_size != self.app.last_terminal_size() {
                self.app.set_last_terminal_size(current_size);
                self.app.renderer().clear_all()?;
            }

            self.render(true)?;

            let evt = event::read()?;
            match evt {
                Event::Key(KeyEvent { code, .. }) => match code {
                    KeyCode::Left | KeyCode::Up => {
                        self.app.select_prev(self.buttons.len());
                    }
                    KeyCode::Right | KeyCode::Down => {
                        self.app.select_next(self.buttons.len());
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(button) = self.buttons.get(self.app.selected_button()) {
                            if let Some(handler) = &button.on_press {
                                handler();
                            }
                        }
                    }
                    KeyCode::Char('q') | KeyCode::Esc => break,
                    _ => {}
                },
                Event::Resize(_, _) => continue,
                _ => {}
            }
        }

        self.app.renderer().leave_alt_screen()?;
        Ok(())
    }

    fn build_layout(&mut self) {
        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        for child in &self.children {
            walk_component(child.as_ref(), &mut layout, &mut buttons);
        }
        self.layout = layout;
        self.buttons = buttons;
    }

    fn render(&self, interactive: bool) -> std::io::Result<()> {
        let renderer: &ConsoleRenderer = self.app.renderer();
        let (width, height) = renderer.size();
        let width = width as usize;
        let height = height as usize;
        let (last_w, last_h) = self.app.last_terminal_size();

        let clear_width = width.max(last_w as usize);
        let clear_height = height.max(last_h as usize);
        renderer.paint_blank(clear_width, clear_height)?;

        let mut current_row: u16 = 2;
        for line in &self.layout {
            match line {
                Line::Heading(text) => {
                    renderer.print_styled(2, current_row, text.clone().white().bold())?;
                    current_row += 1;
                }
                Line::Body(text) => {
                    renderer.print_styled(2, current_row, text.clone().white())?;
                    current_row += 1;
                }
                Line::Muted(text) => {
                    renderer.print_styled(2, current_row, text.clone().dark_grey())?;
                    current_row += 1;
                }
                Line::Spacer => current_row += 1,
                Line::Button(index) => {
                    if let Some(button) = self.buttons.get(*index) {
                        let is_selected = self.app.selected_button() == *index;
                        let text = format!("[ {} ]", button.label);
                        match (is_selected, &button.variant) {
                            (true, ButtonVariant::Primary) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_blue().bold(),
                            )?,
                            (true, ButtonVariant::Secondary) => renderer.print_styled(
                                4,
                                current_row,
                                text.black().on_grey().bold(),
                            )?,
                            (true, ButtonVariant::Destructive) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_red().bold(),
                            )?,
                            (true, ButtonVariant::Outline) => {
                                renderer.print_styled(4, current_row, text.blue().bold())?
                            }
                            (true, ButtonVariant::Ghost) => {
                                renderer.print_styled(4, current_row, text.blue().bold())?
                            }
                            (true, ButtonVariant::Link) => renderer.print_styled(
                                4,
                                current_row,
                                text.blue().bold().underlined(),
                            )?,
                            (false, ButtonVariant::Primary) => {
                                renderer.print_styled(4, current_row, text.white().on_blue())?
                            }
                            (false, ButtonVariant::Secondary) => {
                                renderer.print_styled(4, current_row, text.dark_grey())?
                            }
                            (false, ButtonVariant::Destructive) => {
                                renderer.print_styled(4, current_row, text.white().on_dark_red())?
                            }
                            (false, ButtonVariant::Outline) => {
                                renderer.print_styled(4, current_row, text.white())?
                            }
                            (false, ButtonVariant::Ghost) => {
                                renderer.print_styled(4, current_row, text.dark_grey())?
                            }
                            (false, ButtonVariant::Link) => {
                                renderer.print_styled(4, current_row, text.white().underlined())?
                            }
                        }
                        current_row += 1;
                    }
                }
            }
        }

        if interactive {
            let instructions = "↑↓/←→: Select button | Space/Enter: Click | q: Quit";
            let instr_y = (height.saturating_sub(1)).min(current_row as usize + 2);
            renderer.print_footer(instr_y as u16, instructions)?;
        }

        renderer.flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::{Button as CoreButton, Text, View};
    use mkui_core::headless::TextVariant;

    #[test]
    fn build_layout_renders_a_realistic_showcase_tree() {
        // Smoke test for the backend path: a small `mkui-core` tree
        // containing a heading, a button, and nested views flows through
        // `build_layout` and produces a non-trivial layout + button list.
        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(
            View::new()
                .class("container")
                .child(Text::new("Hello").variant(TextVariant::Heading1))
                .child(Text::new("subtitle").variant(TextVariant::Caption))
                .child(
                    View::new()
                        .class("row")
                        .child(CoreButton::new("Primary").variant(ButtonVariant::Primary))
                        .child(CoreButton::new("Ghost").variant(ButtonVariant::Ghost)),
                ),
        );

        app.build_layout();

        assert_eq!(app.buttons.len(), 2);
        assert_eq!(app.buttons[0].label, "Primary");
        assert_eq!(app.buttons[1].label, "Ghost");
        assert!(matches!(app.buttons[0].variant, ButtonVariant::Primary));

        let button_lines: Vec<&Line> = app
            .layout
            .iter()
            .filter(|l| matches!(l, Line::Button(_)))
            .collect();
        assert_eq!(button_lines.len(), 2);
        assert_eq!(*button_lines[0], Line::Button(0));
        assert_eq!(*button_lines[1], Line::Button(1));

        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Heading(t) if t == "Hello")));
        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Muted(t) if t == "subtitle")));
    }

    #[test]
    fn child_components_are_stored_on_the_console_mkui() {
        // Guard: if the bridge ever drops children silently the console
        // backend would render a blank screen with no panic — make that a
        // test failure instead.
        let app = Mkui::new()
            .expect("Mkui::new should succeed without a real TTY")
            .child(Text::new("a"))
            .child(View::new().child(CoreButton::new("ok")));

        assert_eq!(app.children.len(), 2);
    }
}
