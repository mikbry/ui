//! Terminal renderer used by the console backend.
//!
//! [`ConsoleRenderer`] is the console counterpart of [`mkui_web::renderer::WebRenderer`]
//! and [`mkui_wgpu::renderer::WgpuRenderer`]: it owns the output surface
//! (`stdout` here) and exposes a small set of operations the higher-level
//! [`crate::high_level::Mkui`] composes against.
//!
//! Keeping the terminal I/O behind this type lets the rest of the crate
//! describe *what* should appear without caring about ANSI escapes,
//! cursor positioning, or raw-mode toggles.

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, size, Clear, ClearType},
};
use std::io::{self, stdout, Write};

/// Console output surface.
///
/// Wraps the small slice of `crossterm` the backend needs (raw-mode
/// toggling, cursor visibility, screen clears, byte writes) so the rest of
/// the crate can move data through a single seam.
#[derive(Debug, Default)]
pub struct ConsoleRenderer;

impl ConsoleRenderer {
    pub fn new() -> Self {
        Self
    }

    /// Current terminal size in `(columns, rows)`. Returns `(80, 24)` if
    /// the platform refuses to answer.
    pub fn size(&self) -> (u16, u16) {
        size().unwrap_or((80, 24))
    }

    pub fn enter_alt_screen(&self) -> io::Result<()> {
        enable_raw_mode()?;
        execute!(stdout(), Hide, Clear(ClearType::All))?;
        Ok(())
    }

    pub fn leave_alt_screen(&self) -> io::Result<()> {
        execute!(stdout(), Show, Clear(ClearType::All), ResetColor)?;
        disable_raw_mode()?;
        Ok(())
    }

    pub fn clear_all(&self) -> io::Result<()> {
        execute!(stdout(), Clear(ClearType::All), Clear(ClearType::Purge))?;
        Ok(())
    }

    pub fn move_to(&self, col: u16, row: u16) -> io::Result<()> {
        execute!(stdout(), MoveTo(col, row))?;
        Ok(())
    }

    /// Write a stylised string at `(col, row)` using crossterm's
    /// `StyledContent` (any `Display` works, e.g. `text.white().bold()`).
    pub fn print_styled<D: std::fmt::Display>(
        &self,
        col: u16,
        row: u16,
        content: D,
    ) -> io::Result<()> {
        execute!(stdout(), MoveTo(col, row))?;
        print!("{}", content);
        Ok(())
    }

    pub fn print_footer(&self, row: u16, text: &str) -> io::Result<()> {
        execute!(
            stdout(),
            MoveTo(0, row),
            SetForegroundColor(Color::DarkGrey),
            Print(text),
            ResetColor
        )?;
        Ok(())
    }

    /// Paint a `clear_width × clear_height` rectangle of spaces starting at
    /// the origin. Used between frames to overwrite stale glyphs when the
    /// terminal size changes between renders.
    pub fn paint_blank(&self, clear_width: usize, clear_height: usize) -> io::Result<()> {
        for row in 0..clear_height {
            execute!(
                stdout(),
                MoveTo(0, row as u16),
                Print(" ".repeat(clear_width))
            )?;
        }
        execute!(stdout(), MoveTo(0, 0))?;
        Ok(())
    }

    pub fn flush(&self) -> io::Result<()> {
        stdout().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_constructs_without_io() {
        let _ = ConsoleRenderer::new();
    }

    #[test]
    fn renderer_size_returns_a_sensible_fallback() {
        let r = ConsoleRenderer::new();
        let (cols, rows) = r.size();
        assert!(cols >= 1 && rows >= 1, "size fallback must be non-zero");
    }
}
