# BLOCKED — Phase 2, dame invocation does not perform dame's rubric-bound review

**Posted:** 2026-07-30
**Phase:** 2 (PR #161, branch `chevalier/mkui-slug-rewrite-phase2`, head sha `fdfc10000bd6fbeef2114e372aa63a5fad3e69b7` at time of posting)
**blocked_reason:** `dame_infrastructure_gap` (CHARTER § Blocked signal condition #6)

## Summary

The dame invocation defined in CHARTER § `verification_oracle.dame_invocation.command`
— `miky agent assign --task pr-review --provider codex --pr <PR> --brief-file
docs/chevalier/mkui-slug-rewrite/dame-rubric.md ...` — returned an
`APPROVE`-shaped verdict for PR #161 at `fdfc100`, but the underlying review
did not evaluate dame-rubric.md's Phase 2 criteria. It performed a
diff-scoped code-review follow-up instead of dame's specified rubric-bound
verification (render glyphs, measure dilation in NDC, independently rerun
the 24-fixture adapter comparison, pixel-scan the thin-gap check, verify the
band-overlap epsilon arithmetic, check cross-phase invariants, run the
preflight sensitivity test). This is more dangerous than an infrastructure
failure that produces no verdict, because it defaults to BLESS while
silently skipping the checks BLESS is supposed to certify. Per CHARTER's
own framing (§ Reference harness authoring: "chevalier controlling both
implementation and reference recreates Marabot PR #200's oracle-integrity
failure"), treating this verdict as a real BLESS would reintroduce
exactly that failure mode one level up — dame not actually applying the
oracle it's bound to.

## How this was found

Dispatched via the documented command, `--expect-sha fdfc10000bd6...`,
review id `ae-01785402938253164000-00000000`. The verdict file
(`~/.miky/projects/mikbry__ui/agent-events/codex-review-pr-161-ae-01785402938253164000-00000000.md`)
reads only:

> The PR has advanced by one commit, from `0c553c0` to `fdfc100`. That
> commit is narrowly scoped... I'm now validating the exact-head
> review/CI state and the two edited references.
>
> No findings at head `fdfc100`... The only post-approval changes are that
> citation correction and audit-trail documentation; rendering and test
> behavior are unchanged.

No mention of dilation width, band-overlap epsilon, the thin-gap scan, SSIM,
per-glyph Δ, cross-phase invariants, or the preflight sensitivity test
anywhere in the verdict or its full live transcript
(`codex-review-live-ae-01785402938253164000-00000000.out`).

Inspecting the dispatch's own recorded spec
(`codex-review-spec-ae-01785402938253164000-00000000.json`) shows why: the
prompt actually sent to the reviewing agent begins

> `"ROUND-5 CONTINUATION of the multi-round review for PR #161. Prior round
> verdict (from the audit trail): ... Focus this round on whether the prior
> round's findings were addressed."`

— followed by the full dame-rubric.md text. So the rubric *was* present in
context, but the round-continuation framing ("round N > 1 injects the
prior-round verdict as brief context", per `miky agent assign --help`)
instructs the agent to scope itself to the diff since the last round rather
than perform a fresh, independent rubric evaluation. The agent's own first
message confirms it followed that framing: "I'll keep this round scoped to
regressions or unresolved prior findings" — then routed to GitHub's
"review follow-up" skill workflow rather than doing any rendering or
measurement.

**Root cause, precisely stated:** `miky agent assign`'s round counter is
keyed per-PR only (confirmed via `~/.miky/projects/mikbry__ui/reviews.jsonl`
— no field distinguishes `--brief-file dame-rubric.md` dispatches from
plain Codex `pr-review` dispatches; no `--fresh` / `--no-prior-context` /
`--thread` flag exists per `miky agent assign --help`). Because CHARTER's
own per-phase loop always dispatches Codex code-review (step 4) before dame
(step 6) on the same PR, dame's dispatch is *structurally always* round
N > 1 for every phase in this mission — this is not a Phase-2-specific
edge case.

## Corroborating: Phase 1 (already merged) has the same defect

Checking the three PR #160 review rounds for comparison
(`codex-review-pr-160-ae-0178523*.md`):

- Round 1 (`345dbb6`, REQUEST_CHANGES) *did* apply the rubric — it explicitly
  cites "the ratified acceptance rubric rather than relying on the PR's
  self-reported results" and blocks on the `[-0.001, 1.001]` pre-clamp
  range criterion. This is the one round in the mission's history where a
  rubric criterion was actually the basis for a verdict.
- Round 2 (`af4d21f`, APPROVE) and round 3 (`d99d1d7`, APPROVE — the one
  STATE.md records as "Dame verdicts: BLESS (round 1, sha d99d1d7)") both
  show the identical "review follow-up" / diff-scoped pattern as this
  Phase 2 dispatch. Round 3's own text: "I'll focus strictly on rubric-
  addressing regressions or unresolved findings" — narrower than a rubric
  evaluation, and it explicitly frames itself as re-confirming round 1's
  finding, not applying the rubric fresh.

So Phase 1 merged on a dame verdict with the same structural gap. This
BLOCKED.md does not attempt to un-merge Phase 1 or rewrite its STATE.md
history — flagging it here for the operator's awareness, since it affects
how much confidence the mission's prior BLESS carries.

Also corroborating: `docs/chevalier/mkui-slug-rewrite/dame-preflight-report.md`
— the artifact dame-rubric.md § "Dame's own preflight sensitivity test"
says must exist before Phase 1 dispatch (3 mutants: vertical-root sign
flip, band-lookup off-by-one, dilation-transform Jacobian error) — does not
exist anywhere in this repository's history. The preflight never ran.

## What is NOT in question

This is not a finding about mkui's Slug implementation. Several rubric
`(N)` criteria ARE empirically exercised — just not independently by
dame — via the green `gpu-offscreen` CI job on the exact PR #161 head
(`fdfc100`), which runs `phase1_matches_reference_adapter_within_rubric_thresholds`
and the now-active `phase2_no_thin_gap_regressions_on_curve_heavy_glyphs`
under the pinned Docker + Lavapipe stack. Chevalier's own pre-dispatch
self-check (this session) covers the full Phase 2 rubric list:

- **(S) Bounded dilation replaces constant 1.5-logical-px expansion** —
  `half_pixel_dilation_units` in `crates/mkui-vector2d-wgpu/src/lib.rs`;
  no remaining `1.5`-constant dilation.
- **(S) Dilation is 2D-bounded, not transform-aware** — no MVP/viewport
  terms in the dilation path (grepped).
- **(N) Dilation width ≈ 0.5 × physical_pixel_size** — regression test
  `dilation_stays_half_a_physical_pixel_across_device_pixel_ratios` proves
  this at 1x/1.5x/2x/3x (not dame's own NDC measurement, but the same
  claim, tested).
- **(S) Band overlap epsilon present** in `crates/mkui-vector2d/src/slug.rs`
  `build_bands`.
- **(N) Band overlap epsilon = 1/1024 em** — `units_per_em / 1024.0`,
  tested by `band_overlap_epsilon_normalizes_to_units_per_em`.
- **(S) Bands normalized to em space / `units_per_em` provided** —
  `SlugBlobCache::encode_with_units_per_em`, called from `place_slug_run`.
- **(N) Adapter-comparison per-channel Δ ≤ 4/255** — 24/24 comparisons,
  max Δ ≤ 1/255, under Lavapipe (this session's run).
- **(N) No thin-gap regressions** — amended criterion implemented
  (`texel_delta` byte-identical exception); 0 real regressions found on
  `o`/`g` at 3 DPIs under Lavapipe.
- **(T) `cargo test -p mkui-vector2d` / `-p mkui-vector2d-wgpu` /
  `-p mkui-wgpu` (`gpu-tests,slug`)** — 81/81, 15/15, 148/148 (0 ignored).
- **(N) Phase 1 SSIM criteria still hold** — SSIM = 1.000000 on all 24,
  floor 0.995.
- **Cross-phase invariants**: `cargo fmt --check` clean; workspace clippy
  (CI excludes) clean; CHANGELOG `[Unreleased]` entry present and updated;
  `git diff --name-only 7cb352c..HEAD` scoped to
  `crates/mkui-vector2d{,-wgpu}/`, `crates/mkui-wgpu/`, `CHANGELOG.md`,
  `docs/chevalier/mkui-slug-rewrite/STATE.md`, plus amendment commit
  `07926f0` (operator-authored, carried in by the required rebase — not a
  chevalier edit to CHARTER/dame-rubric); reference-harness unchanged
  since `9f76af3`; PR carries both `chevalier` + `mission:mkui-slug-rewrite`
  labels; exact-head CI green (30/30 checks) including `gpu-offscreen` with
  VVL active.

What's missing is specifically dame's *independent* application of the
rubric (its own rendering/measurement, its own adapter regeneration, the
preflight sensitivity proof) — not all empirical verification of Phase 2.

## Why this is escalated rather than resolved autonomously

Per CHARTER § YOLO scope, chevalier cannot touch `mikbry/miky` (a different
repo) to fix the round-counter/brief-file scoping defect, and per §
Immutability chevalier cannot reinterpret what counts as a valid dame BLESS.
Recording this session's APPROVE-shaped verdict as "Dame: BLESS" in
STATE.md and merging PR #161 on it would put a false oracle signal into the
mission's own audit trail. This is squarely CHARTER § Blocked signal
condition #6.

## Candidate operator fixes (unranked)

1. Add a flag to `miky agent assign` that starts a fresh review thread
   (ignores prior-round context) when `--brief-file` differs from the
   prior round's, or when a `--role dame` discriminator (already tracked
   as `#587` sub-issue 16) is passed — the typed fix CHARTER's fallback
   shape was always meant to be temporary for.
2. Manually verify Phase 2 against dame-rubric.md (operator or a fresh,
   context-free reviewer) using chevalier's self-check above as the
   starting evidence, and explicitly ratify that as this phase's BLESS.
3. Accept the empirical CI-level verification (Lavapipe-run parity +
   thin-gap tests, all green on the exact head) as sufficient for Phase 2
   specifically, with an explicit operator note that dame's independent
   application is deferred until the substrate fix lands — and retroactively
   note the same for Phase 1.

## Next steps

Mission pauses on Phase 2 at the dame-dispatch step. PR #161 remains open,
unmerged, head `fdfc100`. Codex round 4 (real code review, not
round-continuation-scoped in the same way — round 1 of *this* phase's
Codex thread, not round N of a mixed thread) APPROVE stands; that is a
valid signal. On operator direction (fix the substrate, manually
adjudicate, or accept the empirical evidence), chevalier resumes at
whichever step the operator's choice implies.
