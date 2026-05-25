//! High-level [`Mkui`] entry point for the console backend.
//!
//! Sprint 4: the backend now owns an `mkui_core::Mkui` (which itself wraps
//! an `mkui_runtime::AppTree`). The navigation loop reads the runtime tree,
//! looks up actions through the tree's `ActionRegistry`, and fires them on
//! Enter/Space — closures register dirty bits via `RuntimeCtx`, the renderer
//! observes them.

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    style::Stylize,
};
use mkui_core::components::Component;
use mkui_runtime::ButtonVariant;

use crate::app::ConsoleApp;
use crate::components::{walk_tree, ConsoleButton, Line};
use crate::renderer::ConsoleRenderer;

/// Console backend for the shared `mkui-core` component tree.
pub struct Mkui {
    app: ConsoleApp,
    core: mkui_core::components::Mkui,
    layout: Vec<Line>,
    buttons: Vec<ConsoleButton>,
}

impl Mkui {
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            app: ConsoleApp::new()?,
            core: mkui_core::components::Mkui::new(),
            layout: Vec::new(),
            buttons: Vec::new(),
        })
    }

    /// Matches the web `Mkui` API: append a component to the tree.
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.core = self.core.child(child);
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
                            if let Some(action_id) = button.on_press {
                                // Fire through the tree's action registry —
                                // any dirty signal the closure emits will
                                // surface on the next frame's redraw path.
                                self.core.tree_mut();
                                self.core.tree().actions().fire(action_id);
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
        walk_tree(self.core.tree(), &mut layout, &mut buttons);
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
                        match (is_selected, button.variant) {
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
                            // `ButtonVariant` is `#[non_exhaustive]` — future
                            // variants fall back to the Primary style until
                            // their own arm lands.
                            (true, _) => renderer.print_styled(
                                4,
                                current_row,
                                text.white().on_blue().bold(),
                            )?,
                            (false, _) => {
                                renderer.print_styled(4, current_row, text.white().on_blue())?
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
        let mut app = Mkui::new().expect("init mkui-console");
        app = app.child(
            View::new()
                .child(Text::new("Hello").variant(TextVariant::Heading1))
                .child(Text::new("subtitle").variant(TextVariant::Caption))
                .child(
                    View::new()
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

        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Heading(t) if t == "Hello")));
        assert!(app
            .layout
            .iter()
            .any(|l| matches!(l, Line::Muted(t) if t == "subtitle")));
    }
}
