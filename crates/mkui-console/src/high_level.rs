use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, size},
    cursor::{Hide, MoveTo, Show},
};
use std::io::{self, stdout, Write};
use mkui_core::components::*;

/// Console app with high-level mkui interface
pub struct Mkui {
    // For now, we'll store the showcase layout directly
    // TODO: Implement proper component tree rendering
    selected_button: usize,
    last_terminal_size: (u16, u16),
}

impl Mkui {
    pub fn new() -> std::io::Result<Self> {
        let initial_size = size().unwrap_or((80, 24));
        Ok(Self {
            selected_button: 0,
            last_terminal_size: initial_size,
        })
    }
    
    // Matches web Mkui API
    pub fn child(self, _child: impl Component + 'static) -> Self {
        // TODO: Store components for rendering
        // For now, ignore the component tree and render the showcase directly
        self
    }
    
    pub fn run(mut self) -> std::io::Result<()> {
        println!("mkui: Creating console app...");
        
        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;

        loop {
            // Check if terminal was resized
            let current_size = size()?;
            let size_changed = current_size != self.last_terminal_size;
            if size_changed {
                self.last_terminal_size = current_size;
                execute!(stdout(), Clear(ClearType::All), Clear(ClearType::Purge))?;
            }
            
            self.render()?;
            
            let event = event::read()?;
            match event {
                Event::Key(KeyEvent { code, .. }) => {
                    match code {
                        KeyCode::Left | KeyCode::Up => {
                            if self.selected_button > 0 {
                                self.selected_button -= 1;
                            } else {
                                self.selected_button = 4;
                            }
                        }
                        KeyCode::Right | KeyCode::Down => {
                            if self.selected_button < 4 {
                                self.selected_button += 1;
                            } else {
                                self.selected_button = 0;
                            }
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            let button_names = ["Primary", "Secondary", "Destructive", "Outline", "Ghost"];
                            println!("\r\n{} button clicked!", button_names[self.selected_button]);
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                        KeyCode::Char('q') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
                Event::Resize(_, _) => continue,
                _ => {}
            }
        }

        execute!(stdout(), Show, Clear(ClearType::All), ResetColor)?;
        disable_raw_mode()?;
        Ok(())
    }

    fn render(&self) -> io::Result<()> {
        // Get terminal size first
        let (width, height) = size()?;
        let width = width as usize;
        let height = height as usize;
        
        // Clear the entire terminal buffer
        let clear_width = width.max(self.last_terminal_size.0 as usize);
        let clear_height = height.max(self.last_terminal_size.1 as usize);
        
        for row in 0..clear_height {
            execute!(stdout(), MoveTo(0, row as u16), Print(" ".repeat(clear_width)))?;
        }
        
        execute!(stdout(), MoveTo(0, 0))?;
        
        let mut current_row = 2;
        
        // Header
        let header_text = "miklabs/ui";
        let tagline = "Modern UI toolkit for Rust & C++";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", header_text.white());
        print!("  {}", tagline.dark_grey());
        current_row += 2;
        
        // Hero section
        let title = "miklabs/ui Component Library";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", title.white().bold());
        current_row += 1;
        
        let subtitle = "Beautiful, accessible components built with Rust and WebAssembly";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", subtitle.dark_grey());
        current_row += 3;
        
        // Button showcase section
        let showcase_title = "Button Components";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", showcase_title.white().bold());
        current_row += 1;
        
        let showcase_subtitle = "Various button styles and variants";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", showcase_subtitle.dark_grey());
        current_row += 2;
        
        // Buttons
        let buttons = ["Primary", "Secondary", "Destructive", "Outline", "Ghost"];
        for (i, name) in buttons.iter().enumerate() {
            let is_selected = self.selected_button == i;
            let button_text = format!("[ {} ]", name);
            
            execute!(stdout(), MoveTo(4, current_row as u16))?;
            
            if is_selected {
                match name {
                    &"Primary" => print!("{}", button_text.white().on_blue().bold()),
                    &"Secondary" => print!("{}", button_text.black().on_grey().bold()),
                    &"Destructive" => print!("{}", button_text.white().on_red().bold()),
                    &"Outline" => print!("{}", button_text.blue().bold()),
                    &"Ghost" => print!("{}", button_text.blue().bold()),
                    _ => {}
                }
            } else {
                match name {
                    &"Primary" => print!("{}", button_text.white().on_blue()),
                    &"Secondary" => print!("{}", button_text.dark_grey()),
                    &"Destructive" => print!("{}", button_text.white().on_dark_red()),
                    &"Outline" => print!("{}", button_text.white()),
                    &"Ghost" => print!("{}", button_text.dark_grey()),
                    _ => {}
                }
            }
            current_row += 1;
        }
        
        current_row += 2;
        
        // Footer
        let footer = "Built with 🦀 Rust • MIT/Apache Licensed";
        execute!(stdout(), MoveTo(2, current_row as u16))?;
        print!("{}", footer.dark_grey());
        
        // Instructions
        let instructions = "↑↓/←→: Select button | Space/Enter: Click | q: Quit";
        let instr_y = (height.saturating_sub(1)).min(current_row + 2);
        execute!(stdout(), 
            MoveTo(0, instr_y as u16),
            SetForegroundColor(Color::DarkGrey),
            Print(instructions),
            ResetColor
        )?;

        stdout().flush()?;
        Ok(())
    }
}