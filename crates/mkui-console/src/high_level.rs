use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType},
};
use mkui_core::components::{Button, Component, Text, View};
use mkui_core::headless::{ButtonVariant, TextVariant};
use std::io::{self, stdout, Write};
use std::rc::Rc;

/// Console backend for the shared `mkui-core` component tree.
///
/// The renderer walks any `Box<dyn Component>` tree built from `mkui-core` —
/// the same model the web and native backends consume — and produces a
/// terminal rendering. It does not know about showcase-specific class
/// strings or layouts; styling comes from the typed `TextVariant` /
/// `ButtonVariant` values on each component.
pub struct Mkui {
    children: Vec<Box<dyn Component>>,
    layout: Vec<Line>,
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

#[derive(Clone, Debug, PartialEq)]
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
            layout: Vec::new(),
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
        self.build_layout();

        if self.buttons.is_empty() {
            // No interactive controls — render once without the
            // "Press q/Esc" footer and return. The footer would advertise
            // a key the program never reads.
            return self.render(false);
        }

        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;

        loop {
            let current_size = size()?;
            if current_size != self.last_terminal_size {
                self.last_terminal_size = current_size;
                execute!(stdout(), Clear(ClearType::All), Clear(ClearType::Purge))?;
            }

            self.render(true)?;

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

    fn build_layout(&mut self) {
        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        for child in &self.children {
            walk_component(child.as_ref(), &mut layout, &mut buttons);
        }
        self.layout = layout;
        self.buttons = buttons;
    }

    fn render(&self, interactive: bool) -> io::Result<()> {
        let (width, height) = size()?;
        let width = width as usize;
        let height = height as usize;

        let clear_width = width.max(self.last_terminal_size.0 as usize);
        let clear_height = height.max(self.last_terminal_size.1 as usize);
        for row in 0..clear_height {
            execute!(
                stdout(),
                MoveTo(0, row as u16),
                Print(" ".repeat(clear_width))
            )?;
        }
        execute!(stdout(), MoveTo(0, 0))?;

        let mut current_row: u16 = 2;
        for line in &self.layout {
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
                            (true, ButtonVariant::Primary) => {
                                print!("{}", text.white().on_blue().bold())
                            }
                            (true, ButtonVariant::Secondary) => {
                                print!("{}", text.black().on_grey().bold())
                            }
                            (true, ButtonVariant::Destructive) => {
                                print!("{}", text.white().on_red().bold())
                            }
                            (true, ButtonVariant::Outline) => print!("{}", text.blue().bold()),
                            (true, ButtonVariant::Ghost) => print!("{}", text.blue().bold()),
                            (true, ButtonVariant::Link) => {
                                print!("{}", text.blue().bold().underlined())
                            }
                            (false, ButtonVariant::Primary) => print!("{}", text.white().on_blue()),
                            (false, ButtonVariant::Secondary) => print!("{}", text.dark_grey()),
                            (false, ButtonVariant::Destructive) => {
                                print!("{}", text.white().on_dark_red())
                            }
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

/// Single-pass walk over the shared component tree.
///
/// Emits flat lines for the terminal renderer and collects interactive
/// buttons into the parallel array `Line::Button(index)` points into.
fn walk_component(
    component: &dyn Component,
    layout: &mut Vec<Line>,
    buttons: &mut Vec<ConsoleButton>,
) {
    let any = component as &dyn std::any::Any;

    if let Some(view) = any.downcast_ref::<View>() {
        for child in view.children() {
            walk_component(child.as_ref(), layout, buttons);
        }
        return;
    }

    if let Some(text) = any.downcast_ref::<Text>() {
        let content = text.content().to_string();
        let line = match text.text_variant() {
            TextVariant::Heading1 | TextVariant::Heading2 | TextVariant::Heading3 => {
                Line::Heading(content)
            }
            TextVariant::Caption | TextVariant::Label => Line::Muted(content),
            TextVariant::Body | TextVariant::Code => Line::Body(content),
        };
        layout.push(line);
        layout.push(Line::Spacer);
        return;
    }

    if let Some(button) = any.downcast_ref::<Button>() {
        let index = buttons.len();
        buttons.push(ConsoleButton {
            label: button.content().to_string(),
            variant: button.button_variant().clone(),
            on_press: button.on_press_handler().clone(),
        });
        layout.push(Line::Button(index));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mkui_core::components::Button as CoreButton;
    use std::cell::Cell;

    #[test]
    fn walk_component_preserves_on_press_handler() {
        let pressed = Rc::new(Cell::new(0u32));
        let pressed_in = Rc::clone(&pressed);

        let button = CoreButton::new("ok").on_press(move || {
            pressed_in.set(pressed_in.get() + 1);
        });

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&button, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        let captured = buttons[0]
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
    fn walk_component_recurses_into_nested_views() {
        let pressed = Rc::new(Cell::new(false));
        let pressed_in = Rc::clone(&pressed);

        let tree = View::new().class("row").child(
            View::new().child(CoreButton::new("deep").on_press(move || pressed_in.set(true))),
        );

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&tree, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        buttons[0].on_press.as_ref().expect("handler")();
        assert!(pressed.get());
    }

    #[test]
    fn walk_component_handles_buttons_without_handlers() {
        let button = CoreButton::new("no handler");
        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&button, &mut layout, &mut buttons);

        assert_eq!(buttons.len(), 1);
        assert!(buttons[0].on_press.is_none());
    }

    #[test]
    fn text_variant_drives_line_style_not_class_string() {
        // The backend must classify text by its typed `TextVariant`, not by
        // sniffing showcase-specific Tailwind class strings — that coupling
        // is what the "real component renderer" issue removes.
        let tree = View::new()
            .child(
                Text::new("title")
                    .variant(TextVariant::Heading1)
                    .class("text-4xl"),
            )
            .child(
                Text::new("note")
                    .variant(TextVariant::Caption)
                    .class("text-xs"),
            )
            .child(Text::new("body").class("text-base"));

        let mut layout = Vec::new();
        let mut buttons = Vec::new();
        walk_component(&tree, &mut layout, &mut buttons);

        let lines: Vec<&Line> = layout
            .iter()
            .filter(|l| !matches!(l, Line::Spacer))
            .collect();
        assert_eq!(lines.len(), 3);
        assert_eq!(*lines[0], Line::Heading("title".into()));
        assert_eq!(*lines[1], Line::Muted("note".into()));
        assert_eq!(*lines[2], Line::Body("body".into()));
    }

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
}
