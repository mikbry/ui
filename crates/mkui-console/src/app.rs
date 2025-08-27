use crate::renderer::ConsoleRenderer;
use crossterm::event::KeyCode;
use std::io;

pub struct ConsoleApp {
    renderer: ConsoleRenderer,
}

impl ConsoleApp {
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            renderer: ConsoleRenderer::new()?,
        })
    }
    
    pub fn renderer(&mut self) -> &mut ConsoleRenderer {
        &mut self.renderer
    }
    
    pub fn run<F>(mut self, mut render_fn: F) -> io::Result<()>
    where
        F: FnMut(&mut ConsoleRenderer, Option<KeyCode>) -> bool,
    {
        let mut last_key = None;
        
        loop {
            // Check for resize
            self.renderer.check_resize()?;
            
            // Clear screen 
            self.renderer.clear_screen()?;
            
            // Call render function
            if render_fn(&mut self.renderer, last_key) {
                break;
            }
            
            // Flush output
            self.renderer.flush()?;
            
            // Handle events
            if let Some(key) = self.renderer.handle_events()? {
                if matches!(key, KeyCode::Char('q') | KeyCode::Esc) {
                    break;
                }
                last_key = Some(key);
            } else {
                last_key = None;
            }
        }
        
        Ok(())
    }
}