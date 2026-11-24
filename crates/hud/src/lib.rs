#![forbid(unsafe_code)]
//! # mkui-hud — second-class, host-reusable HUD plumbing
//!
//! Per [ADR 0013](../../../docs/architecture/0013-mkui-goal-and-three-backend-bar.md)
//! and its appended HUD-boundary section, `mkui-hud` holds HUD-adjacent code
//! that is reusable *across any embedding host* building a game-style HUD on
//! top of `mkui-wgpu`'s host-embedding mode (#173/#175) — but is not a true
//! cross-backend primitive (that belongs in `mkui-core`/`mkui-wgpu`) and does
//! not carry a specific host's own look or meaning (that belongs in the
//! host's own repository, not even here).
//!
//! `mkui-hud` is explicitly second-class: it is never a dependency of
//! `mkui-core` or any backend crate, and it is not held to the three-backend
//! bar (ADR 0013's shadcn/ui-catalogue "done" test does not apply to it).
//!
//! This crate is currently a scaffold (#290) — no HUD component logic has
//! moved into it yet. See #230, #250, #262 for the first candidate
//! deliverables and their recorded dispositions.

/// Scaffold marker so downstream tooling (and this crate's own smoke test)
/// has something concrete to link against before the first real HUD helper
/// lands.
pub const CRATE_NAME: &str = "mkui-hud";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_mkui_hud() {
        assert_eq!(CRATE_NAME, "mkui-hud");
    }
}
