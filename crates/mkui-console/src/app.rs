//! Backend app object for the console renderer.
//!
//! [`ConsoleApp`] owns the long-lived terminal state — last observed
//! terminal size, selected button index — that survives between frames.
//! It is the console counterpart of [`mkui_web::app::WebApp`] and
//! [`mkui_wgpu::app::WgpuApp`].
//!
//! The high-level [`crate::high_level::Mkui`] composes [`ConsoleApp`] with a
//! [`crate::renderer::ConsoleRenderer`] and the [`crate::components`]
//! module's flatteners to turn a [`mkui_core::components`] tree into terminal
//! output without duplicating bookkeeping fields.

use crate::renderer::ConsoleRenderer;

/// Persistent state for an interactive console session.
///
/// Holds the selection cursor and the last terminal size seen by the render
/// loop so resize handling can compare against it. Anything that needs to
/// survive across redraws lives here rather than in [`crate::Mkui`], which
/// is the per-build surface users hand a component tree to.
pub struct ConsoleApp {
    renderer: ConsoleRenderer,
    last_terminal_size: (u16, u16),
    selected_button: usize,
}

impl ConsoleApp {
    pub fn new() -> std::io::Result<Self> {
        let renderer = ConsoleRenderer::new();
        let last_terminal_size = renderer.size();
        Ok(Self {
            renderer,
            last_terminal_size,
            selected_button: 0,
        })
    }

    pub fn renderer(&self) -> &ConsoleRenderer {
        &self.renderer
    }

    pub fn last_terminal_size(&self) -> (u16, u16) {
        self.last_terminal_size
    }

    pub fn set_last_terminal_size(&mut self, size: (u16, u16)) {
        self.last_terminal_size = size;
    }

    pub fn selected_button(&self) -> usize {
        self.selected_button
    }

    /// Move the selection cursor forward, wrapping back to 0 once it passes
    /// `len - 1`. Calling this with `len == 0` is a no-op.
    pub fn select_next(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if self.selected_button + 1 < len {
            self.selected_button += 1;
        } else {
            self.selected_button = 0;
        }
    }

    /// Move the selection cursor backward, wrapping to `len - 1` at the
    /// start of the list. Calling this with `len == 0` is a no-op.
    pub fn select_prev(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if self.selected_button > 0 {
            self.selected_button -= 1;
        } else {
            self.selected_button = len - 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_next_wraps_to_zero() {
        let mut app = ConsoleApp::new().unwrap();
        // Manually drive past the end.
        app.selected_button = 2;
        app.select_next(3);
        assert_eq!(app.selected_button(), 0);
    }

    #[test]
    fn select_prev_wraps_to_last() {
        let mut app = ConsoleApp::new().unwrap();
        app.selected_button = 0;
        app.select_prev(4);
        assert_eq!(app.selected_button(), 3);
    }

    #[test]
    fn selection_helpers_handle_empty_lists() {
        let mut app = ConsoleApp::new().unwrap();
        app.select_next(0);
        app.select_prev(0);
        assert_eq!(app.selected_button(), 0);
    }
}
