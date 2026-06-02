//! Pointer routing for the wgpu backend.
//! Converts winit cursor and mouse events into press, arm, hit-test, and
//! activation behavior over the current frame's interactive regions.
//!
//! Translates raw `winit::WindowEvent`s into a press-to-arm activation
//! model:
//!
//! 1. **Press** at `(x, y)` — hit-test the per-frame `Vec<HitTestEntry>`
//!    in reverse paint order. If the cursor lands on an entry, **arm**
//!    that `NodeId`. No action fires yet.
//! 2. **Release** at `(x, y)` — hit-test again. The action fires only if
//!    the release lands on the *same* armed node. If the release lands
//!    elsewhere (or on no node), the armed state clears without firing.
//! 3. **`CursorLeft`** — cursor exited the window: clear armed state
//!    without firing (the user moved off and the press is cancelled).
//! 4. **`Escape` key press** — user-driven cancel: clear armed state
//!    without firing.
//!
//! This is the contract Codex round-10 Q4 ratified. A simpler
//! "fire-on-release-wherever-cursor-is" shape was explicitly rejected —
//! it disables drag-cancel, the standard desktop affordance.
//!
//! The cursor itself is latched from `WindowEvent::CursorMoved`
//! (`MouseInput` events do not carry a position, only the button +
//! state). Logical/physical DPI conversion uses `window.scale_factor()`.
//!
//! Action firing is **never** invoked while holding a borrow on the
//! `AppTree` or the registry (Sprint 4 anti-pattern carry-forward):
//! callers consume [`ClickHit`]'s cloned `ActionId` *after* dropping the
//! per-frame hit vector borrow.

#[cfg(not(target_arch = "wasm32"))]
use winit::dpi::PhysicalPosition;
#[cfg(not(target_arch = "wasm32"))]
use winit::event::{ElementState, MouseButton};

use mkui_runtime::{ActionId, NodeId};

use crate::types::{rect_contains, Point};
use crate::walker::HitTestEntry;

/// Pointer-state machine. One instance owned by `WgpuApp`; updated each
/// frame from the winit event stream.
///
/// The state machine is intentionally tiny: a latched cursor (for DPI
/// conversion + first-frame guard) and an `armed: Option<NodeId>` slot.
/// Everything else — fire/no-fire/cancel — is derived from those two
/// fields plus the hit-test outcome at each event.
#[derive(Debug, Default, Clone, Copy)]
pub struct PointerState {
    /// Most recent cursor position in *physical* pixels. `None` until the
    /// first `CursorMoved` event lands — wgpu apps that get a click on
    /// the very first frame before any cursor motion would otherwise
    /// dereference a default-initialised origin.
    physical_pos: Option<(f64, f64)>,
    /// Node armed by the most recent press, awaiting release. `None`
    /// between releases, after a cancellation, and at startup. Cleared
    /// on release (regardless of fire/no-fire), on `CursorLeft`, and on
    /// `Escape` press.
    armed: Option<NodeId>,
}

impl PointerState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the latched cursor position. Call from `WindowEvent::CursorMoved`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_cursor(&mut self, position: PhysicalPosition<f64>) {
        self.physical_pos = Some((position.x, position.y));
    }

    /// Convenience updater for tests that don't want to depend on winit
    /// types — both arms feed the same internal state.
    pub fn update_cursor_raw(&mut self, x: f64, y: f64) {
        self.physical_pos = Some((x, y));
    }

    pub fn cursor_logical(&self, scale_factor: f64) -> Option<Point> {
        let (x, y) = self.physical_pos?;
        let scale = scale_factor.max(f64::EPSILON);
        Some(Point::new((x / scale) as f32, (y / scale) as f32))
    }

    /// Currently-armed node, if any. Exposed for diagnostics + tests;
    /// production callers should route through [`PointerState::on_press`]
    /// / [`PointerState::on_release`] / [`PointerState::cancel`].
    pub fn armed(&self) -> Option<NodeId> {
        self.armed
    }

    /// Press handler. Hit-tests `hit_entries` at `cursor` in reverse
    /// paint order and arms the topmost matching node. Returns the
    /// armed `NodeId` if the press landed on an interactive region;
    /// `None` if it landed on empty space (in which case the previously
    /// armed node, if any, is also cleared — a press on empty space
    /// cancels a stale arm).
    ///
    /// Never fires an action: per Codex round-10 Q4, activation happens
    /// on release-inside-same-node, not on press.
    pub fn on_press(&mut self, hit_entries: &[HitTestEntry], cursor: Point) -> Option<NodeId> {
        let hit = hit_test(hit_entries, cursor);
        self.armed = hit.map(|h| h.node);
        self.armed
    }

    /// Release handler. Hit-tests `hit_entries` at `cursor` and returns
    /// `Some(ClickHit)` *only if* the release lands on the same node
    /// armed by the matching press. Any other shape — release on a
    /// different node, release on empty space, release with no prior
    /// press — returns `None`.
    ///
    /// The armed slot clears unconditionally on release, win or lose,
    /// so the next event starts fresh.
    pub fn on_release(&mut self, hit_entries: &[HitTestEntry], cursor: Point) -> Option<ClickHit> {
        let armed = self.armed.take();
        let hit = hit_test(hit_entries, cursor)?;
        if armed == Some(hit.node) {
            Some(hit)
        } else {
            None
        }
    }

    /// Clear the armed slot without firing. Call from `CursorLeft`
    /// (mouse left the window) and from `Escape` press (user cancel).
    pub fn cancel(&mut self) {
        self.armed = None;
    }
}

/// Convert a raw `winit::ElementState + MouseButton` pair to a typed
/// `Option<ElementState>`, filtering out non-left buttons. Lets the
/// event-loop handler stay terse:
///
/// ```ignore
/// if let Some(state) = left_button_state(button, state) { ... }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub fn left_button_state(button: MouseButton, state: ElementState) -> Option<ElementState> {
    (button == MouseButton::Left).then_some(state)
}

/// Result of a hit-test that found a target. The caller fires `action`
/// through the tree's `ActionRegistry`. Held separately from
/// [`HitTestEntry`] so the caller doesn't accidentally re-borrow the
/// per-frame vec while running the action closure (anti-pattern
/// carry-forward from Sprint 4).
///
/// Field names mirror the round-10 §"Concrete Shape" sketch's
/// [`HitTestEntry`] (`node`, `action`).
#[derive(Debug, Clone, Copy)]
pub struct ClickHit {
    pub node: NodeId,
    pub action: Option<ActionId>,
}

/// Reverse paint-order hit-test. Iterates `hit_entries` from the end
/// (latest-painted = topmost) and returns the first containing entry, if
/// any.
///
/// Cloning the `ActionId` out of the entry lets the caller drop the
/// `&[HitTestEntry]` borrow before invoking the action, avoiding any
/// chance of borrowing the per-frame vec across an action that itself
/// rebuilds the tree.
pub fn hit_test(hit_entries: &[HitTestEntry], cursor: Point) -> Option<ClickHit> {
    hit_entries
        .iter()
        .rev()
        .find(|entry| rect_contains(entry.rect, cursor))
        .map(|entry| ClickHit {
            node: entry.node,
            action: entry.action,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Rect, Size};
    use mkui_runtime::ActionId as RtActionId;
    use mkui_runtime::NodeId;

    fn entry_with_action(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        idx: u32,
        action: Option<RtActionId>,
    ) -> HitTestEntry {
        HitTestEntry {
            rect: Rect::new(Point::new(x, y), Size::new(w, h)),
            node: NodeId::from_raw(idx, 0),
            action,
        }
    }

    fn entry(x: f32, y: f32, w: f32, h: f32, idx: u32) -> HitTestEntry {
        entry_with_action(x, y, w, h, idx, None)
    }

    #[test]
    fn cursor_logical_scales_by_dpi() {
        let mut p = PointerState::new();
        p.update_cursor_raw(200.0, 100.0);
        let logical = p.cursor_logical(2.0).expect("cursor set");
        assert_eq!(logical, Point::new(100.0, 50.0));
    }

    #[test]
    fn cursor_logical_is_none_before_first_move() {
        let p = PointerState::new();
        assert!(p.cursor_logical(1.0).is_none());
    }

    #[test]
    fn hit_test_returns_none_when_cursor_misses_every_rect() {
        let entries = vec![entry(0.0, 0.0, 10.0, 10.0, 1)];
        assert!(hit_test(&entries, Point::new(50.0, 50.0)).is_none());
    }

    #[test]
    fn hit_test_picks_topmost_on_overlap() {
        // Two overlapping buttons; the second one was painted later
        // (further down the vec) so the reverse iterator must surface it.
        let entries = vec![
            entry(0.0, 0.0, 100.0, 100.0, 1),
            entry(20.0, 20.0, 40.0, 40.0, 2),
        ];
        let hit = hit_test(&entries, Point::new(30.0, 30.0)).expect("inside both");
        assert_eq!(hit.node, NodeId::from_raw(2, 0));
    }

    #[test]
    fn hit_test_falls_through_to_underlying_when_overlap_misses() {
        let entries = vec![
            entry(0.0, 0.0, 100.0, 100.0, 1),
            entry(80.0, 80.0, 10.0, 10.0, 2),
        ];
        let hit = hit_test(&entries, Point::new(30.0, 30.0)).expect("inside outer only");
        assert_eq!(hit.node, NodeId::from_raw(1, 0));
    }

    // ---- press-to-arm state machine (Codex round-10 Q4 contract) ----

    #[test]
    fn arm_release_same_node_fires() {
        let entries = vec![entry(0.0, 0.0, 100.0, 100.0, 1)];
        let mut p = PointerState::new();
        let armed = p.on_press(&entries, Point::new(50.0, 50.0));
        assert_eq!(armed, Some(NodeId::from_raw(1, 0)));
        let click = p
            .on_release(&entries, Point::new(60.0, 60.0))
            .expect("release on armed node fires");
        assert_eq!(click.node, NodeId::from_raw(1, 0));
        assert!(p.armed().is_none(), "release clears the armed slot");
    }

    #[test]
    fn arm_release_different_node_no_fire() {
        let entries = vec![
            entry(0.0, 0.0, 50.0, 50.0, 1),
            entry(100.0, 100.0, 50.0, 50.0, 2),
        ];
        let mut p = PointerState::new();
        let armed = p.on_press(&entries, Point::new(20.0, 20.0));
        assert_eq!(armed, Some(NodeId::from_raw(1, 0)));
        let click = p.on_release(&entries, Point::new(120.0, 120.0));
        assert!(click.is_none(), "release on different node must not fire");
        assert!(p.armed().is_none(), "release clears the armed slot");
    }

    #[test]
    fn arm_release_on_empty_space_no_fire() {
        let entries = vec![entry(0.0, 0.0, 50.0, 50.0, 1)];
        let mut p = PointerState::new();
        p.on_press(&entries, Point::new(20.0, 20.0));
        let click = p.on_release(&entries, Point::new(200.0, 200.0));
        assert!(click.is_none(), "release on empty space must not fire");
        assert!(p.armed().is_none());
    }

    #[test]
    fn arm_cursor_leave_no_fire() {
        let entries = vec![entry(0.0, 0.0, 100.0, 100.0, 1)];
        let mut p = PointerState::new();
        p.on_press(&entries, Point::new(50.0, 50.0));
        // Mouse leaves the window before release.
        p.cancel();
        assert!(p.armed().is_none(), "CursorLeft cancels the armed slot");
        // A subsequent release (when the user re-enters and releases the
        // button somewhere) must NOT fire.
        let click = p.on_release(&entries, Point::new(50.0, 50.0));
        assert!(
            click.is_none(),
            "release after cancel must not fire even if cursor lands on the previously armed node"
        );
    }

    #[test]
    fn arm_escape_no_fire() {
        let entries = vec![entry(0.0, 0.0, 100.0, 100.0, 1)];
        let mut p = PointerState::new();
        p.on_press(&entries, Point::new(50.0, 50.0));
        // User presses Escape — host calls cancel() from the keyboard handler.
        p.cancel();
        assert!(p.armed().is_none());
        let click = p.on_release(&entries, Point::new(50.0, 50.0));
        assert!(
            click.is_none(),
            "Escape-cancelled press must not fire on release"
        );
    }

    #[test]
    fn release_with_no_arm_no_fire() {
        let entries = vec![entry(0.0, 0.0, 100.0, 100.0, 1)];
        let mut p = PointerState::new();
        // No prior press; e.g. mouse re-enters the window with the button
        // held and the OS surfaces a release.
        let click = p.on_release(&entries, Point::new(50.0, 50.0));
        assert!(click.is_none(), "release with no armed node must not fire");
    }

    #[test]
    fn press_on_empty_space_clears_stale_arm() {
        // Defensive: if a previous press armed a node and (somehow) no
        // release was seen, a new press on empty space should clear the
        // stale arm so a later release cannot fire it.
        let entries = vec![entry(0.0, 0.0, 50.0, 50.0, 1)];
        let mut p = PointerState::new();
        p.on_press(&entries, Point::new(20.0, 20.0));
        let armed = p.on_press(&entries, Point::new(200.0, 200.0));
        assert!(armed.is_none());
        assert!(p.armed().is_none());
    }
}
