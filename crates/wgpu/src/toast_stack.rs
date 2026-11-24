//! `ToastStack` — toast/banner stack state: dismiss, minimise, unread
//! count, history (#193).
//!
//! Pure logic, no winit/wgpu dependency: like [`crate::pointer::PointerState`]
//! and [`crate::text_edit::TextEditState`], one instance is owned by the
//! caller and kept across frames (the renderer is immediate-mode and retains
//! no component tree between frames). [`crate::components::toast_stack`]
//! renders the current state each frame and pushes [`crate::types::HitRegion`]
//! entries the caller's pointer router feeds back in as a [`ToastTarget`] via
//! [`ToastStack::apply`].
//!
//! ## Unread count
//!
//! [`ToastStack::unread_count`] is always derived from the active list (a
//! count of `!read` entries), never a maintained counter — a cached counter
//! is exactly how "updates correctly as items are dismissed/read" bugs get
//! introduced (two code paths that both need to remember to decrement it).
//! The read/unread transitions are:
//!
//! - [`ToastStack::push`] starts a toast unread.
//! - [`ToastStack::mark_read`] (also reachable via [`ToastTarget::MarkRead`]
//!   — a body click, not just a host-driven call) marks it read without
//!   otherwise touching it.
//! - [`ToastStack::minimise`] collapses it to compact form; it does **not**
//!   mark it read — a minimised toast can still be unread (e.g. a game HUD
//!   that auto-collapses old toasts to make room, without the player having
//!   looked at them).
//! - [`ToastStack::dismiss`] always marks the toast read (there is no such
//!   thing as an unread history entry) before moving it to history.
//!
//! ## History
//!
//! Dismissed toasts move to [`ToastStack::history`], most-recent-first,
//! capped at [`HISTORY_CAP`] entries (oldest dropped) so a long-running host
//! session can't grow this unboundedly. [`ToastStack::history_visible`] is
//! state, not a render-only flag, so "on demand" (the issue's acceptance
//! wording) is part of the state machine and testable without a `Scene`.

/// Opaque, stable identifier for a toast. Stays valid for the toast's whole
/// lifetime (active *and* once it moves to history) — never reused, and
/// never an index, so a hit region captured on one frame still names the
/// right toast after an earlier dismiss has shifted every later index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToastId(u64);

/// A hit-test target [`crate::components::toast_stack`] pushes into
/// the caller's `PanelLayout<T>` — feed it back through [`ToastStack::apply`]
/// once the caller's pointer router resolves a click to one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToastTarget {
    Dismiss(ToastId),
    ToggleMinimise(ToastId),
    MarkRead(ToastId),
}

/// One toast/banner entry. Fields are public, same as this crate's other
/// plain state structs ([`crate::text_edit::TextEditState`]'s siblings) —
/// there is no invariant between them beyond what [`ToastStack`]'s methods
/// already enforce on construction/mutation.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: ToastId,
    pub variant: super::theme::ToastVariant,
    pub message: String,
    pub minimised: bool,
    pub read: bool,
}

/// Dismissed toasts beyond this count (oldest first) are dropped. A
/// long-running game-UI host would otherwise grow `history` forever.
pub const HISTORY_CAP: usize = 50;

/// Caller-owned toast/banner stack state. See the module docs for the
/// read/unread and history contracts.
#[derive(Debug, Clone, Default)]
pub struct ToastStack {
    active: Vec<Toast>,
    history: Vec<Toast>,
    history_visible: bool,
    next_id: u64,
}

impl ToastStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a new, unread, non-minimised toast onto the stack. Returns its
    /// [`ToastId`] so the caller can address it later (e.g. an auto-expiry
    /// timer the caller drives, out of scope for #193's acceptance).
    pub fn push(
        &mut self,
        variant: super::theme::ToastVariant,
        message: impl Into<String>,
    ) -> ToastId {
        let id = ToastId(self.next_id);
        self.next_id += 1;
        self.active.push(Toast {
            id,
            variant,
            message: message.into(),
            minimised: false,
            read: false,
        });
        id
    }

    /// Dispatch a [`ToastTarget`] resolved from a hit-test. Returns `true`
    /// if `target` named a toast that is still active (a stale target from
    /// an already-dismissed toast, or an unknown id, is a no-op).
    pub fn apply(&mut self, target: ToastTarget) -> bool {
        match target {
            ToastTarget::Dismiss(id) => self.dismiss(id),
            ToastTarget::ToggleMinimise(id) => {
                let Some(toast) = self.active.iter_mut().find(|t| t.id == id) else {
                    return false;
                };
                toast.minimised = !toast.minimised;
                true
            }
            ToastTarget::MarkRead(id) => self.mark_read(id),
        }
    }

    /// Mark `id` read without dismissing or minimising it. No-op (returns
    /// `false`) for an unknown id or one already read.
    pub fn mark_read(&mut self, id: ToastId) -> bool {
        let Some(toast) = self.active.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        if toast.read {
            return false;
        }
        toast.read = true;
        true
    }

    /// Collapse `id` to its compact form. Does **not** mark it read — see
    /// the module docs. No-op for an unknown id.
    pub fn minimise(&mut self, id: ToastId) -> bool {
        let Some(toast) = self.active.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        toast.minimised = true;
        true
    }

    /// Expand `id` back out of its compact form. No-op for an unknown id.
    pub fn restore(&mut self, id: ToastId) -> bool {
        let Some(toast) = self.active.iter_mut().find(|t| t.id == id) else {
            return false;
        };
        toast.minimised = false;
        true
    }

    /// Move `id` from the active stack to [`Self::history`] (most-recent-
    /// first), forcing it read first. No-op — and no duplicate history
    /// entry — for an unknown id or one already dismissed.
    pub fn dismiss(&mut self, id: ToastId) -> bool {
        let Some(index) = self.active.iter().position(|t| t.id == id) else {
            return false;
        };
        let mut toast = self.active.remove(index);
        toast.read = true;
        toast.minimised = false;
        self.history.insert(0, toast);
        self.history.truncate(HISTORY_CAP);
        true
    }

    /// Count of active toasts not yet marked read. Always derived — see the
    /// module docs.
    pub fn unread_count(&self) -> usize {
        self.active.iter().filter(|t| !t.read).count()
    }

    pub fn active(&self) -> &[Toast] {
        &self.active
    }

    /// Dismissed toasts, most-recent-first, capped at [`HISTORY_CAP`].
    pub fn history(&self) -> &[Toast] {
        &self.history
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    /// Whether the on-demand history list should currently render. State,
    /// not a render-only flag, so "on demand" is part of the state machine.
    pub fn history_visible(&self) -> bool {
        self.history_visible
    }

    pub fn set_history_visible(&mut self, visible: bool) {
        self.history_visible = visible;
    }

    pub fn toggle_history_visible(&mut self) {
        self.history_visible = !self.history_visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ToastVariant;

    #[test]
    fn multiple_toasts_stack_without_colliding_ids() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        let b = stack.push(ToastVariant::Warning, "b");
        let c = stack.push(ToastVariant::Destructive, "c");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_eq!(stack.active().len(), 3);
        assert_eq!(
            stack.active().iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![a, b, c]
        );
    }

    #[test]
    fn dismiss_and_minimise_both_work_per_item() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        let b = stack.push(ToastVariant::Info, "b");

        assert!(stack.minimise(a));
        assert!(stack.active().iter().find(|t| t.id == a).unwrap().minimised);
        assert!(!stack.active().iter().find(|t| t.id == b).unwrap().minimised);

        assert!(stack.dismiss(b));
        assert_eq!(stack.active().len(), 1);
        assert_eq!(stack.active()[0].id, a);
        assert_eq!(stack.history().len(), 1);
        assert_eq!(stack.history()[0].id, b);
    }

    #[test]
    fn dismissing_an_already_dismissed_or_unknown_id_is_a_no_op() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");

        assert!(stack.dismiss(a));
        assert!(
            !stack.dismiss(a),
            "re-dismissing must not duplicate history"
        );
        assert_eq!(stack.history().len(), 1);

        let unknown = ToastId(9999);
        assert!(!stack.dismiss(unknown));
        assert!(!stack.minimise(unknown));
        assert!(!stack.restore(unknown));
        assert!(!stack.mark_read(unknown));
        assert!(!stack.apply(ToastTarget::ToggleMinimise(unknown)));
        assert!(!stack.apply(ToastTarget::MarkRead(unknown)));
    }

    #[test]
    fn apply_mark_read_reaches_the_same_transition_as_a_direct_call() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        assert_eq!(stack.unread_count(), 1);
        assert!(stack.apply(ToastTarget::MarkRead(a)));
        assert_eq!(stack.unread_count(), 0);
    }

    #[test]
    fn stale_target_after_a_dismiss_does_not_act_on_the_wrong_toast() {
        // Regression guard for index-based targets: dismiss item 0, then
        // make sure a target captured for item 2 (on the pre-dismiss frame)
        // still names item 2 by id, not whatever now sits at its old index.
        let mut stack = ToastStack::new();
        let first = stack.push(ToastVariant::Info, "first");
        let _second = stack.push(ToastVariant::Info, "second");
        let third = stack.push(ToastVariant::Info, "third");

        stack.dismiss(first);

        assert!(stack.apply(ToastTarget::ToggleMinimise(third)));
        assert!(
            stack
                .active()
                .iter()
                .find(|t| t.id == third)
                .unwrap()
                .minimised,
            "toggling the id captured before the dismiss must still hit the same toast"
        );
    }

    #[test]
    fn unread_count_updates_as_items_are_dismissed_and_read() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        let b = stack.push(ToastVariant::Info, "b");
        let c = stack.push(ToastVariant::Info, "c");
        assert_eq!(stack.unread_count(), 3);

        assert!(stack.mark_read(a));
        assert_eq!(stack.unread_count(), 2);

        // Minimising does not mark read.
        stack.minimise(b);
        assert_eq!(stack.unread_count(), 2);

        // Dismissing an unread toast still drops the unread count — it left
        // the active set, read or not.
        stack.dismiss(c);
        assert_eq!(stack.unread_count(), 1);

        // mark_read then dismiss must decrement only once (not double-count
        // the same toast going from unread -> read -> gone).
        stack.mark_read(b);
        assert_eq!(stack.unread_count(), 0);
        stack.dismiss(b);
        assert_eq!(stack.unread_count(), 0);
    }

    #[test]
    fn history_list_shows_previously_dismissed_items_on_demand() {
        let mut stack = ToastStack::new();
        assert!(!stack.history_visible(), "history starts hidden");

        let a = stack.push(ToastVariant::Info, "a");
        let b = stack.push(ToastVariant::Warning, "b");
        stack.dismiss(a);
        stack.dismiss(b);

        // Most-recent-first.
        assert_eq!(
            stack.history().iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![b, a]
        );

        stack.set_history_visible(true);
        assert!(stack.history_visible());
        stack.toggle_history_visible();
        assert!(!stack.history_visible());
    }

    #[test]
    fn history_is_capped_oldest_dropped_first() {
        let mut stack = ToastStack::new();
        let mut ids = Vec::new();
        for i in 0..(HISTORY_CAP + 5) {
            let id = stack.push(ToastVariant::Info, format!("toast {i}"));
            ids.push(id);
            stack.dismiss(id);
        }
        assert_eq!(stack.history().len(), HISTORY_CAP);
        // Most-recent-first, so the newest pushed id is still at the front...
        assert_eq!(stack.history()[0].id, *ids.last().unwrap());
        // ...and the oldest ids were dropped off the back.
        assert!(!stack.history().iter().any(|t| t.id == ids[0]));
    }

    #[test]
    fn clear_history_empties_the_list() {
        let mut stack = ToastStack::new();
        let a = stack.push(ToastVariant::Info, "a");
        stack.dismiss(a);
        assert_eq!(stack.history().len(), 1);
        stack.clear_history();
        assert!(stack.history().is_empty());
    }
}
