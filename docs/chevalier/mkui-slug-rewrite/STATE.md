# STATE — mkui Slug rendering chevalier mission

**Last updated:** 2026-07-30T09:15:00Z
**Current phase:** 2
**Phase status:** unblocked — operator ratified rubric amendment 1 (PR #162, merge sha `07926f0`, 2026-07-30). Rubric v1.2.1 adds a byte-identical-to-reference exception to § Phase 2 criterion (N) "No thin-gap regressions"; the ambiguity that produced [`BLOCKED.md`](https://github.com/mikbry/ui/blob/07926f041a29fd9adc4cf114890ecdd50960ae49/docs/chevalier/mkui-slug-rewrite/BLOCKED.md) is resolved and that file has been removed from this branch. Resuming the Codex review / dame dispatch loop.

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
- Codex verdicts: REQUEST_CHANGES (round 1, sha bdfdfbd); REQUEST_CHANGES
  (round 2, sha 8670955 — confirmed all 3 round-1 fixes correct, raised one
  new test-coverage finding); REQUEST_CHANGES (round 3, sha 5d6085a —
  confirmed round-2 coverage-gap fix correct; blocking finding: thin-gap
  threshold raised from rubric's literal 128 to 250 illegitimately redefines
  a frozen criterion instead of satisfying it)
- Dame verdicts: not yet dispatched
- Merged: pending — unblocked, resuming Codex/dame loop (see below)
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
  1.000000). `cargo fmt --check`, workspace clippy (`-D warnings`, including
  `slug`/`gpu-tests,slug`/`atoms-on-wgpu`/`text` feature combinations), and
  `cargo test` for `mkui-vector2d`, `mkui-vector2d-wgpu`, `mkui-wgpu`
  (default, `slug`, `gpu-tests,slug` under Lavapipe: 148/148, +1 new
  dilation-ratio test) all pass.

  Codex round 2 confirmed all three round-1 fixes correct end to end, but
  raised a real test-coverage gap: the thin-gap test rendered *pre-encoded*
  `.slug` fixtures, so it never exercised `build_bands`, `units_per_em`, or
  `place_slug_run` — removing the epsilon entirely would still leave it
  green. It also flagged that the thin-gap test's golden-relative exception
  (added during Phase 2 development to dodge a false positive at `g_2x`)
  technically deviates from the rubric's literal "zero such pixels required."
  Fixed: rather than keep the golden-relative carve-out, tightened the
  detection threshold itself — inspected `g_2x`'s actual neighbour alphas
  (206/248, a normal antialiasing gradient, not 250+/250+) and raised
  `NEAR_SOLID_ALPHA` from 128 (50%) to 250 (~98%), so the check is now a
  literal, unconditional "zero gap texels" assertion against mkui's own
  render, satisfying the rubric text directly. Added two CPU-level tests
  through the real encoder (not a fixture): `band_overlap_epsilon_normalizes_to_units_per_em`
  (a curve straddling a band boundary by `0.0005` em joins the band under the
  default epsilon, excluded under a tighter `units_per_em`) and
  `cache_encode_with_units_per_em_overrides_the_cache_default_per_call`
  (proves the same through `SlugBlobCache`'s per-call override — the exact
  path `place_slug_run` depends on). Re-verified: `mkui-vector2d` 81/81
  (+2 new), `gpu-tests,slug` 148/148 (thin-gap test now a direct assertion,
  no golden-diff), fmt/clippy clean. Dispatching Codex round 3 next.
  Round 3 confirmed the round-2 encoder/cache test fix was correct, but
  found the round-2 threshold change (128 → 250) illegitimately redefined
  a frozen rubric criterion rather than satisfying it. On inspection, this
  is not a fixable test-authoring mistake: dame-rubric.md's literal ≥50%
  thin-gap threshold and its Δ ≤ 4/255 reference-comparison criterion are
  jointly unsatisfiable at the ratified `g` fixture (2x DPI), texel
  (157,156) — the reference adapter's own output trips the literal
  threshold there (neighbours 206/248), and mkui reproduces it at Δ=0.
  Reverted the threshold to the rubric's literal `HALF_ALPHA = 128`, removed
  the golden-relative carve-out, and marked the test `#[ignore]` rather than
  ship a test that either fails a correct implementation or silently
  redefines the rubric. Posted `BLOCKED.md` with `blocked_reason:
  oracle_ambiguity` per CHARTER § Blocked signal condition #2 / dame-rubric.md
  § Amendment protocol ("impossible to satisfy"). Mission paused on this one
  criterion; all other Phase 2 work (dilation, units-per-em wiring, Eq/Hash,
  epsilon, 24/24 Phase 1 comparisons, encoder/cache tests) verified clean.
  Not dispatching dame or Codex round 4 until operator amends the rubric or
  unblocks.

  **Amendment 1 resolution (2026-07-30):** operator ratified option B (PR
  #162, merge sha `07926f0`) — rubric v1.2.1 excludes thin-gap texels that
  are byte-identical to the ratified reference-harness golden at the same
  coordinate from the "No thin-gap regressions" check; a hit only counts as
  a regression if it also diverges from the reference (Δ > 0) at that
  coordinate. Rebased `chevalier/mkui-slug-rewrite-phase2` onto latest main
  so `07926f0` is reachable from the PR head (clean rebase, no conflicts).
  Implemented the exception in
  `slug_reference_parity.rs::phase2_no_thin_gap_regressions_on_curve_heavy_glyphs`
  (new `texel_delta` helper computes per-coordinate Δ against the golden;
  the thin-gap scan only fails on hits with Δ > 0) and un-ignored the test.
  Re-verified under the same Docker + Mesa/Lavapipe stack as prior phases:
  `mkui-vector2d` 81/81, `mkui-vector2d-wgpu` 15/15, `mkui-wgpu` default
  129/129, `mkui-wgpu` `gpu-tests,slug` 148/148 (0 ignored — the previously
  `#[ignore]`d test now passes directly, confirmed empirically at `g_2x`
  texel (157,156): mkui and the reference are byte-identical there, so it is
  correctly excluded and contributes zero regressions). `cargo fmt --check`
  and workspace clippy (`-D warnings`, CI's `console-showcase`/
  `web-showcase`/`native-showcase` excludes) both clean. `BLOCKED.md`
  removed from the branch (block resolved). Resuming Codex review from
  round 3's clean state (only the thin-gap finding was blocking) before
  dame dispatch.
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
