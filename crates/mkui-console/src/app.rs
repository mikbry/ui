//! Backend app object for the console renderer.
//!
//! [`ConsoleApp`] owns the long-lived terminal state — last observed
//! terminal size, selected control index — that survives between frames.
//! It is the console counterpart of `mkui_web::app::WebApp` and
//! `mkui_wgpu::app::WgpuApp`.
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
///
/// The cursor is an index into whatever flat `Vec` of interactive controls
/// the caller is navigating this frame — originally just buttons, now any
/// `crate::components::Selectable` (`Button` or `MenuList` item, #189 round
/// 3). `ConsoleApp` itself doesn't know or care which: it only knows "a
/// position in a list of length `len`."
pub struct ConsoleApp {
    renderer: ConsoleRenderer,
    last_terminal_size: (u16, u16),
    selected_index: usize,
}

impl ConsoleApp {
    pub fn new() -> std::io::Result<Self> {
        let renderer = ConsoleRenderer::new();
        let last_terminal_size = renderer.size();
        Ok(Self {
            renderer,
            last_terminal_size,
            selected_index: 0,
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

    pub fn selected_index(&self) -> usize {
        self.selected_index
    }

    /// Set the cursor directly — used once, before the first render, to
    /// seed it from a `MenuList`'s construction-time `selected_index`
    /// rather than always starting at 0. Not called again afterwards: a
    /// later `build_layout()` refresh (after confirming an item) must not
    /// stomp on where the user has since navigated to.
    pub fn set_selected_index(&mut self, index: usize) {
        self.selected_index = index;
    }

    /// Move the selection cursor forward, wrapping back to 0 once it passes
    /// `len - 1`. Calling this with `len == 0` is a no-op.
    pub fn select_next(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if self.selected_index + 1 < len {
            self.selected_index += 1;
        } else {
            self.selected_index = 0;
        }
    }

    /// Move the selection cursor backward, wrapping to `len - 1` at the
    /// start of the list. Calling this with `len == 0` is a no-op.
    pub fn select_prev(&mut self, len: usize) {
        if len == 0 {
            return;
        }
        if self.selected_index > 0 {
            self.selected_index -= 1;
        } else {
            self.selected_index = len - 1;
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
        app.selected_index = 2;
        app.select_next(3);
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn select_prev_wraps_to_last() {
        let mut app = ConsoleApp::new().unwrap();
        app.selected_index = 0;
        app.select_prev(4);
        assert_eq!(app.selected_index(), 3);
    }

    #[test]
    fn selection_helpers_handle_empty_lists() {
        let mut app = ConsoleApp::new().unwrap();
        app.select_next(0);
        app.select_prev(0);
        assert_eq!(app.selected_index(), 0);
    }

    #[test]
    fn set_selected_index_seeds_the_cursor_directly() {
        let mut app = ConsoleApp::new().unwrap();
        app.set_selected_index(2);
        assert_eq!(app.selected_index(), 2);
    }
}
