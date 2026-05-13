use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType},
};
use mkui_core::components::{Button, Component, Text, View};
use mkui_core::headless::ButtonVariant;
use std::io::{self, stdout, Write};
use std::rc::Rc;

/// Console app with the high-level mkui interface.
///
/// The component tree is held as `Box<dyn Component>` from `mkui-core` —
/// exactly the same model the web and native backends consume.
pub struct Mkui {
    children: Vec<Box<dyn Component>>,
    buttons: Vec<ConsoleButton>,
    selected_button: usize,
    last_terminal_size: (u16, u16),
}

#[derive(Clone)]
struct ConsoleButton {
    label: String,
    variant: ButtonVariant,
    on_press: Option<Rc<dyn Fn()>>,
}

#[derive(Clone)]
enum Line {
    Heading(String),
    Body(String),
    Muted(String),
    Spacer,
    Button(usize),
}

impl Mkui {
    pub fn new() -> std::io::Result<Self> {
        let initial_size = size().unwrap_or((80, 24));
        Ok(Self {
            children: Vec::new(),
            buttons: Vec::new(),
            selected_button: 0,
            last_terminal_size: initial_size,
        })
    }

    /// Matches the web `Mkui` API: append a component to the tree.
    pub fn child(mut self, child: impl Component + 'static) -> Self {
        self.children.push(Box::new(child));
        self
    }

    pub fn run(mut self) -> std::io::Result<()> {
        let layout = self.flatten_tree();
        self.collect_buttons(&layout);

        if self.buttons.is_empty() {
            // No interactive controls — render once without the
            // "Press q/Esc" footer and return. The footer would advertise
            // a key the program never reads.
            return self.render(&layout, false);
        }

        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;

        loop {
            let current_size = size()?;
            if current_size != self.last_terminal_size {
                self.last_terminal_size = current_size;
                execute!(stdout(), Clear(ClearType::All), Clear(ClearType::Purge))?;
            }

            self.render(&layout, true)?;

            let evt = event::read()?;
            match evt {
                Event::Key(KeyEvent { code, .. }) => match code {
                    KeyCode::Left | KeyCode::Up => {
                        if self.selected_button > 0 {
                            self.selected_button -= 1;
                        } else {
                            self.selected_button = self.buttons.len() - 1;
                        }
                    }
                    KeyCode::Right | KeyCode::Down => {
                        if self.selected_button + 1 < self.buttons.len() {
                            self.selected_button += 1;
                        } else {
                            self.selected_button = 0;
                        }
                    }
                    KeyCode::Enter | KeyCode::Char(' ') => {
                        if let Some(button) = self.buttons.get(self.selected_button) {
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

        execute!(stdout(), Show, Clear(ClearType::All), ResetColor)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn flatten_tree(&self) -> Vec<Line> {
        let mut out = Vec::new();
        for child in &self.children {
            self.flatten_component(child.as_ref(), &mut out);
        }
        out
    }

    fn flatten_component(&self, component: &dyn Component, out: &mut Vec<Line>) {
        let any = component as &dyn std::any::Any;

        if let Some(view) = any.downcast_ref::<View>() {
            for child in view.children() {
                self.flatten_component(child.as_ref(), out);
            }
            return;
        }

        if let Some(text) = any.downcast_ref::<Text>() {
            let content = text.content().to_string();
            let class = text.class_name();
            let line = if class.contains("text-4xl") || class.contains("text-2xl") {
                Line::Heading(content)
            } else if class.contains("text-muted-foreground") {
                Line::Muted(content)
            } else {
                Line::Body(content)
            };
            out.push(line);
            out.push(Line::Spacer);
            return;
        }

        if let Some(_button) = any.downcast_ref::<Button>() {
            let index = self
                .buttons
                .len()
                .saturating_add(out.iter().filter(|l| matches!(l, Line::Button(_))).count());
            out.push(Line::Button(index));
        }
    }

    fn collect_buttons(&mut self, _layout: &[Line]) {
        // Walk the component tree once and pull every Button into `buttons`
        // in the same order they appear in `_layout`.
        let mut collected = Vec::new();
        for child in &self.children {
            Self::collect_buttons_in(child.as_ref(), &mut collected);
        }
        self.buttons = collected;
    }

    fn collect_buttons_in(component: &dyn Component, out: &mut Vec<ConsoleButton>) {
        let any = component as &dyn std::any::Any;
        if let Some(view) = any.downcast_ref::<View>() {
            for child in view.children() {
                Self::collect_buttons_in(child.as_ref(), out);
            }
        } else if let Some(button) = any.downcast_ref::<Button>() {
            out.push(ConsoleButton {
                label: button.content().to_string(),
                variant: button.button_variant().clone(),
                on_press: button.on_press_handler().clone(),
            });
        }
    }

    fn render(&self, layout: &[Line], interactive: bool) -> io::Result<()> {
        let (width, height) = size()?;
        let width = width as usize;
        let height = height as usize;

        let clear_width = width.max(self.last_terminal_size.0 as usize);
        let clear_height = height.max(self.last_terminal_size.1 as usize);
        for row in 0..clear_height {
            execute!(stdout(), MoveTo(0, row as u16), Print(" ".repeat(clear_width)))?;
        }
        execute!(stdout(), MoveTo(0, 0))?;

        let mut current_row: u16 = 2;
        for line in layout {
            match line {
                Line::Heading(text) => {
                    execute!(stdout(), MoveTo(2, current_row))?;
                    print!("{}", text.clone().white().bold());
                    current_row += 1;
                }
                Line::Body(text) => {
                    execute!(stdout(), MoveTo(2, current_row))?;
                    print!("{}", text.clone().white());
                    current_row += 1;
                }
                Line::Muted(text) => {
                    execute!(stdout(), MoveTo(2, current_row))?;
                    print!("{}", text.clone().dark_grey());
                    current_row += 1;
                }
                Line::Spacer => current_row += 1,
                Line::Button(index) => {
                    if let Some(button) = self.buttons.get(*index) {
                        let is_selected = self.selected_button == *index;
                        let text = format!("[ {} ]", button.label);
                        execute!(stdout(), MoveTo(4, current_row))?;
                        match (is_selected, &button.variant) {
                            (true, ButtonVariant::Primary) => print!("{}", text.white().on_blue().bold()),
                            (true, ButtonVariant::Secondary) => print!("{}", text.black().on_grey().bold()),
                            (true, ButtonVariant::Destructive) => print!("{}", text.white().on_red().bold()),
                            (true, ButtonVariant::Outline) => print!("{}", text.blue().bold()),
                            (true, ButtonVariant::Ghost) => print!("{}", text.blue().bold()),
                            (true, ButtonVariant::Link) => print!("{}", text.blue().bold().underlined()),
                            (false, ButtonVariant::Primary) => print!("{}", text.white().on_blue()),
                            (false, ButtonVariant::Secondary) => print!("{}", text.dark_grey()),
                            (false, ButtonVariant::Destructive) => print!("{}", text.white().on_dark_red()),
                            (false, ButtonVariant::Outline) => print!("{}", text.white()),
                            (false, ButtonVariant::Ghost) => print!("{}", text.dark_grey()),
                            (false, ButtonVariant::Link) => print!("{}", text.white().underlined()),
                        }
                        current_row += 1;
                    }
                }
            }
        }

        if interactive {
            let instructions = "↑↓/←→: Select button | Space/Enter: Click | q: Quit";
            let instr_y = (height.saturating_sub(1)).min(current_row as usize + 2);
            execute!(
                stdout(),
                MoveTo(0, instr_y as u16),
                SetForegroundColor(Color::DarkGrey),
                Print(instructions),
                ResetColor
            )?;
        }

        stdout().flush()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::Button as CoreButton;
    use std::cell::Cell;

    #[test]
    fn collect_buttons_in_preserves_on_press_handler() {
        let pressed = Rc::new(Cell::new(0u32));
        let pressed_in = Rc::clone(&pressed);

        let button = CoreButton::new("ok").on_press(move || {
            pressed_in.set(pressed_in.get() + 1);
        });

        let mut collected = Vec::new();
        Mkui::collect_buttons_in(&button, &mut collected);

        assert_eq!(collected.len(), 1);
        let captured = collected[0]
            .on_press
            .as_ref()
            .expect("handler must be captured, not dropped");
        captured();
        captured();

        assert_eq!(
            pressed.get(),
            2,
            "captured handler must be the same one the user supplied"
        );
    }

    #[test]
    fn collect_buttons_in_walks_nested_views() {
        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);

        let tree = View::new()
            .class("row")
            .child(View::new().child(CoreButton::new("deep").on_press(move || pressed_in.set(true))));

        let mut collected = Vec::new();
        Mkui::collect_buttons_in(&tree, &mut collected);

        assert_eq!(collected.len(), 1);
        collected[0].on_press.as_ref().expect("handler")();
        assert!(pressed.get());
    }

    #[test]
    fn collect_buttons_in_handles_buttons_without_handlers() {
        let button = CoreButton::new("no handler");
        let mut collected = Vec::new();
        Mkui::collect_buttons_in(&button, &mut collected);

        assert_eq!(collected.len(), 1);
        assert!(collected[0].on_press.is_none());
    }
}
