# COMPLETION — mkui Slug rendering chevalier mission

**Posted:** 2026-07-30
**Mission:** mkui-slug-rewrite (`mikbry/ui#157`, CHARTER v1.2.1 ratified `07926f0`)
**All four phases implemented.** Phases 1-3 merged to `main`. Phase 4 (PR #164) is **OPEN**, awaiting the operator's visual smoke test — this document does not authorize its merge.

## The one thing to read before anything else: dame never independently applied the rubric, at any phase

CHARTER's acceptance criteria (§ below) ask for "dame BLESS" per phase. **No phase in this mission received one.** This is not a gap chevalier is hiding — it is the subject of `docs/chevalier/mkui-slug-rewrite/BLOCKED.md`'s `dame_infrastructure_gap` finding (posted mid-Phase-2, sha `645edf4`, now folded into `STATE.md`'s standing note since the block itself was resolved): `miky agent assign`'s round counter is keyed per-PR, not per-brief, so every dame dispatch after CHARTER's own step-4 Codex code-review is structurally round N > 1, which injects prior-round context and collapses dame's specified fresh, independent rubric application into a diff-scoped "review follow-up." Checking Phase 1's PR #160 review history after the fact showed the identical pattern in its own "dame BLESS" (`d99d1d7`) — so this was never phase-specific, and it was never something this mission actually had, not something Phase 2 broke.

The operator was presented with this finding and chose option 3 of `BLOCKED.md` (an explicit, recorded adjudication, not a chevalier judgment call): accept chevalier's self-check against the rubric + real CI green + a genuine Codex code-review cycle as the evidence basis for merging Phases 2-4, with dame's independent application deferred until `mikbry/miky#605` (a substrate fix tracked upstream) lands. That is the evidentiary basis for every "BLESS" cited below — not literal dame output, and each entry says so plainly rather than dressing an operator-adjudicated substitute up as the real thing.

## Acceptance criteria

- [x] **Phase 1 — PR #160 merged**, squash sha `7cb352c4b683f559a8123852599ce68b54369509`. Rubric § Phase 1 criteria (dual-ray weighted coverage, `fwidth` AA, 24/24 adapter-parity comparisons Δ ≤ 1/255, SSIM 1.0) verified via Codex round 1 REQUEST_CHANGES → round 2 APPROVE (a real defect found and fixed: the coverage clamp's placement diverged from the reference) and CI green (30/30 incl. `gpu-offscreen`). The recorded "dame BLESS" (`d99d1d7`) is, per the audit above, the same diff-scoped pattern as every later phase — not independent. See `STATE.md` § Phase 1.
- [x] **Phase 2 — PR #161 merged**, squash sha `47ac3a86de0cacec8613b6069b8fd03206623539`. Rubric § Phase 2 criteria (bounded half-physical-pixel dilation at 1x/1.5x/2x/3x DPR, band-overlap epsilon, 0 real thin-gap regressions post rubric-amendment-1, 24/24 parity) verified via chevalier's self-check, Codex round 4 APPROVE (rounds 1-3 found and fixed three real defects: dilation was logical- not physical-pixel; units-per-em never reached the real encode path; `SlugConfig` silently lost `Eq`/`Hash`), and CI green. Merged on **operator adjudication (`BLOCKED.md` option 3)**, not dame BLESS — the dame dispatch at `fdfc100` returned an APPROVE-shaped verdict that evaluated no rubric criterion (the finding that led to `BLOCKED.md`). See `STATE.md` § Phase 2.
- [x] **Phase 3 — PR #163 merged**, squash sha `506c4f86eebbab1e5ad55bb40e4f0973c2d94cff`. Rubric § Phase 3 criteria (baseline snap below cap-height threshold, piecewise-constancy, golden-image tests at 3 DPI) verified via chevalier's self-check, a genuine two-round Codex cycle (round 1 REQUEST_CHANGES on a real defect — the snap was baked into `place_slug_run` using a caller-supplied DPR that would go stale on a DPI change; round 2 APPROVE after the fix moved it into `mkui-vector2d-wgpu::pack`, using the frame's fresh DPR every render), and CI green. Merged per the same operator-adjudicated pattern as Phase 2 — no dame dispatch attempted (per the standing note, a second `--brief-file` round cannot apply the rubric fresh until `mikbry/miky#605` lands). See `STATE.md` § Phase 3.
- [x] **Phase 4 — PR #164, dame-BLESS-equivalent, PR remains OPEN.** Rubric § Phase 4 variant B criteria (integer-only bitmap scale, unconditional device-pixel position snap, sub-pixel-invariance) verified via chevalier's self-check, a genuine two-round Codex cycle (round 1 REQUEST_CHANGES on a real defect — the position snap was incorrectly gated on Phase 3's small-text threshold, leaving the demo's own 16px label unsnapped, contradicting Codex plan step 8's literal "snap every glyph to device pixels"; round 2 APPROVE after the gate was removed), and CI green (30/30 incl. `gpu-offscreen`). **Not merged — this is deliberate.** See § below.
- [x] **Phases 1-3 merged before this document; Phase 4 open + evidence-complete at this document.** Confirmed above.
- [x] **CHANGELOG.md `[Unreleased]` has an entry per phase** — all four (`### Added`), Phase 4's landing in the still-open PR #164 as the acceptance criterion anticipates.
- [x] **`cargo test --workspace` passes** at this commit (Phase 4 branch `chevalier/mkui-slug-rewrite-phase4`, head `852ce67ec99d677bf9ceacd1491fa2be4a9f795f`): `cargo test --workspace --exclude console-showcase --exclude web-showcase --exclude native-showcase --locked` → **423 passed, 0 failed, 6 ignored (43 suites)**. The three excludes mirror CI's own convention (each pins a different primary backend on `mkui`; building all three together trips the one-backend `compile_error!` guard, #102).
- [x] **`cargo fmt --check` + `cargo clippy` clean** at this commit: `cargo fmt --check --all` clean; workspace clippy (`--exclude console-showcase --exclude web-showcase --exclude native-showcase --all-targets --locked -- -D warnings`) clean; feature-slug matrix clippy (`mkui-vector2d-wgpu`, `mkui-wgpu --features slug`, `atoms-on-wgpu --features slug`, `text --features slug`, all `--all-targets`) clean.
- [x] **All existing `mkui-vector2d[-wgpu]` + `mkui-wgpu` tests still pass** — `mkui-vector2d` 81/81, `mkui-vector2d-wgpu` 20/20, `mkui-wgpu` default 132/132, `slug` 136/136, `gpu-tests,slug` 155/155 (1 ignored by design — the Phase 3 golden-capture regen tool), all under the pinned Docker + Lavapipe image (`rust:1.89-bookworm`, digest `948f9b08`).
- [x] **Golden-image regression tests exist + pass in `gpu-offscreen` at 1×/1.5×/2× DPI** — `crates/mkui-wgpu/tests/goldens/` (24 PNGs, 8 ratified glyphs × 3 DPIs, Phase 3), verified passing in the `gpu-offscreen` CI job on every merged phase and on PR #164.
- [x] **Reference adapter unchanged since ratification** — `git log --oneline 9f76af3..HEAD -- docs/chevalier/mkui-slug-rewrite/reference-harness/` is empty at this commit: zero chevalier commits to the adapter across all four phases.
- [x] **Mission-input docs unchanged since ratification, except the one operator-ratified amendment** — `git log --oneline b338bd9..HEAD -- docs/chevalier/mkui-slug-rewrite/{CHARTER,dame-rubric,codex-8-step-plan}.md` shows exactly one commit, `07926f0` (rubric amendment 1, operator-authored PR #162) — the amendment protocol's own contemplated case, not tampering.
- [x] **STATE.md updated per phase-boundary** — `docs/chevalier/mkui-slug-rewrite/STATE.md`, current through Phase 4.
- [x] **Every PR labeled `chevalier` + `mission:mkui-slug-rewrite`** — confirmed on #160, #161, #163, #164.
- [ ] **Operator visual smoke test — pending, this document is the trigger for it.** Chevalier cannot self-verify the perceptual dimension (CHARTER § Interaction points Point 2). Run on PR #164's branch (`chevalier/mkui-slug-rewrite-phase4`, head `852ce67`):
  - `cargo run -p text --features slug` — the Abel "Mag" title (Slug lane, Phases 1-3) beside the "Abel via Slug, label via bitmap" label (bitmap lane, Phase 4's integer-scale + device-pixel snap fix).
  - `cargo run -p atoms-on-wgpu --features slug`
  - `cargo run -p native-showcase`

  On confirmation the output looks visibly correct (crisper Slug text than pre-mission, no bitmap-label blur/asymmetry regression): **operator merges PR #164** — this is the mission's final merge, per the 2-touchpoint contract (CHARTER § Interaction points).
- [x] **This document, mapping every criterion to SHA + evidence.**

## What to actually look at during the smoke test

- The Slug title's antialiasing should look symmetric and crisp at both 1x and (if your display is HiDPI) 2x — that is Phase 1's dual-ray coverage + `fwidth` AA, and Phase 2's bounded dilation, working together.
- No thin gaps or unstable pixels on curved glyphs (`o`, `g`-shaped strokes if visible in the title) — Phase 2's band-overlap epsilon.
- The bitmap label ("Abel via Slug, label via bitmap", 16px) should look like clean, sharp 2×-scaled blocks — not blurred or asymmetrically duplicated — that's Phase 4's integer-scale fix; before this phase it would have rendered at a 1.6× fractional scale.
- Nothing about the demo's basic shape should look different from before this mission started — it is still two distinct, intentional text lanes side by side (Slug title, bitmap label), per ADR 0008's decision not to route the label through Slug.

## Mission artifacts

- CHARTER: `docs/chevalier/mkui-slug-rewrite/CHARTER.md` (ratified `07926f0`, v1.2.1)
- Rubric: `docs/chevalier/mkui-slug-rewrite/dame-rubric.md` (v1.2.1)
- Frozen execution plan: `docs/chevalier/mkui-slug-rewrite/codex-8-step-plan.md`
- Reference oracle: `docs/chevalier/mkui-slug-rewrite/reference-harness/` (ratified `9f76af3`)
- New architecture decision: `docs/architecture/0008-bitmap-vs-slug-labels.md`
- Heartbeat log: `docs/chevalier/mkui-slug-rewrite/STATE.md`
- Now-resolved block: `docs/chevalier/mkui-slug-rewrite/BLOCKED.md` (removed from the tree once resolved; its content and resolution are preserved in `STATE.md`'s standing note and git history at `645edf4`)

## Outstanding cross-mission follow-up (not this mission's to close)

- `mikbry/miky#605` — the round-counter/context-injection defect that made dame's fallback dispatch shape structurally unable to apply a fresh rubric on any round after a PR's first. Until it lands, any future chevalier+dame mission using this same fallback dispatch shape will hit the identical gap on its very first multi-round PR.
- `docs/chevalier/mkui-slug-rewrite/dame-preflight-report.md` (dame-rubric.md § "Dame's own preflight sensitivity test") was never created — the preflight never ran, for the same underlying reason. Recorded for the record; not separately actionable until `#605` lands and dame dispatch is real again.
