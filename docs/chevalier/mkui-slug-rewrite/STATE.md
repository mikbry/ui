# STATE — mkui Slug rendering chevalier mission

**Last updated:** 2026-07-28T04:00:00Z
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
- Codex verdicts: REQUEST_CHANGES (round 1, sha bdfdfbd); round 2 pending
  (dispatching now)
- Dame verdicts: pending
- Merged: pending
- Notes: Implemented Codex 8-step-plan steps 4-5 — bounded 2D half-physical-
  pixel dilation and a band overlap epsilon (additive `SlugConfig.units_per_em`
  field defaulting to `1.0`, every existing call site unaffected). Codex round
  1 found three real defects, all fixed (sha pending push):
  1. **Dilation was logical-pixel, not physical-pixel.** The first cut
     computed `0.5/scale` in the caller's own pixel space — correct only when
     that space has no logical/physical split, which mkui-wgpu's real
     windowed renderer does not (Slug quads project against the *logical*
     viewport, #97). At `device_pixel_ratio` 1.5x/2x the physical dilation
     would have become 0.75/1.0px instead of 0.5px. Fixed: `half_pixel_dilation_units`
     now takes `device_pixel_ratio` too (`0.5/(device_pixel_ratio*scale)`);
     `SlugAdapter::prepare`/`prepare_paths` gained a `device_pixel_ratio`
     parameter; `mkui-wgpu`'s `render/mod.rs` derives it from the physical
     surface config vs. the logical scene viewport. Added a regression test
     (`dilation_stays_half_a_physical_pixel_across_device_pixel_ratios`)
     proving the physical dilation is ~0.5px at ratios 1x/1.5x/2x/3x — this is
     exactly the integration gap Codex's *original* Sprint 8 review flagged
     at `app.rs`, now closed end-to-end rather than only inside the adapter's
     own self-test.
  2. **The real SFNT path never used a non-default `units_per_em`.** `place_slug_run`
     read the outline's units-per-em only for placement scale, not for
     encoding, so a 2048-upem face got an epsilon 2048x too small to have any
     effect (the cache's config was fixed at construction with the default
     `1.0`). Fixed: `SlugBlobCache::encode_with_units_per_em` overrides the
     epsilon per call (the cache's `SlugGlyphKey` already disambiguates by
     `font_id`, so this can't alias two fonts' blobs); `place_slug_run` now
     calls it with the outline's real units-per-em.
  3. **`SlugConfig` silently dropped `Eq`/`Hash`.** Adding a plain `f32` field
     broke the derive (contradicting the changelog's "additive" framing).
     Fixed: `units_per_em` is now stored as its bit pattern (`f32::to_bits`),
     so `SlugConfig` keeps deriving `Eq`/`Hash` — genuinely additive.
  Re-verified under the same pinned Docker + Lavapipe image as Phase 1 after
  all three fixes: all 24 Phase 1 comparisons still hold (Δ ≤ 1/255, SSIM =
  1.000000), plus the thin-gap regression scan on `o`/`g` at 3 DPIs (one
  false-positive caught during Phase 2 development and fixed before round 1:
  `g_2x` has a legitimate near-zero-alpha texel that's byte-identical to the
  reference, so the scan only flags texels absent from the reference's own
  golden). `cargo fmt --check`, workspace clippy (`-D warnings`, including
  `slug`/`gpu-tests,slug`/`atoms-on-wgpu`/`text` feature combinations), and
  `cargo test` for `mkui-vector2d`, `mkui-vector2d-wgpu`, `mkui-wgpu`
  (default, `slug`, `gpu-tests,slug` under Lavapipe: 148/148, +1 new
  dilation-ratio test) all pass. Dispatching Codex round 2 next.
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
