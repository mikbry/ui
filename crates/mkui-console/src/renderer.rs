use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor, Stylize},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType, size},
    cursor::{Hide, MoveTo, Show},
};
use std::io::{self, stdout, Write};

pub struct ConsoleRenderer {
    last_terminal_size: (u16, u16),
}

impl ConsoleRenderer {
    pub fn new() -> io::Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;
        let initial_size = size().unwrap_or((80, 24));
        Ok(Self { 
            last_terminal_size: initial_size,
        })
    }
    
    pub fn clear_screen(&mut self) -> io::Result<()> {
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
        
        // Move cursor to top
        execute!(stdout(), MoveTo(0, 0))?;
        Ok(())
    }
    
    pub fn render_text(&self, text: &str, x: u16, y: u16, color: Option<Color>) -> io::Result<()> {
        execute!(stdout(), MoveTo(x, y))?;
        if let Some(c) = color {
            execute!(stdout(), SetForegroundColor(c))?;
        }
        print!("{}", text);
        if color.is_some() {
            execute!(stdout(), ResetColor)?;
        }
        Ok(())
    }
    
    pub fn render_button(&self, text: &str, x: u16, y: u16, selected: bool, variant: &str) -> io::Result<()> {
        execute!(stdout(), MoveTo(x, y))?;
        let button_text = format!("[ {} ]", text);
        
        if selected {
            // Selected state
            match variant {
                "Primary" => print!("{}", button_text.white().on_blue().bold()),
                "Secondary" => print!("{}", button_text.black().on_grey().bold()),
                "Destructive" => print!("{}", button_text.white().on_red().bold()),
                "Outline" => print!("{}", button_text.blue().bold()),
                "Ghost" => print!("{}", button_text.blue().bold()),
                _ => print!("{}", button_text.white().bold()),
            }
        } else {
            // Normal state
            match variant {
                "Primary" => print!("{}", button_text.white().on_blue()),
                "Secondary" => print!("{}", button_text.dark_grey()),
                "Destructive" => print!("{}", button_text.white().on_dark_red()),
                "Outline" => print!("{}", button_text.white()),
                "Ghost" => print!("{}", button_text.dark_grey()),
                _ => print!("{}", button_text),
            }
        }
        Ok(())
    }
    
    pub fn flush(&self) -> io::Result<()> {
        stdout().flush()
    }
    
    pub fn handle_events(&self) -> io::Result<Option<KeyCode>> {
        if let Event::Key(KeyEvent { code, .. }) = event::read()? {
            return Ok(Some(code));
        }
        Ok(None)
    }
    
    pub fn check_resize(&mut self) -> io::Result<bool> {
        let current_size = size()?;
        let size_changed = current_size != self.last_terminal_size;
        if size_changed {
            self.last_terminal_size = current_size;
            // Force a complete clear on resize
            execute!(stdout(), Clear(ClearType::All), Clear(ClearType::Purge))?;
        }
        Ok(size_changed)
    }
}

impl Drop for ConsoleRenderer {
    fn drop(&mut self) {
        let _ = execute!(stdout(), Show, Clear(ClearType::All), ResetColor);
        let _ = disable_raw_mode();
    }
}