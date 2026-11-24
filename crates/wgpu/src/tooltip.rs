//! `TooltipState` — hover/focus visibility state for a tooltip (#192).
//!
//! Pure logic, no winit/host dependency: like [`crate::pointer::PointerState`]
//! and [`crate::text_edit::TextEditState`], one instance is owned by the
//! caller and kept across frames (the renderer is immediate-mode and
//! retains no component tree between frames).
//!
//! The acceptance contract (#192 criterion #1) is that a tooltip appears on
//! hover **or** keyboard/gamepad focus — never hover-only. [`TooltipState`]
//! tracks the two inputs independently (`hovered`, `focused`) and ORs them
//! in [`TooltipState::is_visible`]; nothing here decides *how* a frame
//! discovers hover or focus:
//!
//! - **Hover** — feed [`crate::pointer::PointerState::hovered`] /
//!   [`crate::input_router::InputRouter::hovered_node`]. Compare the
//!   returned `NodeId` against the tooltip's anchor and call
//!   [`TooltipState::pointer_enter`] / [`TooltipState::pointer_leave`].
//! - **Focus** — a plain bool the host feeds in via
//!   [`TooltipState::set_focused`]. No gamepad input exists anywhere in
//!   this workspace yet (#176 tracks the broader input-routing contract);
//!   a future gamepad focus-traversal system is expected to land as the
//!   same host-fed `true`/`false` a keyboard Tab would produce, so
//!   `set_focused` already covers it without a change here (tracked as
//!   follow-up #240, along with a show/hide delay timer — neither is
//!   needed to satisfy #192's acceptance criteria).
//!
//! [`crate::components::tooltip`] paints the bubble once the caller has
//! decided, via [`TooltipState::is_visible`], that it should be shown.

/// Hover/focus visibility state for one tooltip anchor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TooltipState {
    hovered: bool,
    focused: bool,
    disabled: bool,
}

impl TooltipState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the tooltip should currently be drawn: hovered OR focused,
    /// unless disabled. Never hover-only (#192).
    pub fn is_visible(&self) -> bool {
        !self.disabled && (self.hovered || self.focused)
    }

    pub fn is_hovered(&self) -> bool {
        self.hovered
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Suppresses the tooltip regardless of hover/focus — e.g. while its
    /// anchor control is itself disabled.
    pub fn set_disabled(&mut self, disabled: bool) {
        self.disabled = disabled;
    }

    /// Call when the pointer enters the anchor's hit region.
    pub fn pointer_enter(&mut self) {
        self.hovered = true;
    }

    /// Call when the pointer leaves the anchor's hit region.
    pub fn pointer_leave(&mut self) {
        self.hovered = false;
    }

    /// Call with the anchor's current keyboard/gamepad focus state.
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hidden_by_default() {
        assert!(!TooltipState::new().is_visible());
    }

    #[test]
    fn hover_alone_shows_it() {
        let mut t = TooltipState::new();
        t.pointer_enter();
        assert!(t.is_visible());
    }

    #[test]
    fn focus_alone_shows_it_not_hover_only() {
        let mut t = TooltipState::new();
        t.set_focused(true);
        assert!(t.is_visible(), "focus alone must show the tooltip");
    }

    #[test]
    fn hover_leaving_while_focused_keeps_it_shown() {
        let mut t = TooltipState::new();
        t.pointer_enter();
        t.set_focused(true);
        t.pointer_leave();
        assert!(t.is_visible(), "focus still holds it open after hover ends");
    }

    #[test]
    fn blur_while_hovered_keeps_it_shown() {
        let mut t = TooltipState::new();
        t.set_focused(true);
        t.pointer_enter();
        t.set_focused(false);
        assert!(t.is_visible(), "hover still holds it open after blur");
    }

    #[test]
    fn both_off_hides_it() {
        let mut t = TooltipState::new();
        t.pointer_enter();
        t.set_focused(true);
        t.pointer_leave();
        t.set_focused(false);
        assert!(!t.is_visible());
    }

    #[test]
    fn disabled_suppresses_hover_and_focus() {
        let mut t = TooltipState::new();
        t.set_disabled(true);
        t.pointer_enter();
        t.set_focused(true);
        assert!(!t.is_visible());
    }

    #[test]
    fn pointer_leave_without_enter_is_a_no_op() {
        let mut t = TooltipState::new();
        t.pointer_leave();
        assert!(!t.is_hovered());
        assert!(!t.is_visible());
    }
}
