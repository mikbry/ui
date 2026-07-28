# CHARTER — mkui Slug rendering completion (chevalier + dame du lac)

**Version:** v1.2.1 (2026-07-28, amendment 1) — awaits operator ratification. Amends v1.2 (§ Blocked signal path corrected from `strategy/goals/...` to `docs/chevalier/...`). Supersedes v1.1 pre-Codex-review draft at `strategy/goals/mkui-slug-rewrite/CHARTER.md`.
**Filed under:** `mikbry/miky#587` chevalier epic — SECOND mission (empirical Sprint 18 data point)
**Started:** _pending launch approval — mission dispatch touchpoint 1 = operator merge of this PR_
**Sprint:** 18 Lane 2 (dispatch); COMPLETION expected Sprint 19-20
**Chevalier provider:** Claude (frontier tier) — per Marabot precedent
**Dame provider:** Codex (cross-provider by convention — adversarial verification)
**Chevalier discipline:** real chevalier per `#587` §3 rules 1-5 — fire-and-forget, done/blocked signal only, no check-ins beyond ratification + COMPLETION

**Ratified verification adapter:** [`docs/chevalier/mkui-slug-rewrite/reference-harness/`](reference-harness/) — merged as PR #158 at commit `9f76af3` on 2026-07-28. This adapter is the immutable independent oracle dame consumes. Authored by a third-party (adapter-author role, not chevalier or dame), per `#587` Finding L follow-up + Finding N.

## Mission

Complete the Slug rendering algorithm in `crates/mkui-vector2d-wgpu/` per Codex's 8-step remediation plan (`mikbry/ui#157` body). The current implementation is a Slug-inspired prototype implementing ~half the published algorithm — horizontal-only rays, logical-pixel AA, simplified root eligibility. Visible symptom: text on `cargo run -p text --features slug` looks bitmap-y.

**Reference pattern:** Marabot MCP server rewrite (`strategy/archive/marabot/rust-rewrite/CHARTER.md`) as chevalier-shape reference. Slug adds the dame du lac layer per `#587` Finding L (portable pattern: `chevalier_dame_pattern` memory).

## Scope

**Target artifacts:**

- **Primary implementation crates**:
  - `crates/mkui-vector2d/src/slug.rs` — CPU band builder (dual-axis bands, em-space overlap)
  - `crates/mkui-vector2d-wgpu/src/lib.rs` — GPU band upload (vertical bands + curve indices)
  - `crates/mkui-vector2d-wgpu/src/slug.wgsl` — fragment shader (dual-ray coverage, fwidth AA, root eligibility)
  - `crates/mkui-wgpu/src/app.rs` — baseline snap for small text
  - `crates/mkui-wgpu/src/render/mod.rs` — AA + dilation integration
  - `crates/mkui-wgpu/src/slug_text.rs` — Slug color path (bridging with #155 gamma-linear pipeline)

- **Visual smoke targets (chevalier does not view; operator drives at COMPLETION)**:
  - `examples/text` (with `--features slug`)
  - `examples/atoms-on-wgpu` (with `--features slug`)
  - `examples/native-showcase`

- **Regression tests (chevalier authors)**:
  - `crates/mkui-wgpu/tests/goldens/` — golden-image regression tests at 1×/1.5×/2× DPI
  - Unit tests colocated with modified modules

**In-scope for the chevalier:**

- Full implementation of Codex's 8 steps (frozen copy at [`codex-8-step-plan.md`](codex-8-step-plan.md) — the `execution_oracle`), delivered as 4 per-phase PRs (see § Phase structure below)
- Dame du lac review at every phase boundary against the ratified adapter at `docs/chevalier/mkui-slug-rewrite/reference-harness/`
- Codex mid-mission code-review per PR (existing per-PR loop, embedded in chevalier's flow)
- CHANGELOG.md entries per merged PR
- `STATE.md` at `docs/chevalier/mkui-slug-rewrite/STATE.md` per phase-boundary heartbeat (see § Heartbeat protocol)
- `COMPLETION.md` at `docs/chevalier/mkui-slug-rewrite/COMPLETION.md` with per-criterion evidence at mission end

**Explicitly OUT of chevalier scope:**

- **Reference harness authoring or modification** — the adapter at `docs/chevalier/mkui-slug-rewrite/reference-harness/` is IMMUTABLE. Chevalier may READ it (understand the algorithm the mkui implementation must match) but MUST NOT modify. Per `#587` Finding L follow-up (Codex review 2026-07-27): chevalier controlling both implementation and reference recreates Marabot PR #200's oracle-integrity failure. This constraint is load-bearing.

**Out-of-scope (STRICT):**

- Any change to `mkui-vector2d`'s public API (Sprint 8 Wave 1 landed the substrate; the Result-based `subdivide_cubic` API and `SlugGlyph`/`encode_slug_glyph` signatures are frozen)
- Any change to the linear-color-space contract from #155 (linear intermediate + present pass)
- Any change to the wgpu render pass architecture (multisample intermediate, resolve, present) beyond what Codex step 4 (dynamic dilation) specifically requires
- Any Sprint 8 Wave 2 work (consumer trio: button/card/dot demo — separate scope)
- Any refactor of adjacent code (`mkui-core`, `mkui-runtime`, other backends)

## Non-goals (explicit)

- **Not a rewrite of the substrate** — the substrate landed via #148 + #150 + #155; this mission consumes those APIs.
- **Not an MSAA re-add** — established as band-aid via closed #156. Codex step 4's dynamic dilation is Slug-specific, not MSAA.
- **Not a distance-field text (SDF) alternative** — orthogonal, later sprint if ever.
- **Not a general rendering quality push** — the mission is specifically Slug completeness per Codex's plan. Other rendering polish is out of scope.

## Two oracles (both immutable during mission)

### `execution_oracle` — Codex 8-step plan (frozen)

- **Codex 8-step plan**: [`codex-8-step-plan.md`](codex-8-step-plan.md) — frozen copy of the normative plan at ratification time. Chevalier operates against this file, NOT the mutable `mikbry/ui#157` body.
- **Companion algorithm reference** (for cross-check when the Codex plan is silent on implementation details): JCGT paper (Lengyel 2017): https://jcgt.org/published/0006/02/02/. Chevalier reads for correctness cross-reference only; the plan is the primary spec.
- **Storage format note per miky #158 N4**: the ratified adapter uses `RGBA32Float` / `RGBA32Uint` textures (avoiding half-float quantization at the oracle boundary). This differs from the upstream README's compact 16-bit recommendation. Chevalier's mkui implementation MAY use either storage mode — the rubric handles both (see dame-rubric.md § N1 storage-format handling). Do NOT default to upstream README's 16-bit format thinking it is authoritative for this mission; the deviation is deliberate at the oracle side.
- **Existing mkui code + tests**: `crates/mkui-vector2d*/tests/` — chevalier's own tests (authored + inherited) are the primary programmatic acceptance layer.

### `verification_oracle` — dame du lac review protocol

The verification oracle IS the ratified adapter at [`docs/chevalier/mkui-slug-rewrite/reference-harness/`](reference-harness/) — merged as PR #158 at commit `9f76af3` on 2026-07-28. Dame consumes the adapter's outputs (24 known-good PNGs, sensitivity mutant, per-line HLSL provenance). Dame does NOT re-derive the algorithm.

**Dispatch shape** (uses existing miky substrate — `--role dame` typed vocabulary is `#587` §7 sub-issues 16-20, Sprint 19+ productization; first-mission uses fallback):

```yaml
# Chevalier dispatches dame at each phase boundary via existing substrate:
dame_invocation:
  provider: codex                           # cross-provider (chevalier = Claude)
  # Fallback shape until #587 sub-issue 16 lands (typed --role dame):
  command: |
    miky agent assign \
      --task pr-review \
      --provider codex \
      --pr <own PR number> \
      --expect-sha <own PR head SHA> \
      --repo mikbry/ui \
      --brief-file docs/chevalier/mkui-slug-rewrite/dame-rubric.md \
      --headless
  verdict_mapping:
    APPROVE: BLESS                          # dame greenlights merge (Phases 1-3) OR advances to operator gate (Phase 4)
    REQUEST_CHANGES: REFORGE                # chevalier iterates within phase
  polling: |
    # Async — returns review id at dispatch; chevalier polls until verdict lands:
    miky agent log --review <review id>
    # Verdict file at:
    ~/.miky/projects/mikbry__ui/agent-events/codex-review-pr-<N>-<review id>.md
  authority: final                          # dame verdict binding; override only via BLOCKED.md dame_verdict_dispute
```

**Dame's oracle artifacts** (read-only; dame cannot modify):

- [`codex-8-step-plan.md`](codex-8-step-plan.md) — the frozen implementation plan (execution_oracle)
- [`dame-rubric.md`](dame-rubric.md) — the per-phase BLESS/REFORGE criteria (the specification for dame's judgment)
- [`reference-harness/`](reference-harness/) — the ratified independent adapter (Slug HLSL→WGSL port, pinned upstream `EricLengyel/Slug@be3c13e`, 24 known-good goldens + seeded-mutant sensitivity proof)
- JCGT paper (Lengyel 2017): https://jcgt.org/published/0006/02/02/ — algorithm cross-reference

Dame is bound to the rubric — cannot re-interpret, cannot add criteria, cannot judge outside the rubric's explicit scope. If a criterion is ambiguous, dame returns REFORGE with `blocked_reason: oracle_ambiguity`.

Chevalier ALSO reads the rubric — informs self-verification before dame invocation.

Neither chevalier nor dame may amend the rubric or the CHARTER. Amendments require operator ratification per § Amendment protocol.

## Phase structure

`pr_shape: per_phase` (per `#587` Finding I — chevalier's per-mission PR shape is CHARTER-declared, differing from Marabot's single-PR shape).

Each phase = one PR = one Codex code review + one dame BLESS/REFORGE cycle before merging + advancing.

**Phase 0 is REMOVED from v1.1** — reference adapter is pre-ratified via PR #158 (`9f76af3`, 2026-07-28). Mission starts directly at Phase 1.

- **Phase 1**: Codex 8-step-plan steps 1+2+3 — dual-ray coverage + fwidth AA. Estimated 3-6 dame REFORGE iterations (highest-complexity phase; foundational).
- **Phase 2**: Codex 8-step-plan steps 4+5 — dilation + band epsilon. Estimated 1-3 iterations. **Scope note per Codex R1 review**: this phase implements the bounded half-physical-pixel 2D dilation from `#157`'s plan, NOT full transform-aware dynamic dilation from the retrospective. Transform-aware dynamic dilation is deliberately out-of-scope for this mission (mkui doesn't ship perspective/transform text yet).
- **Phase 3**: Codex 8-step-plan steps 6+7 — cap-height snap + DPI golden-image tests. Estimated 1-3 iterations.
- **Phase 4**: Codex 8-step-plan step 8 — Abel/label routing (scope-decision-first). Estimated 1-2 iterations.

**Per-phase loop** (chevalier owns the sequencing):

1. Implement phase scope
2. Run cross-phase invariants locally (`cargo fmt`, `cargo clippy`, `cargo test` per rubric)
3. Push branch; open PR against `main`
4. Dispatch Codex code-review via `miky agent assign --task pr-review --provider codex --pr <own PR> --expect-sha <own SHA>`
5. On Codex verdict:
   - **APPROVE**: proceed to step 6
   - **REQUEST CHANGES**: iterate within phase (fix findings, push, re-dispatch Codex). Loop until APPROVE.
6. Dispatch dame via the invocation declared in `verification_oracle.dame_invocation.command` above (fallback shape: second Codex `pr-review` invocation with `--brief-file docs/chevalier/mkui-slug-rewrite/dame-rubric.md` mapping APPROVE→BLESS, REQUEST_CHANGES→REFORGE)
7. On dame verdict:
   - **BLESS** on Phases 1-3: merge PR; update `STATE.md`; advance to next phase
   - **BLESS** on Phase 4: DO NOT MERGE; update `STATE.md`; post `COMPLETION.md`; **operator visual smoke test is the merge gate** (per P1 #6 from Codex R1 review — final phase preserves the operator gate that fire-and-forget contract's 2 touchpoints declares)
   - **REFORGE**: iterate within phase per REFORGE structured divergence list. Loop back to step 1 or 3 as appropriate.
8. If dame REFORGE has `blocked_reason: oracle_ambiguity` OR `docker_unavailable` OR `dame_infrastructure_gap` OR `rubric_tampered`: chevalier posts BLOCKED.md; mission pauses until operator amends rubric or unblocks.

## Acceptance criteria (bright-line, self-verifiable + dame-blessable)

Chevalier posts `COMPLETION.md` mapping each criterion to git SHA + test/dame output when ALL pass:

- [ ] **Phase 1 dame BLESS + PR merged** — cite rubric § Phase 1 + dame's YAML verdict + merge SHA
- [ ] **Phase 2 dame BLESS + PR merged** — cite rubric § Phase 2 + verdict + merge SHA
- [ ] **Phase 3 dame BLESS + PR merged** — cite rubric § Phase 3 + verdict + merge SHA
- [ ] **Phase 4 dame BLESS** — cite rubric § Phase 4 (variant-dependent) + verdict; **PR remains OPEN awaiting operator visual smoke test** — do NOT self-merge Phase 4 (per Codex R1 review P1 #6)
- [ ] **Phases 1-3 merged before COMPLETION.md; Phase 4 open + BLESSED at COMPLETION.md** (dame is the merge-authority delegate for Phases 1-3; operator holds final merge for Phase 4)
- [ ] **CHANGELOG.md `[Unreleased]` has an entry per merged phase** (Phase 4 entry drafted in the still-open PR)
- [ ] **`cargo test --workspace` passes** at COMPLETION commit (main + open Phase 4 PR)
- [ ] **`cargo fmt --check` + `cargo clippy` clean** at COMPLETION commit
- [ ] **All existing `mkui-vector2d[-wgpu]` + `mkui-wgpu` tests still pass** (no regression)
- [ ] **Golden-image regression tests exist + pass** in `gpu-offscreen` job at 1×/1.5×/2× DPI
- [ ] **Reference adapter at `docs/chevalier/mkui-slug-rewrite/reference-harness/` unchanged since ratification** — `git log docs/chevalier/mkui-slug-rewrite/reference-harness/` shows no post-`9f76af3` commits from chevalier (dame verifies at every phase per rubric § Cross-phase invariants)
- [ ] **STATE.md updated per phase-boundary** (see § Heartbeat protocol)
- [ ] **Every PR labeled `chevalier` + `mission:mkui-slug-rewrite`** (labels created by operator alongside CHARTER ratification, or chevalier auto-creates via `gh label create`)
- [ ] **Operator visual smoke test at COMPLETION** — operator runs `cargo run -p text --features slug` + `cargo run -p atoms-on-wgpu --features slug` + `cargo run -p native-showcase` and confirms visible-crisper output. (This IS the last-mile gate — see § Interaction points below; chevalier cannot self-verify perceptual dimension.)
- [ ] **COMPLETION.md posted with evidence per criterion** — dame's per-phase verdicts, CI check-suite URLs, test outputs, PR merge SHAs, reference-harness regenerated goldens diff summary

## Blocked signal — only true hard blocks

Chevalier posts `BLOCKED.md` at `docs/chevalier/mkui-slug-rewrite/BLOCKED.md` and pauses only if:

1. **Needs credentials** — external doc access, API token, private repo (unlikely for this scope but preserved for symmetry with Marabot CHARTER)
2. **Oracle ambiguity** — JCGT paper + reference shader + rubric all diverge on a specific pixel behavior (dame REFORGE variant `blocked_reason: oracle_ambiguity`)
3. **Ecosystem gap** — a WGSL/wgpu 29 primitive needed that the version doesn't support, AND handrolling would exceed the 4-week timeout
4. **Docker unavailable** — dame invocation needs Docker + Lavapipe stack to run the reference adapter (per miky #158 N2). If docker is not running on the operator's Mac when dame invokes, dame returns REFORGE with `blocked_reason: docker_unavailable`. Chevalier posts BLOCKED.md pointing at the docker daemon; operator starts docker + re-invokes.
5. **Reference adapter regression** — the adapter at `docs/chevalier/mkui-slug-rewrite/reference-harness/` fails its own sensitivity test (known-good ≠ known-good on rerun, or seeded-mutant ≡ known-good). This indicates the pinned toolchain drifted or the docker image changed. Dame REFORGE variant `blocked_reason: adapter_drift`. Chevalier posts BLOCKED.md; operator investigates + potentially re-ratifies.
6. **Dame infrastructure gap** — the dame invocation (per `verification_oracle.dame_invocation.command`) itself fails structurally — not a REFORGE, an inability to produce any verdict. Chevalier posts BLOCKED.md; operator provides the substrate fix.
7. **Dame verdict dispute** — chevalier disagrees with a dame REFORGE that appears to violate the rubric (blocked_reason: `dame_verdict_dispute` per rubric § Amendment protocol) — operator arbitrates.
8. **Charter shape gap** (Marabot Finding G variant) — chevalier detects it needs a CHARTER-declared state that this document doesn't specify (blocked_reason: `charter_shape_gap`).
9. **Hard timeout hit** — 4 weeks calendar.
10. **Budget cap hit** — $300 total.

**NOT valid escalations** (chevalier handles autonomously):
- Codex REQUEST-CHANGES on a PR (iterate within phase per § Per-phase loop step 5)
- Dame REFORGE on a phase (iterate within phase per step 7)
- Broken commits / test failures during development (fix and retry)
- Recovery from bad merges or rebases (resolve and continue)
- CHANGELOG merge conflicts (resolve textually per prior mkui pattern — keep both sections)
- Choice of Phase 4 variant (route through SFNT/Slug vs restrict bitmap — chevalier documents choice + rationale in the PR)

## Hard limits (safety nets)

- **Calendar timeout**: 4 weeks (restored to v1.1 baseline after Phase 0 removal per adapter pre-ratification). If not converging by week 3, chevalier posts `BLOCKED.md` with `blocked_reason: incomplete_after_3_weeks` + remaining-work list.
- **Token budget**: $300 total (chevalier's tokens + dame's tokens combined). If hit before completion, `BLOCKED.md` with current state + remaining work estimate.
- **PR count**: no hard cap on iterations WITHIN a phase (dame REFORGE cycles are the phase's own signal). Hard cap 4 phases * 8 iterations/phase = 32 PR-pushes total; beyond that, `BLOCKED.md` for operator review.
- **Dame REFORGE cycles per phase**: soft cap 8 REFORGE cycles per phase; if 8 consecutive REFORGEs without BLESS, chevalier posts `BLOCKED.md` (something is systematically wrong — either rubric or chevalier's approach).

Chevalier checks these at every commit boundary + phase boundary; posts BLOCKED if any limit is imminent.

## YOLO scope

Chevalier operates in YOLO mode WITHIN this scope:

**In-worktree operations:**
- All file operations (rm/mv/git-reset/force-push-with-lease on chevalier's own branches)
- All in-crate cargo operations (add/remove/update dependencies within listed crates)
- Running local test binaries + benchmarks
- Running the ratified reference adapter (docker + Lavapipe stack per PROVENANCE.md) for local pre-check
- `git push --force-with-lease` on chevalier's own branches only
- `gh pr create` + `gh pr edit` + `gh pr comment` on chevalier's own PRs
- `gh label create` for `chevalier` / `mission:mkui-slug-rewrite` labels if absent (per Codex R1 rubric defect: labels created at launch prep, not mission)

**Agent dispatch:**
- `miky agent assign --task pr-review --provider codex --pr <own PR> --expect-sha <own SHA>` (mid-mission Codex code-review)
- Dame invocation per `verification_oracle.dame_invocation.command` (second Codex `pr-review` with `--brief-file docs/chevalier/mkui-slug-rewrite/dame-rubric.md`)
- `miky agent log --review <id>` (async polling for either)
- Reading Codex verdicts + dame verdicts + iterating on findings within the phase

**Mission-owned documents (chevalier authors + updates):**
- `docs/chevalier/mkui-slug-rewrite/STATE.md`
- `docs/chevalier/mkui-slug-rewrite/COMPLETION.md`
- `docs/chevalier/mkui-slug-rewrite/BLOCKED.md` (if any block condition fires)

**PR merge authority (delegated by dame BLESS, per Codex R1 P1 #6):**
- Chevalier merges Phase 1, Phase 2, Phase 3 PRs on dame BLESS
- Chevalier DOES NOT merge Phase 4 PR — remains open until operator visual smoke test at COMPLETION

**Scope directories** (chevalier may create + modify code):
- `crates/mkui-vector2d/` — CPU band builder
- `crates/mkui-vector2d-wgpu/` — GPU band upload + WGSL shader
- `crates/mkui-wgpu/` — AA integration + baseline snap
- `crates/mkui-text/` — Phase 4 label routing (if that variant is chosen)
- `examples/{text, atoms-on-wgpu, native-showcase}/` — example updates if needed
- `docs/architecture/` — Phase 4 architecture note (bitmap vs slug labels decision)
- `docs/chevalier/mkui-slug-rewrite/{STATE,COMPLETION,BLOCKED}.md` — mission-owned docs
- `CHANGELOG.md` — `[Unreleased]` entries only (line-edits)

Chevalier does NOT auto-approve:

- **Merging Phase 4 PR** — operator visual smoke test is the merge gate (per Codex R1 P1 #6)
- **Any operation outside declared scope directories** (see § Scope directories above)
- **Any modification of the ratified reference adapter** at `docs/chevalier/mkui-slug-rewrite/reference-harness/` — IMMUTABLE per mission contract; dame verifies via `git log docs/chevalier/mkui-slug-rewrite/reference-harness/` at every phase (must show zero post-`9f76af3` commits from chevalier)
- **Any operation on other repos** (`mikbry/miky`, `mikbry/marabot`, other Marabot components)
- **Any operation that touches credential paths**
- **Any amendment to `dame-rubric.md` or this CHARTER or `codex-8-step-plan.md`** — operator ratification required (all three are IMMUTABLE mission inputs per § Immutability below)
- **Any `git push` to `main` directly** — all changes via PR

## Heartbeat protocol — `STATE.md`

Chevalier maintains `docs/chevalier/mkui-slug-rewrite/STATE.md` (in the mkui repo, alongside CHARTER + rubric + adapter) and updates at each phase-boundary event (PR opened, Codex verdict, dame verdict, PR merged). Structure:

```markdown
# STATE — mkui Slug rendering chevalier mission

**Last updated:** <ISO-8601 timestamp>
**Current phase:** <1, 2, 3, or 4>
**Phase status:** <in-progress | codex-review | dame-review | reforge-iterating | merged>

## Phase 1
- PR: #<N>
- Opened: <ISO-8601>
- Codex verdicts: APPROVE (round <r>, sha <sha>)
- Dame verdicts: REFORGE (round <r>, <blocked_reason> on criterion <id>), BLESS (round <r>)
- Merged: <ISO-8601> (sha <sha>)
- Notes: <chevalier's brief per-phase reflection, ~2-3 sentences>

## Phase 2
- PR: pending
- Status: not started

...
```

Cadence: chevalier updates on every state transition. If chevalier goes N days (default 3) without a PR open OR STATE.md update, orchestrator observes as "quiet" signal (informational; chevalier can respond with a heartbeat if genuinely deep in work).

## Interaction points

**Pre-mission ratification (already complete):**
- Adapter authoring (PR #158 by adapter-author role) — ✅ merged `9f76af3` on 2026-07-28
- CHARTER v1.2 + dame-rubric v1.2 + codex-8-step-plan (THIS PR) — awaits operator ratification

**Point 1: Mission dispatch (operator merges THIS PR)**

- Operator ratifies CHARTER v1.2 + dame-rubric v1.2 + codex-8-step-plan by merging this PR
- Merge SHA becomes the mission-start reference SHA (recorded in `STATE.md` at first update)
- Operator confirms labels exist: `chevalier`, `mission:mkui-slug-rewrite` (chevalier auto-creates if absent per YOLO scope)
- Chevalier launches with these documents as its contract; Phase 1 (Codex plan steps 1+2+3) is chevalier's first work
- Operator does not check in

**Point 2: Mission end (operator merges Phase 4 PR after visual smoke)**

- Chevalier posts `COMPLETION.md` at `docs/chevalier/mkui-slug-rewrite/COMPLETION.md` with all evidence (dame BLESS verdicts for Phases 1-3, dame BLESS verdict for Phase 4 with Phase 4 PR still open)
- Operator returns, reviews delivery arc, verifies criteria, **runs visual smoke test** on the 3 examples on the Phase 4 branch
- On visible-crispness confirmation: **operator merges Phase 4 PR** — this IS the final merge, preserving the 2-touchpoint contract per Codex R1 P1 #6
- If smoke reveals unblessed gaps that the rubric missed: operator files a follow-up + optionally amends rubric v1.3 for a future mission (Finding M territory)

**Between points 1 and 2: NO OPERATOR INTERACTION** for routine operation. Exceptions:
- Chevalier posts `BLOCKED.md` on any of the 10 named block conditions → operator amends or unblocks
- Rubric amendment needed (dame flags `oracle_ambiguity`) → operator ratifies rubric amendment
- Dame substrate failure (first-mission dame is fallback via `miky agent assign --task pr-review --provider codex --brief-file`) → operator may need to intervene if the substrate itself breaks

## Immutability (per Codex R1 P0 #4)

Three files in this directory are IMMUTABLE mission inputs — pinned at operator ratification of this PR:

1. **`CHARTER.md`** (this file) — the mission contract
2. **`dame-rubric.md`** — the per-phase BLESS/REFORGE criteria
3. **`codex-8-step-plan.md`** — the frozen implementation plan (was mutable `#157` body; frozen at ratification)

Plus the pre-ratified adapter:

4. **`reference-harness/`** — merged as PR #158 `9f76af3` on 2026-07-28. IMMUTABLE per that ratification; not modified by chevalier or dame.

**Mission-start reference SHA:** the merge commit of this PR is the mission's ratification SHA. Dame validates that `git log docs/chevalier/mkui-slug-rewrite/{CHARTER,dame-rubric,codex-8-step-plan}.md` shows no post-ratification commits from chevalier (per rubric § Cross-phase invariants).

**Amendment protocol:**
- CHARTER / rubric / Codex plan amendments: operator authors + commits an updated file with an amendment log entry at bottom. Chevalier re-reads at next state transition. Amendments trigger a rubric re-check at the next dame invocation.
- Adapter (`reference-harness/`) amendments: operator re-ratifies via a new adapter PR (separate mission-input update). Chevalier + dame both re-read the adapter at their next invocation.
- Chevalier + dame CANNOT amend any of these documents. Any perceived need for amendment → BLOCKED.md flag + operator action.

## Sprint isolation

- Every PR from this chevalier MUST have labels `chevalier` + `mission:mkui-slug-rewrite` (chevalier auto-creates at mission start if absent per YOLO scope)
- Chevalier does NOT participate in mkui sprint retros (Sprint 18/19 retros are miky orchestrator surface; chevalier's own retro is inside `COMPLETION.md`)
- Chevalier does NOT block Sprint 18 close (`STATE.md` at Sprint 18 close IS the sprint deliverable; COMPLETION lands Sprint 19-20)
- Chevalier is observable via `docs/chevalier/mkui-slug-rewrite/STATE.md` — orchestrator can peek at any time without interrupting

## Cross-references

- **`mikbry/miky#587`** — chevalier epic; Finding L (dame du lac pattern), Finding L follow-up (adapter creation-time independence), Finding N (operator-authors-nothing)
- **`mikbry/ui#157`** — Slug completion tracker (mkui-side); this CHARTER's `execution_oracle` is the frozen copy at `codex-8-step-plan.md`, NOT the mutable `#157` body
- **`mikbry/ui#158`** — adapter-authoring PR (merged `9f76af3`, 2026-07-28) — the ratified verification oracle
- **`chevalier_dame_pattern`** memory — portable pattern (miky orchestrator-side)
- **`chevalier_oracle_integrity`** memory — Marabot Findings A-H integrated into this doc's block conditions + heartbeat + independence chain
- **Marabot CHARTER reference**: `strategy/archive/marabot/rust-rewrite/CHARTER.md` — chevalier-shape reference (single-PR shape; this mission is per-phase per Finding I)
- **Pre-ratification drafts** (v1.0 + v1.1): `strategy/goals/mkui-slug-rewrite/CHARTER.md` + `dame-rubric.md` — retained as historical drafts; superseded by this ratified copy

## Amendment log

- **v1.0-draft** (2026-07-27, mkui orchestrator) — initial draft per `HANDOFF-TO-MKUI-2026-07-27.md`. Location: `strategy/goals/mkui-slug-rewrite/`. Awaits Codex review.
- **v1.1-draft** (2026-07-27, mkui orchestrator, same-day per operator decision) — added Phase 0 (chevalier-authored harness). Location: `strategy/goals/mkui-slug-rewrite/`. Awaits Codex review.
- **v1.2** (2026-07-28, mkui orchestrator, post Codex R1 review + adapter merge) — see § v1.2 changes below. Location: `docs/chevalier/mkui-slug-rewrite/` (mkui repo). Ratified 2026-07-28 via PR #159 merge SHA `b338bd9`.
- **v1.2.1** (2026-07-28, mkui orchestrator, amendment 1 per chevalier Phase 2 BLOCKED.md) — fixed stale § Blocked signal path (`strategy/goals/...` → `docs/chevalier/...`); rubric v1.2.1 adds byte-identical-to-reference exception to Phase 2 "No thin-gap regressions" criterion. Awaits operator ratification via amendment PR merge.

## v1.2 changes

Codex R1 review (2026-07-27) surfaced 6 blocking (P0/P1) + 10 rubric defects. Miky #158 review (2026-07-28) added 4 non-blocking notes. v1.2 folds all fixes:

**Removed (v1.1 → v1.2):**
- Phase 0 (chevalier-authored harness) — recreated Marabot PR #200 oracle-integrity failure per Codex P0 #1. Replaced with pre-ratified adapter at PR #158 (`9f76af3`).
- References to `strategy/goals/mkui-slug-rewrite/` paths — mission docs live in `mikbry/ui` at `docs/chevalier/mkui-slug-rewrite/` (per Codex P1 #5, keeps dame's git-log check on the same repo tree).

**Added:**
- Pre-ratification adapter reference (PR #158, `9f76af3`) as `verification_oracle` — the immutable independent oracle
- Frozen `codex-8-step-plan.md` copy — the `execution_oracle` no longer references mutable `#157` body (Codex P0 #4)
- § Immutability — explicit statement of which files are ratified-and-frozen (Codex P0 #4)
- Docker-availability block condition (miky #158 N2) — dame REFORGE `blocked_reason: docker_unavailable`
- Adapter-drift block condition — dame REFORGE `blocked_reason: adapter_drift` if adapter's own sensitivity test fails on rerun
- Phase 2 scope narrowing — bounded half-physical-pixel dilation only, NOT transform-aware (Codex R1 rubric defect)
- Storage-format execution-oracle note (miky #158 N4) — chevalier is not forced to upstream README's 16-bit; rubric handles both storage modes (dame-rubric § N1)
- Phase 4 merge gate — operator visual smoke replaces chevalier auto-merge (Codex R1 P1 #6)
- Extended YOLO scope directories — `crates/mkui-text/`, `docs/architecture/` (Codex R1 rubric defect)
- Dame command fallback — explicit shape using existing `miky agent assign --task pr-review --provider codex --brief-file` (Codex R1 P0 #3)

**Fixed:**
- `provider: codex, model: sonnet` invalid pairing → replaced with actual `miky agent assign` command shape (Codex R1 rubric defect)
- Hard limits restored to 4 weeks / $300 (Phase 0 removal removes the padding rationale)
- PR count cap: 40 → 32 (Phase 0 removal)
- All `strategy/goals/mkui-slug-rewrite/` paths → `docs/chevalier/mkui-slug-rewrite/`

## Operator sign-off

_Ratified by:_ _pending_
_Ratified date:_ _pending_
_Amendments log_ (post-ratification): see § Amendment log above
