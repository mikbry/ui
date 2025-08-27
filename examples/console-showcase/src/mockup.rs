use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, size},
    cursor::{Hide, MoveTo, Show},
};
use std::io::{self, stdout, Write};

struct ConsoleShowcaseApp {
    selected_button: usize,
    last_terminal_size: (u16, u16),
}

impl ConsoleShowcaseApp {
    fn new() -> Self {
        let initial_size = size().unwrap_or((80, 24));
        Self {
            selected_button: 0,  // Start with first button selected
            last_terminal_size: initial_size,
        }
    }

    fn run(&mut self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;

        loop {
            // Check if terminal was resized
            let current_size = size()?;
            let size_changed = current_size != self.last_terminal_size;
            if size_changed {
                self.last_terminal_size = current_size;
                // Force a complete clear on resize
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
                                self.selected_button = 4;  // Wrap to last button
                            }
                        }
                        KeyCode::Right | KeyCode::Down => {
                            if self.selected_button < 4 {
                                self.selected_button += 1;
                            } else {
                                self.selected_button = 0;  // Wrap to first button
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
                Event::Resize(_, _) => {
                    // Terminal was resized, the size check above will handle it
                    continue;
                }
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
        
        // Clear the entire terminal buffer by writing spaces to every position
        // Use the larger of current or previous size to ensure complete clearing
        let clear_width = width.max(self.last_terminal_size.0 as usize);
        let clear_height = height.max(self.last_terminal_size.1 as usize);
        
        for row in 0..clear_height {
            execute!(stdout(), MoveTo(0, row as u16), Print(" ".repeat(clear_width)))?;
        }
        
        // Move cursor to top and start rendering
        execute!(stdout(), MoveTo(0, 0))?;
        
        let mut current_row = 0;
        
        // Header
        current_row = self.render_header(width, current_row)?;
        current_row += 1; // Empty line
        
        // Main content
        current_row = self.render_hero(width, current_row)?;
        current_row += 1; // Empty line
        
        current_row = self.render_button_showcase(width, current_row)?;
        current_row += 1; // Empty line
        
        // Footer
        current_row = self.render_footer(width, current_row)?;
        
        // Instructions at the bottom
        let instructions = "↑↓/←→: Select button | Space/Enter: Click | q: Quit";
        let instr_y = (height.saturating_sub(1)).min(current_row + 2);
        execute!(stdout(), 
            MoveTo(0, instr_y as u16),
            SetForegroundColor(Color::DarkGrey),
            Print(instructions),
            ResetColor
        )?;

        // Final flush to ensure everything is rendered
        stdout().flush()?;
        Ok(())
    }

    fn render_header(&self, width: usize, start_row: usize) -> io::Result<usize> {
        let mut row = start_row;
        
        // Top border
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", "─".repeat(width).dark_grey());
        row += 1;
        
        // Header content
        let brand = "miklabs/ui";
        let tagline = "Modern UI toolkit for Rust & C++";
        let github_text = "[ GitHub ]";
        
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("  ");
        print!("{}", brand.white());
        
        print!("  ");
        print!("{}", tagline.dark_grey());
        
        // Calculate remaining space for GitHub button
        let used_space = 2 + brand.len() + 2 + tagline.len();
        let remaining = width.saturating_sub(used_space + github_text.len());
        print!("{}", " ".repeat(remaining));
        
        print!("{}", github_text.dark_grey());
        row += 1;
        
        // Bottom border
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", "─".repeat(width).dark_grey());
        row += 1;
        
        Ok(row)
    }

    fn render_hero(&self, width: usize, start_row: usize) -> io::Result<usize> {
        let mut row = start_row;
        
        // Center the hero text
        let title = "miklabs/ui Component Library";
        let subtitle = "Beautiful, accessible components built with Rust and WebAssembly";
        
        let title_padding = (width.saturating_sub(title.len())) / 2;
        let subtitle_padding = (width.saturating_sub(subtitle.len())) / 2;
        
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(title_padding));
        print!("{}", title.white());
        row += 1;
        
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(subtitle_padding));
        print!("{}", subtitle.dark_grey());
        row += 1;
        
        Ok(row)
    }

    fn render_button_showcase(&self, width: usize, start_row: usize) -> io::Result<usize> {
        let mut row = start_row;
        
        // Calculate minimum width needed for all buttons
        let buttons = ["Primary", "Secondary", "Destructive", "Outline", "Ghost"];
        let min_buttons_width: usize = buttons.iter().map(|b| b.len() + 4).sum::<usize>() + (buttons.len() - 1) * 2; // 4 for "[ ]", 2 for spacing
        let min_card_width = min_buttons_width + 4; // +4 for left space + right space + borders
        
        // Calculate card width (leave margins on both sides)
        let margin = (width / 6).max(4); // Dynamic margin based on terminal width
        let available_width = width.saturating_sub(margin * 2);
        let card_width = min_card_width.max(60).min(available_width); // At least minimum needed, prefer 60, but fit in terminal
        let inner_width = card_width.saturating_sub(2); // Account for borders
        
        // Card border top
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("┌{}┐", "─".repeat(card_width.saturating_sub(2)));
        row += 1;
        
        // Title
        let title = "Button Components";
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("│ ");
        print!("{}", title.white());
        let title_padding = inner_width.saturating_sub(title.len() + 1);
        print!("{} │", " ".repeat(title_padding));
        row += 1;
        
        // Subtitle
        let subtitle = "Various button styles and variants";
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("│ {}", subtitle.dark_grey());
        let subtitle_padding = inner_width.saturating_sub(subtitle.len() + 1);
        print!("{} │", " ".repeat(subtitle_padding));
        row += 1;
        
        // Empty line
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("│{} │", " ".repeat(inner_width));
        row += 1;
        
        // Buttons row
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("│ ");
        
        for (i, name) in buttons.iter().enumerate() {
            let is_selected = self.selected_button == i;
            
            if i > 0 {
                print!("  ");
            }
            
            let button_text = format!("[ {} ]", name);
            
            if is_selected {
                // Selected state
                match name {
                    &"Primary" => print!("{}", button_text.white().on_blue().bold()),
                    &"Secondary" => print!("{}", button_text.black().on_grey().bold()),
                    &"Destructive" => print!("{}", button_text.white().on_red().bold()),
                    &"Outline" => print!("{}", button_text.blue().bold()),
                    &"Ghost" => print!("{}", button_text.blue().bold()),
                    _ => {}
                }
            } else {
                // Normal state
                match name {
                    &"Primary" => print!("{}", button_text.white().on_blue()),
                    &"Secondary" => print!("{}", button_text.dark_grey()),
                    &"Destructive" => print!("{}", button_text.white().on_dark_red()),
                    &"Outline" => print!("{}", button_text.white()),
                    &"Ghost" => print!("{}", button_text.dark_grey()),
                    _ => {}
                }
            }
        }
        
        // Calculate remaining space after buttons
        let buttons_text_len: usize = buttons.iter().map(|b| b.len() + 4).sum::<usize>() + (buttons.len() - 1) * 2; // 4 for "[ ]", 2 for spacing
        let button_padding = inner_width.saturating_sub(buttons_text_len + 1);
        
        // Ensure we have enough space for the right border
        if button_padding > 0 {
            print!("{} │", " ".repeat(button_padding));
        } else {
            print!(" │"); // At minimum, one space before border
        }
        row += 1;
        
        // Empty line
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("│{} │", " ".repeat(inner_width));
        row += 1;
        
        // Card border bottom
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", " ".repeat(margin));
        print!("└{}┘", "─".repeat(card_width.saturating_sub(2)));
        row += 1;
        
        Ok(row)
    }

    fn render_footer(&self, width: usize, start_row: usize) -> io::Result<usize> {
        let mut row = start_row;
        
        // Top border
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("{}", "─".repeat(width).dark_grey());
        row += 1;
        
        // Footer content
        let left_content = "Built with 🦀 Rust";
        let source_code = "[ Source Code ]";
        let license_text = " • MIT/Apache Licensed";
        
        execute!(stdout(), MoveTo(0, row as u16))?;
        print!("  ");
        print!("{}", left_content.dark_grey());
        
        // Calculate remaining space
        let used_space = 2 + left_content.len();
        let right_content_len = source_code.len() + license_text.len();
        let remaining = width.saturating_sub(used_space + right_content_len);
        print!("{}", " ".repeat(remaining));
        
        // Right side content
        print!("{}", source_code.dark_grey());
        print!("{}", license_text.dark_grey());
        row += 1;
        
        Ok(row)
    }
}

fn main() -> io::Result<()> {
    let mut app = ConsoleShowcaseApp::new();
    app.run()
}