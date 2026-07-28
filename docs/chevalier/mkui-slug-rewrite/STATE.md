# STATE — mkui Slug rendering chevalier mission

**Last updated:** 2026-07-28T00:00:00Z
**Current phase:** 1
**Phase status:** codex-review

## Phase 1
- PR: #160 (https://github.com/mikbry/ui/pull/160)
- Opened: 2026-07-28
- Codex verdicts: pending (dispatching now)
- Dame verdicts: pending
- Merged: pending
- Notes: Implemented Codex 8-step-plan steps 1-3 (vertical band upload, dual-ray
  weighted coverage with `0x2e74` root eligibility, `fwidth`-derived AA width)
  in `crates/mkui-vector2d-wgpu`. Self-verified against the ratified
  `reference-harness/` adapter under real Docker + Lavapipe (the same pinned
  `rust:1.89-bookworm` image used to capture the goldens): all 24 glyph×DPI
  comparisons pass at max per-channel Δ ≤ 1/255, 0 differing pixels (rubric
  thresholds: Δ ≤ 4/255, ≤ 10 pixels). Fixed one pre-existing hand-authored GPU
  test fixture (`slug_gpu.rs::square_glyph`) that supplied empty vertical
  bands — valid under the old single-ray shader, not under Phase 1's dual-ray
  contract. `cargo fmt --check`, workspace clippy (`-D warnings`), and
  `cargo test` for `mkui-vector2d`, `mkui-vector2d-wgpu`, and `mkui-wgpu`
  (default, `slug`, and `gpu-tests,slug` under Lavapipe) all pass. Dispatching
  Codex code-review next.

## Phase 2
- PR: pending
- Status: not started

## Phase 3
- PR: pending
- Status: not started

## Phase 4
- PR: pending
- Status: not started
