# STATE — mkui Slug rendering chevalier mission

**Last updated:** 2026-07-28T01:00:00Z
**Current phase:** 1
**Phase status:** dame-review

## Phase 1
- PR: #160 (https://github.com/mikbry/ui/pull/160)
- Opened: 2026-07-28
- Codex verdicts: REQUEST_CHANGES (round 1, sha 345dbb6); APPROVE (round 2, sha af4d21f)
- Dame verdicts: pending (dispatching now)
- Merged: pending
- Notes: Implemented Codex 8-step-plan steps 1-3 (vertical band upload, dual-ray
  weighted coverage with `0x2e74` root eligibility, `fwidth`-derived AA width)
  in `crates/mkui-vector2d-wgpu`. Self-verified against the ratified
  `reference-harness/` adapter under real Docker + Lavapipe (the same pinned
  `rust:1.89-bookworm` image used to capture the goldens). Codex round 1 found
  a real defect: `calc_coverage`'s clamp was split from the reference's
  (internal) placement, exposing a genuinely unbounded debug value at the
  ratified `plus` fixture's self-overlap (two same-winding rectangles hitting
  winding magnitude 2), and the parity test's differing-pixel count was
  vacuous relative to its delta-threshold check. Fixed both (sha af4d21f):
  clamp now lives inside `calc_coverage` matching the reference exactly, and
  the parity test independently checks delta / differing-pixel-count / SSIM.
  Round-2 Lavapipe re-verification: 24/24 comparisons, max per-channel Δ ≤
  1/255, differing pixels ≤ 4 (limit 10), SSIM = 1.000000 (floor 0.995).
  Fixed one pre-existing hand-authored GPU test fixture
  (`slug_gpu.rs::square_glyph`) that supplied empty vertical bands — valid
  under the old single-ray shader, not under Phase 1's dual-ray contract.
  `cargo fmt --check`, workspace clippy (`-D warnings`), and `cargo test` for
  `mkui-vector2d`, `mkui-vector2d-wgpu`, and `mkui-wgpu` (default, `slug`, and
  `gpu-tests,slug` under Lavapipe: 147/147) all pass. Dispatching dame next.

## Phase 2
- PR: pending
- Status: not started

## Phase 3
- PR: pending
- Status: not started

## Phase 4
- PR: pending
- Status: not started
