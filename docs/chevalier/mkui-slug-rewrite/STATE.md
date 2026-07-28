# STATE — mkui Slug rendering chevalier mission

**Last updated:** 2026-07-28T03:00:00Z
**Current phase:** 2
**Phase status:** codex-review

## Phase 1
- PR: #160 (https://github.com/mikbry/ui/pull/160)
- Opened: 2026-07-28
- Codex verdicts: REQUEST_CHANGES (round 1, sha 345dbb6); APPROVE (round 2, sha af4d21f)
- Dame verdicts: BLESS (round 1, sha d99d1d7) — dame-review dispatched via
  CHARTER's fallback shape (`miky agent assign --brief-file dame-rubric.md`)
  returned an explicit "Verdict: **APPROVE**" (mapped to BLESS per
  `verdict_mapping`); the substrate's own auto-classification header
  mis-tagged it "comment" (no literal `VERDICT:`-line match), a marker-regex
  artifact, not a substantive finding — the review body is unambiguous and
  corroborated by real CI: all 30 checks green, including `gpu-offscreen`
  (Lavapipe) and `feature-slug`. Cross-phase invariants independently
  re-verified before merge: zero commits on `reference-harness/` since
  `9f76af3`, zero commits on `{CHARTER,dame-rubric,codex-8-step-plan}.md`
  since `b338bd9`, full diff scoped to `crates/mkui-vector2d-wgpu/`,
  `crates/mkui-wgpu/`, `CHANGELOG.md`, and this file.
- Merged: 2026-07-28T12:04:36Z (squash sha `7cb352c4b683f559a8123852599ce68b54369509`)
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
- PR: #161 (https://github.com/mikbry/ui/pull/161)
- Opened: 2026-07-28
- Codex verdicts: pending (dispatching now)
- Dame verdicts: pending
- Merged: pending
- Notes: Implemented Codex 8-step-plan steps 4-5 — bounded 2D half-physical-
  pixel dilation (replacing the flat 1.5px constant with `0.5/scale`,
  font-unit-space dilation before pixel projection — mathematically the
  closed form the reference's Jacobian-based `slug_dilate` collapses to for
  mkui's non-skewed transform) and a band overlap epsilon (`units_per_em /
  1024.0`, additive `SlugConfig.units_per_em` field defaulting to `1.0`, every
  existing call site unaffected). Self-verified under the same pinned Docker +
  Lavapipe image as Phase 1: all 24 Phase 1 comparisons still hold (Δ ≤ 1/255,
  SSIM = 1.000000) after both changes together, plus a new thin-gap
  regression scan on `o`/`g` at 3 DPIs (one false-positive caught and fixed:
  `g_2x` has a legitimate near-zero-alpha texel that's byte-identical to the
  reference, so the scan now only flags texels absent from the reference's
  own golden). `cargo fmt --check`, workspace clippy (`-D warnings`,
  including `slug`/`gpu-tests,slug`/`atoms-on-wgpu`/`text` feature
  combinations), and `cargo test` for `mkui-vector2d`, `mkui-vector2d-wgpu`,
  `mkui-wgpu` (default, `slug`, `gpu-tests,slug` under Lavapipe: 148/148) all
  pass. Dispatching Codex code-review next.
- Known substrate quirk (carried from Phase 1): dame dispatched via the
  `--brief-file dame-rubric.md` fallback shape returns its verdict as prose
  containing an explicit `Verdict: **APPROVE**`/`REQUEST_CHANGES` line, but
  the `miky agent assign` outbox's own auto-classification header looks for a
  literal, unformatted `VERDICT:` marker and mis-tags anything else
  "comment". Read the review body, not the header, when applying
  `verdict_mapping`.

## Phase 3
- PR: pending
- Status: not started

## Phase 4
- PR: pending
- Status: not started
