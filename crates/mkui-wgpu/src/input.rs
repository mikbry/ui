//! Pointer input plumbing for the wgpu backend.
//!
//! Translates raw `winit::WindowEvent`s into:
//!
//! - cursor-position tracking (latched from `CursorMoved` — `MouseInput`
//!   does not carry a position, only the button + state),
//! - press / release with click semantics on **release** (not press) —
//!   matches the conventional desktop drag-cancel behaviour and
//!   acceptance criterion #11,
//! - hit-test in **reverse paint order** against the per-frame
//!   `Vec<HitTestEntry>` the walker produced — topmost wins,
//! - DPI conversion (winit reports physical-pixel positions; the
//!   per-frame `HitTestEntry` rects are in the same logical-pixel space
//!   the scene is laid out in, so we scale by `scale_factor`).
//!
//! Action firing is **never** invoked while holding a borrow on the
//! `AppTree` or the registry (Sprint 4 anti-pattern carry-forward):
//! `process_click` returns the `ActionId` and `node_id`; the caller
//! invokes the action through `AppTree::actions().fire(id)` only after
//! all reads of the tree have ended. The walker's hit entries already
//! cloned everything needed to fire safely.

#[cfg(not(target_arch = "wasm32"))]
use winit::dpi::PhysicalPosition;
#[cfg(not(target_arch = "wasm32"))]
use winit::event::{ElementState, MouseButton};

use mkui_runtime::ActionId;

use crate::types::{rect_contains, Point};
use crate::walker::HitTestEntry;

/// Pointer-state machine. One instance owned by `WgpuApp`; updated each
/// frame from the winit event stream.
#[derive(Debug, Default, Clone, Copy)]
pub struct PointerState {
    /// Most recent cursor position in *physical* pixels. `None` until the
    /// first `CursorMoved` event lands — wgpu apps that get a click on
    /// the very first frame before any cursor motion would otherwise
    /// dereference a default-initialised origin.
    physical_pos: Option<(f64, f64)>,
    /// True between press and release. Used to scope click semantics to
    /// "released over a registered hit region after a press anywhere".
    pressing: bool,
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

    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_mouse(&mut self, button: MouseButton, state: ElementState) -> MouseUpdate {
        if button != MouseButton::Left {
            return MouseUpdate::Ignored;
        }
        match state {
            ElementState::Pressed => {
                self.pressing = true;
                MouseUpdate::Pressed
            }
            ElementState::Released => {
                let was_pressing = self.pressing;
                self.pressing = false;
                if was_pressing {
                    MouseUpdate::Released
                } else {
                    MouseUpdate::Ignored
                }
            }
        }
    }

    pub fn cursor_logical(&self, scale_factor: f64) -> Option<Point> {
        let (x, y) = self.physical_pos?;
        let scale = scale_factor.max(f64::EPSILON);
        Some(Point::new((x / scale) as f32, (y / scale) as f32))
    }
}

/// Outcome of a `update_mouse` call — narrows the caller's downstream
/// logic so the event loop only hit-tests on a real release-after-press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MouseUpdate {
    Pressed,
    Released,
    Ignored,
}

/// Result of a release-time hit-test. `Some` when the cursor landed inside
/// a registered interactive region; the caller fires `on_press` through
/// the tree's `ActionRegistry`. Held separately from `HitTestEntry` so
/// the caller doesn't accidentally re-borrow the per-frame vec while
/// running the action closure (anti-pattern carry-forward from Sprint 4).
#[derive(Debug, Clone, Copy)]
pub struct ClickHit {
    pub node_id: mkui_runtime::NodeId,
    pub on_press: Option<ActionId>,
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
            node_id: entry.node_id,
            on_press: entry.on_press,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Rect, Size};
    use mkui_runtime::NodeId;

    fn entry(x: f32, y: f32, w: f32, h: f32, idx: u32) -> HitTestEntry {
        HitTestEntry {
            rect: Rect::new(Point::new(x, y), Size::new(w, h)),
            node_id: NodeId::from_raw(idx, 0),
            on_press: None,
        }
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
        assert_eq!(hit.node_id, NodeId::from_raw(2, 0));
    }

    #[test]
    fn hit_test_falls_through_to_underlying_when_overlap_misses() {
        let entries = vec![
            entry(0.0, 0.0, 100.0, 100.0, 1),
            entry(80.0, 80.0, 10.0, 10.0, 2),
        ];
        let hit = hit_test(&entries, Point::new(30.0, 30.0)).expect("inside outer only");
        assert_eq!(hit.node_id, NodeId::from_raw(1, 0));
    }
}
