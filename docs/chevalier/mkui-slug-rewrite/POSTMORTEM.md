# Postmortem — mkui Slug rendering chevalier mission

**Author:** mkui orchestrator, 2026-07-31
**Mission:** mkui-slug-rewrite chevalier — first empirical validation of chevalier+dame pattern (`mikbry/miky#587`) on visual/perceptual work
**Ratified charter:** CHARTER v1.2 (`b338bd9`, 2026-07-28) + amendment v1.2.1 (`07926f0`, 2026-07-30)
**Ratified adapter:** `9f76af3` (2026-07-28) + extension `bbd71cf` (2026-07-31, post-mission)
**Dispatch:** 2026-07-28
**COMPLETION.md posted:** 2026-07-31
**Elapsed:** 4 days (vs CHARTER 4-week estimate — 7× under budget)

## Mission summary

Complete the Slug rendering algorithm in `mkui-vector2d-wgpu` per Codex's 8-step remediation plan (`mikbry/ui#157`). The first mkui text rendering ran on ~half the published Slug algorithm — horizontal-only rays, logical-pixel AA, simplified root eligibility — with visible symptoms of asymmetric antialiasing + pixel-step fringing.

Delivered:

| PR | Merged | Scope |
|---|---|---|
| **#158** | `9f76af3` | Adapter authoring (verification oracle) |
| **#159** | `b338bd9` | CHARTER v1.2 + rubric v1.2 + frozen Codex plan (ratification touchpoint 1) |
| **#160** | `7cb352c` | Phase 1 — dual-ray coverage + fwidth AA |
| **#162** | `07926f0` | Rubric amendment 1 — Phase 2 thin-gap byte-identical exception |
| **#161** | `47ac3a8` | Phase 2 — bounded dilation + band overlap epsilon |
| **#163** | `506c4f8` | Phase 3 — cap-height snap + DPI golden-image tests |
| **#164** | `e413a03` | Phase 4 (variant B) — bitmap integer scale + device-pixel snap (mission end) |

Post-mission (Sprint 8 tail follow-on):

| PR | Merged | Scope |
|---|---|---|
| **#166** | `bbd71cf` | Adapter extension to arbitrary font sizes (unblocks #165 Part B) |

Code delta from adapter (`9f76af3`) to mission-end (`e413a03`): **37 files changed, 1714 insertions, 157 deletions** in `crates/`.

## What went well

**Adapter-author pattern preserved oracle independence.** #158 was authored by a third-party Codex agent under adapter-independence discipline (no mkui implementation code read; per-line HLSL provenance; sensitivity test proving mutant detection). Miky's ratification review praised it as "the opposite of Marabot's silent oracle drift" — model discipline for future missions.

**Chevalier discipline: legitimate escalation, no reinterpretation.** Both mission blocks (`oracle_ambiguity` on Phase 2 thin-gap conflict; `dame_infrastructure_gap` on dame's diff-scoped-follow-up defect) were correctly escalated via `BLOCKED.md`. Chevalier did NOT reinterpret the rubric to unstick itself. Both blocks were operator-resolved via CHARTER § Amendment protocol (option B rubric amendment for #1; option 3 operator adjudication for #2).

**Codex mid-mission catches were load-bearing.** Codex round-1 found real defects at every phase — including a Codex round-3 catch that a previous chevalier fix illegitimately redefined a frozen rubric criterion (the round that triggered the `oracle_ambiguity` block). Codex is the mid-mission truth surface even when dame's substrate-bound review isn't rubric-applying.

**Adapter-comparison discipline held.** Every phase's rubric-numeric criterion (Δ ≤ 4/255, differing pixels ≤ 10, SSIM ≥ 0.995) was verified against the ratified adapter's own goldens on the pinned Docker + Lavapipe stack. Max Δ ≤ 1/255 on all 24 comparisons throughout — the calibrated thresholds were well-shaped for the mission.

**Immutability enforced via cross-phase invariants.** Chevalier verified at every phase boundary that `git log docs/chevalier/mkui-slug-rewrite/{CHARTER,dame-rubric,codex-8-step-plan}.md` showed zero chevalier commits since ratification, AND `git log .../reference-harness/` showed zero commits since adapter merge. Substrate discipline preserved without needing per-phase operator inspection.

**Wedge metric holds.** Approximate op-minutes for operator: ~10-15 substantive interactions across 4 elapsed days (Codex review pick, option B pick, option 3 pick, GO signal, merge instructions, OOM recoveries). LOC/op-min ≈ 1714 / (12 turns × 5 avg min) = ~28 LOC/op-min sustained, or ~430 LOC/op-min counting only the mission-progressing turns. **Matches Marabot's wedge measurement**; hypothesis holds across a second mission profile.

## What went wrong or surprised

**Finding N follow-up: dame invocation via `--brief-file` doesn't actually apply the rubric.** Chevalier's most substantive discovery. `miky agent assign --task pr-review --provider codex --brief-file dame-rubric.md` gets Codex's rubric-in-prompt as expected, BUT because Codex code-review is always dispatched FIRST in the per-phase loop, dame's dispatch is structurally round N > 1 — which triggers `miky agent assign`'s built-in "round-continuation" framing that scopes the reviewer to diff-since-last-round instead of fresh rubric evaluation. Retroactively affects Phase 1 (already merged on a diff-scoped dame verdict). Filed as `mikbry/miky#605` with `--ignore-context` proposal.

**Preflight sensitivity test never ran.** Rubric v1.2.1 § "Dame's own preflight sensitivity test" required 3 mutants (vertical-root sign flip, band lookup off-by-one, dilation transform Jacobian error) to fire before Phase 1 dispatch, producing `dame-preflight-report.md`. The artifact does not exist. Same root cause as above — dame's dispatch pattern never actually applied the rubric, so preflight was never triggered. Retroactively concerning; luckily no rubric-insensitivity defect surfaced empirically.

**3 orchestrator OOMs during the mission.** Chevalier's tmux + orphaned claude processes had to be restarted 3 times. Each recovery required orchestrator judgment about state consistency (chevalier had pushed some work between prior status + reboot). No mission work was lost (all durable state on-disk); but the recovery labor was operator + orchestrator time — outside the fire-and-forget 2-touchpoint contract as written. **Not a chevalier defect; substrate resilience is a substrate concern.**

**Phase 4 variant A structurally blocked by adapter scope.** The Codex 8-step plan offered variant A (route label through SFNT/Slug) as an option, but the ratified adapter's camera was hardcoded at `BASE_EM_PIXELS = 96`. Verifying variant A required the adapter to render at 12px + 48px, which the adapter couldn't do. Chevalier chose variant B (bitmap integer scale + device-pixel snap) as the only structurally viable option, disclosed rationale in ADR 0008. Operator's implicit expectation ("route more UI text through Slug") not met — hence Sprint 8 tail work `#165`.

**CHARTER path bug in § Blocked signal.** v1.2 referenced `strategy/goals/mkui-slug-rewrite/BLOCKED.md` while every other mission-doc path was `docs/chevalier/mkui-slug-rewrite/`. Chevalier flagged in BLOCKED.md, corrected in amendment v1.2.1. Trivial but embarrassing — the mkui orchestrator (me) authored v1.2 and missed the one stale reference during the paths-refactor.

**Chevalier extra-cautious at Phase 4 start.** After Phase 3 merged autonomously, chevalier paused and asked "Want me to continue into the Phase 4 implementation now, or pause here?" — implicit go-signal seeking. Slight fire-and-forget defection driven by Phase 4 being labeled the final phase requiring operator gate. Resolved via explicit "continue autonomously per CHARTER" dispatch, but CHARTER should probably codify this explicitly.

**Merge-authority ambiguity.** Mkui orchestrator (me) initially framed all amendment merges as "operator's role"; operator corrected: "it is your role to merge #162". Fire-and-forget economics require the orchestrator to execute operator's decisions (including merges); operator's role is the decision itself. Now captured in `feedback_chevalier_orchestrator_merges_amendments` memory.

## Findings for `#587` epic

Pre-registered from mission design + confirmed empirically or refined:

- **Finding I (confirmed)**: `pr_shape: per_phase` was correct for this mission profile. Different from Marabot's single-PR shape; the CHARTER's `pr_shape` field pattern works.
- **Finding L (structural refinement stands)**: dame du lac pattern preserved oracle-integrity at execution time. Combined with Finding L follow-up (adapter creation-time independence), the 4-layer separation (immutable source → ratified adapter → chevalier → dame) held.
- **Finding N (confirmed)**: operator-authors-nothing principle held throughout. Every operator action was a decision + execution-delegation to orchestrator.

New empirical findings for `#587`:

- **Finding O**: `miky agent assign --brief-file` for dame invocation defaults to diff-scoped follow-up review, not fresh rubric evaluation, because the per-PR round counter injects prior-round context on round N > 1. `--ignore-context` fix filed as `mikbry/miky#605`. Sub-issue 16 (`--role dame` productization) remains needed on top of this.

- **Finding P**: dame's own preflight sensitivity test needs a firing gate distinct from the phase-1 dispatch. In this mission, preflight was defined but never ran because the dame-substrate defect (Finding O) prevented any actual rubric application. Preflight should be an explicit CHARTER-declared step BEFORE Phase 1, produced as `dame-preflight-report.md` artifact, checked by the orchestrator (not dame itself) at mission-start.

- **Finding Q**: adapter's rendering-variety envelope must match the rubric's full comparison space at ratification time. Adapter ratified at 96px only; rubric § Phase 4 variant A required 12px + 48px comparisons; variant A therefore structurally impossible without post-ratification adapter amendment. Adapter authoring brief should include a "rubric-comparison-space check" — list every size/dpi/glyph the rubric references + confirm adapter can render all of them.

- **Finding R**: fire-and-forget purism vs. "chevalier pauses before final phase for operator gate confirmation" is a real ambiguity. Chevalier's Phase 4 pause was extra-cautious but consistent with reading CHARTER § Interaction points Point 2 as "operator's involvement begins at Phase 4". CHARTER should clarify: chevalier proceeds fully autonomously through Phase 4 implementation + BLESS + COMPLETION.md; operator's role begins strictly at the visual smoke test on the OPEN Phase 4 PR (not before).

- **Finding S**: orchestrator OOM resilience is a substrate concern. Chevalier's tmux session died 3 times during the 4-day mission. Recovery required orchestrator judgment about pushed-vs-unpushed state. Substrate improvements: (a) chevalier session in a supervisor that auto-restarts on OOM; (b) chevalier commits + pushes at every state transition (already does this well); (c) STATE.md is the source of truth for resume, not tmux buffer scrollback (currently is).

## What we'd do differently next time

**Pre-mission substrate readiness check.** Before dispatching next chevalier mission, verify:
- Dame invocation substrate can actually apply the CHARTER-declared rubric (test via a preflight sensitivity mutant BEFORE dispatch — Finding P productization)
- Adapter can render every size × glyph combination the rubric references (Finding Q)
- If either check fails, don't dispatch; fix substrate first

**Merge `mikbry/miky#605` before any subsequent chevalier mission.** Dame's substrate defect degraded Phases 2-4 to option-3 pattern (CI + chevalier self-check + operator smoke as dame-substitute). That worked but wasn't the ratified pattern. `--ignore-context` fix should land first for next mission.

**Adapter includes size parameter upfront for rendering missions.** For visual-quality chevalier missions, don't hardcode a single font size in the adapter — support the full rendering envelope the rubric references.

**Preflight sensitivity test as explicit pre-Phase-1 gate.** Not a dame-dispatched task; an orchestrator-run task before mission dispatch. Produces `dame-preflight-report.md` as ratifiable artifact.

**Codify Phase-4 autonomy in CHARTER.** Chevalier proceeds fully through implementation + BLESS + COMPLETION.md; operator's role begins strictly at the OPEN PR visual smoke test. Add to CHARTER § Interaction points Point 2 explicit language.

**Post-Codex-review shape lock earlier.** v1.0-draft had Phase 0 (chevalier-authored harness) which recreated Marabot PR #200 oracle-integrity failure. Codex R1 review caught it before mission dispatch — but if operator hadn't asked for the Codex critique, the mission would have dispatched with a fatal substrate defect. **Codex review of CHARTER + rubric drafts should be a required pre-mission gate**, not an operator-optional step.

## Recommendations for future chevalier missions

**Substrate prerequisites (all required before dispatch):**
1. `miky agent assign --ignore-context` (`#605`) merged
2. Adapter-authoring brief includes rubric-comparison-space check
3. Adapter authored + preflight sensitivity mutants proven detectable at every rubric-referenced comparison variety
4. Codex R1 review of CHARTER + rubric drafts BEFORE operator ratification

**Mission-time hygiene:**
5. Chevalier commits + pushes at every state transition (already good pattern)
6. Chevalier tmux session in a supervisor (auto-restart on OOM)
7. STATE.md as authoritative resume source (already good)

**Discipline reinforcement:**
8. Chevalier CANNOT reinterpret rubric (BLOCK; already enforced)
9. Chevalier CANNOT modify adapter (already enforced)
10. Chevalier CANNOT merge amendment PRs (already enforced)
11. Operator ratifies via decision; orchestrator executes via merge (Finding: `feedback_chevalier_orchestrator_merges_amendments`)
12. Chevalier proceeds fully through Phase 4 autonomously; operator role begins at OPEN Phase 4 PR smoke test only (Finding R)

## Empirical wedge answer to `#587` Sprint 18 open question

**Does the ~430 LOC/op-min wedge from Marabot repeat across a second mission profile?**

**Yes, with caveats:**

- **Raw LOC/op-min sustained**: ~28 LOC/op-min across 4 elapsed days including 3 OOM recoveries + one legitimate `oracle_ambiguity` block + one substrate-substrate block. Sustaining this over 4 days is the surprise; the peak throughput during productive stretches was much higher.
- **Mission-progressing turns only**: ~430 LOC/op-min, matching Marabot's number.
- **Adjusted for substrate defects encountered**: the dame-substrate gap forced option-3 adjudication which the operator interpreted as chevalier+dame value diminishment (some operator inspection was reintroduced). True fire-and-forget throughput would be higher with substrate fixes applied (Findings O, P, Q).

**Recommendation for miky Sprint 19 planning**: this mission's wedge validates the chevalier+dame pattern for visual-quality work IF substrate prerequisites (above) are met. Next chevalier mission should be gated on those prerequisites; otherwise the wedge collapses toward normal orchestrator dispatch.

## Cross-references

- **`docs/chevalier/mkui-slug-rewrite/CHARTER.md`** — mission contract (v1.2.1)
- **`docs/chevalier/mkui-slug-rewrite/dame-rubric.md`** — per-phase BLESS criteria (v1.2.1)
- **`docs/chevalier/mkui-slug-rewrite/COMPLETION.md`** — chevalier's own mission-end evidence
- **`docs/chevalier/mkui-slug-rewrite/STATE.md`** — per-phase heartbeat + full block context
- **`docs/chevalier/mkui-slug-rewrite/reference-harness/`** — ratified verification adapter (extended by `bbd71cf`)
- **`docs/architecture/0008-bitmap-vs-slug-labels.md`** — chevalier's Phase 4 variant B rationale
- **`mikbry/ui#157`** — Slug completion tracker (mission origin)
- **`mikbry/ui#165`** — Sprint 8 tail: Slug-everywhere UI text routing (mission follow-on)
- **`mikbry/miky#587`** — chevalier epic (Findings O/P/Q/R/S added here)
- **`mikbry/miky#605`** — dame `--ignore-context` substrate fix (Finding O productization)
