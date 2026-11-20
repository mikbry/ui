//! Action plumbing — the single shared callback table.
//!
//! Every action a node can fire (button press, future toggle change, …) is
//! stored once in [`ActionRegistry`] and addressed by [`ActionId`] from the
//! node. This decouples action storage from node storage in two important
//! ways:
//!
//! 1. **FFI portability.** C and Python cannot hold a `Box<dyn Fn>` directly.
//!    The bindings own their own callback tables keyed by `ActionId`; the
//!    runtime only sees the id. Rust hosts can register a local closure
//!    through [`ActionRegistry::register_local`] and skip the dance.
//! 2. **Generation reservation.** [`ActionId`] carries a `generation` field
//!    alongside its `index` so a stale id can be rejected (returning `None`)
//!    rather than firing a recycled slot's callback. [`ActionRegistry::remove`]
//!    bumps a slot's generation and returns it to the `free` pool, so a
//!    later lookup with the pre-removal id is rejected even after the slot
//!    is recycled by a later registration (#83, building on the reservation
//!    laid down by #70).
//!
//! The registry is intentionally **not** `Send + Sync`. mkui has no
//! multithreaded runtime today, and adding the bounds prematurely would force
//! every binding to thread them through closures that never cross threads.
//! See the crate-level docs.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// Opaque action handle. The `index` points into [`ActionRegistry`]'s slot
/// vector; `generation` distinguishes a recycled slot from the id that
/// originally occupied it (#83) — [`ActionRegistry::remove`] bumps it, so
/// an id minted before a remove+reuse cycle is rejected by
/// [`ActionRegistry::get`] after the slot is recycled.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct ActionId {
    index: u32,
    generation: u32,
}

impl ActionId {
    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn generation(&self) -> u32 {
        self.generation
    }

    /// Build an `ActionId` from raw parts. Exposed for FFI/test code that
    /// carries an `ActionId` across a non-Rust boundary as two `u32`s.
    pub fn from_raw(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

/// Locally-stored action closure. The `RefCell` permits in-place mutation
/// of captured state from inside the closure; the `Rc` lets the same closure
/// be referenced from multiple ActionIds (e.g. two buttons sharing a handler)
/// without forcing the caller to clone the underlying logic.
pub type LocalAction = Rc<RefCell<dyn FnMut(&mut RuntimeCtx)>>;

/// Single-threaded action table. Slots are addressed by `ActionId`. Each slot
/// carries a `generation` so a stale id can be rejected on lookup;
/// [`ActionRegistry::remove`] bumps it and returns the slot to `free` so a
/// later registration can recycle it (#83).
#[derive(Default)]
pub struct ActionRegistry {
    slots: Vec<Slot>,
    /// Indices of removed slots, available for reuse by the next
    /// `register_local_action`/`register_remote` call (#83).
    free: Vec<u32>,
}

struct Slot {
    /// Bumped by [`ActionRegistry::remove`] every time this slot is freed,
    /// so a pre-removal `ActionId` is rejected once the slot is recycled.
    generation: u32,
    action: Option<LocalAction>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a local Rust closure and return its `ActionId`.
    pub fn register_local<F>(&mut self, f: F) -> ActionId
    where
        F: FnMut(&mut RuntimeCtx) + 'static,
    {
        self.register_local_action(Rc::new(RefCell::new(f)))
    }

    /// Register an externally-supplied `LocalAction`. The binding layer uses
    /// this when it needs to wrap a foreign callback (Python `PyObject`,
    /// C function pointer) in an Rc<RefCell<...>> first.
    pub fn register_local_action(&mut self, action: LocalAction) -> ActionId {
        self.alloc_slot(Some(action))
    }

    /// Register an `ActionId` with no local handler — used by FFI bindings
    /// that own their own callback table and only need the runtime to
    /// allocate a stable id.
    pub fn register_remote(&mut self) -> ActionId {
        self.alloc_slot(None)
    }

    /// Shared allocation path for `register_local_action`/`register_remote`:
    /// reuse a freed slot (its generation already bumped by
    /// [`ActionRegistry::remove`]) if one is available, otherwise append a
    /// fresh slot at `generation == 0`.
    fn alloc_slot(&mut self, action: Option<LocalAction>) -> ActionId {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.action = action;
            ActionId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                action,
            });
            ActionId {
                index,
                generation: 0,
            }
        }
    }

    /// Remove `id`'s action: bump the slot's generation and return its index
    /// to the `free` pool so a later registration can recycle it. Returns
    /// the previously-stored action, or `None` if the slot was already
    /// empty (an FFI-only registration) or `id` is stale/missing — removal
    /// is idempotent, a double `remove` on the same id is a no-op the
    /// second time.
    pub fn remove(&mut self, id: ActionId) -> Option<LocalAction> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let action = slot.action.take();
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(id.index);
        action
    }

    /// Whether `id` still names a live slot — `true` if the index is in
    /// range and the generation matches, regardless of whether the slot
    /// holds a local closure. Unlike [`ActionRegistry::get`], this also
    /// answers `true` for a live FFI-only (`register_remote`) slot, so a
    /// binding's own foreign-callback table (keyed by the same `ActionId`)
    /// can check liveness after a [`ActionRegistry::remove`] without the
    /// registry handing back a payload it was never given (#83).
    pub fn contains(&self, id: ActionId) -> bool {
        self.slots
            .get(id.index as usize)
            .is_some_and(|slot| slot.generation == id.generation)
    }

    /// Look up a registered local action by id. Returns `None` if the slot
    /// is empty (FFI-only registration) or if the id's `generation` does not
    /// match the slot — either a forged/out-of-band id, or one minted before
    /// a [`ActionRegistry::remove`] + recycle cycle (#83).
    pub fn get(&self, id: ActionId) -> Option<&LocalAction> {
        let slot = self.slots.get(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        slot.action.as_ref()
    }

    /// Fire the local action for `id` (no-op if the slot is FFI-only or the
    /// id is stale). Returns the [`RuntimeCtx`] the action populated so the
    /// caller can observe dirty + emitted signals.
    ///
    /// Bindings that own their own callback table do **not** route through
    /// this method — they look up the id in their own table and fire from
    /// there. The runtime stays callback-source-agnostic.
    pub fn fire(&self, id: ActionId) -> RuntimeCtx {
        let mut ctx = RuntimeCtx::default();
        if let Some(action) = self.get(id) {
            (action.borrow_mut())(&mut ctx);
        }
        ctx
    }

    /// Number of registered slots, including FFI-only slots with no local
    /// handler. Used by parity tests + diagnostics.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}

impl std::fmt::Debug for ActionRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActionRegistry")
            .field("slots", &self.slots.len())
            .finish()
    }
}

/// Context passed to a firing action. Actions mark the tree dirty and emit
/// `RuntimeSignal`s; they do **not** reach for renderer APIs directly.
/// Renderers observe the signals on the next frame.
#[derive(Debug, Default)]
pub struct RuntimeCtx {
    dirty: bool,
    emitted: Vec<RuntimeSignal>,
}

impl RuntimeCtx {
    /// Mark the tree dirty so the next render redraws.
    pub fn mark_dirty(&mut self) {
        self.dirty = true;
        self.emitted.push(RuntimeSignal::RequestRedraw);
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn emit(&mut self, signal: RuntimeSignal) {
        self.emitted.push(signal);
    }

    pub fn drain_emitted(&mut self) -> Vec<RuntimeSignal> {
        std::mem::take(&mut self.emitted)
    }

    pub fn emitted(&self) -> &[RuntimeSignal] {
        &self.emitted
    }
}

/// Signal an action can ask the renderer to observe.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum RuntimeSignal {
    /// Renderer should schedule a redraw on the next frame.
    RequestRedraw,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn register_local_returns_unique_ids() {
        let mut reg = ActionRegistry::new();
        let a = reg.register_local(|_| {});
        let b = reg.register_local(|_| {});
        assert_ne!(a.index(), b.index());
    }

    #[test]
    fn register_remote_returns_an_id_without_a_handler() {
        let mut reg = ActionRegistry::new();
        let id = reg.register_remote();
        assert!(
            reg.get(id).is_none(),
            "remote-only registration must not produce a local closure"
        );
    }

    #[test]
    fn fire_invokes_the_registered_closure_and_collects_signals() {
        let mut reg = ActionRegistry::new();
        let count = Rc::new(Cell::new(0u32));
        let count_in = Rc::clone(&count);
        let id = reg.register_local(move |ctx| {
            count_in.set(count_in.get() + 1);
            ctx.mark_dirty();
        });

        let ctx = reg.fire(id);
        assert_eq!(count.get(), 1);
        assert!(ctx.is_dirty());
        assert_eq!(ctx.emitted(), &[RuntimeSignal::RequestRedraw]);
    }

    #[test]
    fn fire_with_wrong_generation_is_noop() {
        // Build an id that points at a real slot but with the wrong
        // generation — a forged/out-of-band id, independent of removal.
        let mut reg = ActionRegistry::new();
        let real = reg.register_local(|ctx| ctx.mark_dirty());
        let stale = ActionId::from_raw(real.index(), real.generation().wrapping_add(1));
        let ctx = reg.fire(stale);
        assert!(!ctx.is_dirty(), "stale id must not fire");
    }

    #[test]
    fn remove_returns_the_previously_stored_action() {
        let mut reg = ActionRegistry::new();
        let id = reg.register_local(|_| {});
        assert!(reg.get(id).is_some());

        let removed = reg.remove(id);
        assert!(removed.is_some(), "remove must hand back the stored action");
        assert!(
            reg.get(id).is_none(),
            "the removed id must no longer resolve"
        );
    }

    #[test]
    fn remove_on_an_ffi_only_slot_returns_none_but_still_frees_it() {
        let mut reg = ActionRegistry::new();
        let id = reg.register_remote();
        assert!(reg.remove(id).is_none(), "no local action was ever stored");
        assert!(reg.get(id).is_none());
    }

    #[test]
    fn remove_on_a_stale_id_is_a_noop() {
        let mut reg = ActionRegistry::new();
        let stale = ActionId::from_raw(99, 0);
        assert!(reg.remove(stale).is_none());
    }

    #[test]
    fn remove_twice_on_the_same_id_is_idempotent() {
        let mut reg = ActionRegistry::new();
        let id = reg.register_local(|_| {});
        assert!(reg.remove(id).is_some());
        assert!(
            reg.remove(id).is_none(),
            "a second remove on the same (now-stale) id must be a no-op"
        );
    }

    #[test]
    fn contains_is_true_for_a_live_remote_slot_even_though_get_is_none() {
        // `register_remote` slots never carry a local closure, so `get`
        // always returns `None` for them. `contains` must still report
        // them live — a binding's foreign-callback table (C/Py) uses this
        // to detect removal, and `get` alone can't distinguish "live,
        // FFI-only" from "removed" (#83).
        let mut reg = ActionRegistry::new();
        let id = reg.register_remote();
        assert!(reg.get(id).is_none());
        assert!(reg.contains(id));

        reg.remove(id);
        assert!(!reg.contains(id), "removed id must no longer be contained");
    }

    #[test]
    fn removed_slot_is_recycled_with_a_bumped_generation() {
        let mut reg = ActionRegistry::new();
        let first = reg.register_local(|_| {});
        reg.remove(first);

        let second = reg.register_local(|_| {});
        assert_eq!(
            second.index(),
            first.index(),
            "the freed slot must be reused"
        );
        assert_ne!(
            second.generation(),
            first.generation(),
            "the recycled slot must carry a new generation"
        );

        // The pre-removal id must not resolve to the new occupant's action.
        assert!(reg.get(first).is_none());
        assert!(reg.get(second).is_some());
    }
}
