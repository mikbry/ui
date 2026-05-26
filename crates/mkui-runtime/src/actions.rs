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
//! 2. **Generation counters.** Removing an action invalidates its
//!    [`ActionId`] — a later lookup with the stale id returns `None` instead
//!    of accidentally firing a recycled slot's callback (Codex round-7
//!    anti-pattern guard against use-after-free).
//!
//! The registry is intentionally **not** `Send + Sync`. mkui has no
//! multithreaded runtime today, and adding the bounds prematurely would force
//! every binding to thread them through closures that never cross threads.
//! See the crate-level docs.

use std::cell::RefCell;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

/// Opaque action handle. The `index` points into [`ActionRegistry`]'s slot
/// vector; `generation` distinguishes recycled slots from the original.
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

/// Single-threaded action table. Slots are addressed by `ActionId` and use a
/// generation counter to invalidate stale ids after removal (Codex Q3 / Q6
/// anti-pattern guard).
#[derive(Default)]
pub struct ActionRegistry {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

struct Slot {
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
        if let Some(idx) = self.free.pop() {
            let slot = &mut self.slots[idx as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.action = Some(action);
            ActionId {
                index: idx,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                action: Some(action),
            });
            ActionId {
                index,
                generation: 0,
            }
        }
    }

    /// Register an `ActionId` with no local handler — used by FFI bindings
    /// that own their own callback table and only need the runtime to
    /// allocate a stable id.
    pub fn register_remote(&mut self) -> ActionId {
        if let Some(idx) = self.free.pop() {
            let slot = &mut self.slots[idx as usize];
            slot.generation = slot.generation.wrapping_add(1);
            slot.action = None;
            ActionId {
                index: idx,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                action: None,
            });
            ActionId {
                index,
                generation: 0,
            }
        }
    }

    /// Look up a registered local action by id. Returns `None` if the slot
    /// is empty (FFI-only registration) or if the id has been invalidated
    /// by a later removal.
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

    /// Number of registered slots, including empty ones held open by their
    /// generation counter. Used by parity tests + diagnostics.
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
            .field("free", &self.free.len())
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
    fn fire_with_stale_id_is_noop() {
        // Build an id that points at a real slot but with the wrong
        // generation — simulates an id held past a (future) remove call.
        let mut reg = ActionRegistry::new();
        let real = reg.register_local(|ctx| ctx.mark_dirty());
        let stale = ActionId::from_raw(real.index(), real.generation().wrapping_add(1));
        let ctx = reg.fire(stale);
        assert!(!ctx.is_dirty(), "stale id must not fire");
    }
}
