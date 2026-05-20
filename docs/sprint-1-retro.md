# Sprint 1 retro — mkui

> **First non-miky-orchestrator sprint on `mkui`.** Mirroring the Marabot retro shape per the cross-project check-in. The sprint covered the batch contract (#3, #4, #6, #10) on top of carry-over foundational PRs (#1 / #11 and #8 / #12 landed before this sprint formally started). Sprint goal was "lock the backend contract shape so future renderer / catalog work can slot in."

**Window:** 2026-05-12 → 2026-05-20 (~8 days; only the last day was active orchestration — the rest was the dependency chain settling).
**Tag delta:** `v0.1.0` → `v0.2.0` (post-batch-1, refactor(core)) → `v0.3.0` (post-batch-2, backend-contract).
**Throughput:** 6 PRs merged total (3 carry-over + 4 in-sprint), workspace test count 19 → 79 (+60 tests), 9 open issues → 4 remaining.

---

## What shipped

### Carry-over (pre-sprint)
- **#11 → closes #1** — `refactor(core): define mkui-core as the shared component contract`
- **#12 → closes #8** — `feat(wgpu): upstream stonesketch GUI primitives into mkui-wgpu`

### `mkui-batch-2-backend-contract` (this sprint)
| PR | Issue | Theme |
|---|---|---|
| #13 | #3 | console backend renders real component tree (kills hardcoded showcase) |
| #14 | #6 | mkui-web uses extensible component registry, not closed downcast |
| #15 | #4 | mkui-web/console/wgpu aligned to same 5-module shape |
| #16 | #10 | smoke tests across mkui-core, mkui-web, mkui — 19→79 tests |

### Sprint-close artifacts
- `chore: gitignore .claude/` direct-to-main (`fafe12d`) — workspace agent state
- `docs/audit-report.md` (370 lines, 10-category Rust audit, overall 6.4/10) — produced via `miky project audit`

---

## Did the sprint plan survive contact with reality?

**There was no plan document.** That's the highest-signal answer.

Sprint 1 was planned in chat. The "plan" lived as a markdown table I produced ad-hoc when the user asked me to plan a first sprint. No `docs/sprint-1-plan.md`, no immutable goal sentence, no issue table with effort tiers, no conflict-surface analysis, no sequencing rationale. The Sprint 7 plan / retro docs in `miky-internal/` revealed afterwards what the shape *should* have been — and the gap was painful in three specific places (see Section 3).

So the answer to "did the plan survive contact" is **n/a — there was no plan**. The Marabot retro reportedly surfaced the same gap framed differently ("plan-document is invisible to cold agents"). mkui surfaces it as **"plan-document doesn't exist at all if the operator doesn't know to write one."** Marabot at least had a plan that was invisible; mkui's first sprint had no plan to be invisible.

That said — every batch-level decision the operator improvised in chat ended up roughly correct *after* a corresponding mistake (see Section 3). The Sprint 1 plan shape *would have* been:

- **Goal:** lock backend contract so renderer / catalog work can slot in
- **Capacity:** 4 issues per batch (Miky's documented cap)
- **Batch 1 (foundational, pre-sprint):** #1, #8 (sequential, two batches of one) — already shipped
- **Batch 2 (contract):** #4, #6, #3, #10 (parallel, four agents)
- **Conflict surface:** #3 vs #4 on `mkui-console/src/high_level.rs` (caught in retrospect, not at plan time)
- **Sequencing:** merge #4 first (foundational module template), then #6 and #3 (consumers), then #10 (tests on top). Actually merged: #13(#3) → #15(#4) → #14(#6) → #16(#10) — i.e. opposite for the first two. (See Lesson 1 below.)

---

## Top 3 lessons (binding — change behavior in Sprint 2)

### Lesson 1 — `--merge parallel` for rebases is wrong; default to sequential

**The observation.** After merging #13, I injected rebase prompts into **all three** remaining agents in one `miky batch rebase --merge parallel --force` invocation. The user corrected: *"why did you launch a rebase for all agent !!!! one rebase at a time to reduce the number of rebases."*

The math: each merge requires a rebase. Rebasing N agents in parallel when only one will merge next wastes N−1 rebase cycles — those agents will rebase again against post-next-merge main anyway. Sequential rebasing means each agent rebases exactly once per merge. Across this sprint, the wasted-rebase tax was 2 extra rebases per agent (#6 and #10 each rebased 3 times instead of 1 — 6 extra rebases total ≈ 30+ min of agent attention).

**Why it slipped.** `miky batch rebase` default is `--merge sequential` and the docs/help text are clear. I picked `parallel` because I rationalized "they all need it anyway." That's a Sprint 7-Lesson-1-shape failure (fixed the worked example, missed the broader rule): I optimized for "minimize inject invocations" when the broader rule is "minimize rebase work."

**Behavior change.** Saved to `~/.claude/projects/-Users-mik-dev-mikbry-ui/memory/feedback_miky_rebase_sequential.md`. Default to sequential. `--merge parallel` is opt-in only when the operator says it explicitly.

**How to apply.** When a PR merges in a multi-worktree batch: `miky batch rebase <batch> --after-merge <PR> --merged-sha <sha>` with no `--merge` flag. After that agent's PR lands, re-run.

---

### Lesson 2 — Without CI, the "pre-push gates" prompt is unenforceable theatre

**The observation.** Every rebase prompt `miky batch rebase` injected includes:

```
After the rebase, run every gate before pushing:
  cargo fmt --all -- --check
  git diff --check origin/main...HEAD
  cargo test --workspace
  cargo clippy --workspace --all-targets -- -D warnings
If all gates are green, git push --force-with-lease.
```

But:
- `cargo fmt --check` was failing on `main` itself when each agent started (pre-existing fmt debt from PR #12).
- `cargo clippy -D warnings` was failing on `main` with 8 errors in `mkui-core` (PR #12) and **11 errors in `mkui-c`** that the audit later revealed are *soundness* issues, not style.
- No `.github/workflows/ci.yml` exists. The pre-push gates were never enforced server-side.

So agents either (a) ran the gates and saw "main is broken anyway, push and move on" (rational), or (b) skipped the gates entirely (also rational given the agent's narrow PR scope). The prompt's gates didn't gate anything.

**Why it matters.** When the operator (me) checked PR #15 locally, I had to do gate triage that the agent couldn't reasonably do: "is this fmt issue NEW or pre-existing on main?" That triage burned ~10 minutes per PR × 3 PRs = ~30 min of operator attention that wouldn't have existed if CI had been the truth-source. The audit's category-9 score (2/10) plus its top-3 finding #2 ("No CI exists") confirm this is mkui's biggest single operational gap.

**Behavior change.** Sprint 2's batch must include `.github/workflows/ci.yml` as PR #1 of the batch. Phase 1.2 of the audit roadmap. Until CI exists, the rebase prompt should say "DON'T trust `cargo clippy -D warnings` on main yet; only check that you didn't add NEW lints" — but the actual fix is CI, not prompt-rewording.

**Cross-project signal for miky.** The default `miky batch rebase` rebase prompt **assumes CI exists**. For projects that don't have CI yet, the prompt is wrong. Miky could either (a) detect `.github/workflows/*` absence and warn at `miky project init` time, or (b) parameterize the rebase prompt template so first-CI-less batches get a different gate list. Suggestion in Section 4.

---

### Lesson 3 — Audit-driven sprint planning beats operator-vision-driven sprint planning

**The observation.** I drafted "Sprint 2 — Renderer reality" *before* running `miky project audit`. My proposal was: "#2 (full wgpu renderer) is the next foundational unblock, #5 (PyO3) and #7 (README) as smalls."

The audit landed and said: **"#2 is Phase 2 work. Sprint 2 = Phase 1 (operational hygiene)."** Top 3 audit findings — `mkui-c` unsoundness, no CI, `mkui-core` API bugs — are all infrastructure debt that I hadn't surfaced from reading the code. The audit found them in 8 minutes; my code-reading would have taken hours and still missed the soundness piece (which requires actually running `clippy -D warnings`, not reading files).

**Why it slipped.** I treated the audit as "produce a roadmap doc for the human to read later." The user's correction was implicit: they asked me to read the Miky sprint docs *and* run the audit — i.e. the audit is *input to the plan*, not output of it. Miky's documented post-batch workflow makes this binding: *"No audit report at `docs/audit-report.md`. Stale audits are a sprint-close blocker per `docs/CONTRIBUTING.md`."* The warning fired twice during sprint 1's two post-batch runs and I treated it as advisory both times.

**Behavior change.** `miky project audit` is **input to sprint planning, not output**. Run it before drafting any plan doc. The audit's remediation roadmap is the long-list; the sprint plan picks from it based on Phase + effort + dependency. (Sprint 7's plan does this implicitly — the Sprint 6 retro feeds the Sprint 7 issue table.)

**How to apply.** Sprint kick-off ritual:
1. `miky post-batch` (closes the prior sprint cleanly).
2. `miky project audit --stack <rust|python|etc>` produces `docs/audit-report.md`.
3. Read the audit's Phase 1 / Phase 2 task tables.
4. Draft `docs/sprint-N-plan.md` picking from those phases.
5. **Only then** `miky batch create`.

---

## What broke — tactical observations

### CI absence drove ~3 hours of manual gate-running

For each in-flight PR I ran the four gates locally (fmt, diff-check, test, clippy) and triaged "new vs pre-existing." For PR #15 alone that took ~30 min including the nudge-cycle to get one fmt issue fixed via `miky agent inject`. Across #14 and #16 same story. **Without CI, every PR review by the operator is bespoke triage.** Sprint 2 Phase 1.2 fixes this.

### `.claude/` blocked `miky post-batch` mid-flow

Discovered on the Sprint-1 close: `miky post-batch` refused to fast-forward because the primary checkout had untracked `.claude/`. Stashing was the workaround. Now gitignored on main (`fafe12d`), but the discovery happened the *first* time post-batch ran, and the fix was applied as a `chore:` direct-to-main commit. Sprint-2 retro will confirm this is closed.

**Cross-project signal for miky:** `miky post-batch` should probably treat `.claude/` and `.miky/` (and `.code-workspace`) as known-tool-state and tolerate them as untracked when deciding whether the working tree is clean enough to proceed. Or: `miky project init` could write a default `.gitignore` that includes these. Today, the operator has to discover and fix each one. This applies to **every project miky touches**, not just mkui.

### `.miky/issue-conventions.md` lives in a folder that I (then we) decided to gitignore

The repo had `.miky/issue-conventions.md` which defines the project-specific scope vocabulary (`wgpu`, `web`, `console`, `native`, etc.) — that's *shared* project configuration, not ephemeral per-issue state. But it lives alongside `.miky/issue-N-ship-prompt.md` files that *are* ephemeral. We chose to gitignore the whole `.miky/` folder (my recommendation, user confirmed) — losing the shared conventions file. **`miky` should probably split these into two locations**: `.miky/config/` for shared (committed) and `.miky/state/` for ephemeral (gitignored).

### `miky batch rebase` stale-agent guard triggered when agents were just idle

When I tried to inject the *first* rebase prompt, all three remaining agents were 754–756s idle. `miky batch rebase` flagged them as "stale (inject manually, or re-run with --force)" and refused without `--force`. They weren't stale, just waiting on user input after their last turn. **The 12-minute idle threshold may be too aggressive for the "agent just finished its turn and is waiting" case.** A higher threshold (30 min?) or a heuristic ("idle but the last process state was 'awaiting input'") would reduce the `--force` requirement.

### `gh pr checks` returns non-zero when a repo has no CI

I tried `miky pr checks 14` (which delegates to `gh pr checks`) and got `exit 1` with "no checks reported on the branch." That's correct from `gh`'s side but it interacts badly with `miky` workflows that condition on the return code. **Miky could degrade gracefully here**: treat "no checks configured" distinctly from "checks failed."

### `miky pr` lacks a `diff` subcommand

I wanted to peek at PR #13's diff before merging. `miky pr` has `view` / `checks` / `merge` / `logs` / `rerun` but no `diff`. I fell back to `cd`-ing into the worktree and using `git diff main..HEAD --stat`. Workable, but adding `miky pr diff <N> [--stat]` would close the gap.

### Audit instructions file `.claude-audit-instructions.md` not in default .gitignore

`miky project audit` produces `.claude-audit-instructions.md` (a sibling to `.claude-agent-instructions.md`). The latter is in the default `.gitignore` that `miky project init` should write; the audit-instructions file isn't. Same shape as the `.miky/` / `.claude/` issue.

---

## Discipline read — does miky fit mkui's shape?

**Short answer: yes, with three caveats.**

The Marabot retro reportedly answered "no — plan-document gap" (per the user's framing). mkui's answer is **"yes overall, but the discipline assumes CI-and-audit-driven sprint planning, and Sprint 1 ran neither."** Once both are in place (Sprint 2), the discipline should fit cleanly. The three caveats:

### Caveat 1 — Batch size of 5 may be too large for mkui's file-collision shape

mkui's workspace is small (9 library crates, ~5 source files each). Two issues that touch the same backend (#3 + #4 both touched `mkui-console/src/high_level.rs`) had a real collision. The Sprint 7 plan's conflict-surface analysis would have caught this at plan time — but even with that analysis, two parallel agents rewriting the same file in parallel is a structural problem, not a sequencing one.

**Suggested adaptation for mkui:** target 3-4 issues per batch, not 5. mkui's "novel-surface" PRs touch fewer files than miky's, but the files they touch are denser (file-per-backend pattern). The capacity isn't bounded by Codex throughput (we don't run Codex yet) but by the operator's review attention per PR.

### Caveat 2 — mkui's "Codex tier" classification is currently 1-dimensional

Miky's Sprint 7 retro defines five PR tiers: Mechanical / Template / Typed-primitive / Substrate / Novel-surface, with per-tier Codex round budgets. For mkui without Codex in the loop yet, the tiers still apply to *operator review attention*. PR #15 (#4 — module restructure) was substrate tier and needed two operator interventions (rebase nudge + fmt nudge). PR #13 was template tier and needed zero. PR #16 was substrate (smoke tests across 3 crates) and benefited from the wait-and-iterate decision (Lesson 2 of this retro will note: "agent dirty working tree is a signal").

**Suggested adaptation:** when miky's batch-create or batch-start grows a tier-flag, mkui will use it. Until then, surface it in `docs/sprint-N-plan.md` per-PR.

### Caveat 3 — The discipline assumes the operator knows the project shape

When the user asked me to list issues and plan a first sprint, I produced a 9-row issue table from `miky issue list`. That's correct mechanics. But:
- The shape of mkui (downstream consumer = stonesketch + Miky, target = native macOS app, deferred = web/wasm) lives in `miky-internal/docs/OPERATOR_CONSOLE_DESIGN.md` and `astoneer/stonesketch/docs/gui.md`, not in mkui's own README.
- The operator (me) had to read those *out-of-repo* docs to plan the sprint correctly.

For mkui specifically this worked because I had access. For a cold agent (or a different orchestrator), the sprint plan would have been wrong. **mkui needs an in-repo `docs/positioning.md` or `docs/downstream-consumers.md` so the project shape is visible without cross-repo reading.** Issue #7 (README positioning) covers some of this; a separate `docs/downstream-consumers.md` would cover the rest.

This is the **mkui-specific agent-skills gap** the prompt asks about: not "sprint-plan-artifact is invisible" (Marabot's), but **"project positioning lives in downstream-consumer docs that mkui can't see."** Per Sprint 7's lesson on the broader-scope failure mode: the broader rule is *"every project must self-describe its consumers + downstream constraints, or sprint planning depends on operator side-channel knowledge."*

### What's the highest-signal cross-project ask?

If miky added **one** thing for the next non-miky-orchestrator sprint, it should be:

> A `miky project audit` mode that runs *before* the first batch (not just before the sprint close), and the audit's Phase 1 findings auto-populate the first sprint's issue set.

That closes the lesson-3 loop (audit-driven planning) and removes the operator's "what should Sprint 1 cover?" guess entirely. The audit knows what's broken; the sprint plan should consume that.

---

## Cross-project bug list (anticipated by the prompt + new)

| Bug | Anticipated? | Status |
|---|---|---|
| `.claude/` blocked post-batch | yes — discussed | Fixed locally; suggest miky default-ignore |
| `.miky/` mixes shared + ephemeral state | new | Recommended split `.miky/config/` vs `.miky/state/` |
| `.claude-audit-instructions.md` not in default gitignore | new | Local fix in mkui; suggest miky template fix |
| Rust stack detection wrong for mkui shape | tested — **no, autodetection works**. `miky project audit --stack rust` rendered the right 10-category checklist | OK |
| Agent-instruction template assumes `crates/<name>/tests/` | tested — works. mkui follows that layout exactly | OK |
| `miky doctor` gap | partially — pre-#168, the gap is "stale audit", "no CI", "dirty primary checkout", "stale agents" all surfaced as separate post-batch warnings rather than one health digest | Noted; #168 will close |
| Cross-platform issues | none observed (Darwin 24.6.0) | n/a — single-platform run |
| Rebase prompt assumes CI exists | new — Lesson 2 | Reported above |
| Stale-agent guard 12 min threshold | new | Reported above |
| `miky pr checks` returns non-zero on no-CI repos | new | Reported above |
| `miky pr diff` missing | new | Reported above |

---

## Parked candidates (Sprint 2+)

1. **`docs/downstream-consumers.md`** — capture stonesketch + Miky as named consumers, so the project shape is visible without cross-repo reading. Sprint 2 small.
2. **`docs/CONTRIBUTING.md`** — referenced by `miky post-batch` audit warning ("stale audits are a sprint-close blocker per docs/CONTRIBUTING.md") but the file doesn't exist. Sprint 2 small.
3. **Sprint-plan template adaptation** — mkui-specific shape with capacity-3-not-5 default + per-PR tier annotation. Sprint 3 once Phase 1 lands.
4. **Pre-push hook for clippy** — Sprint 7 Lesson 1 in miky's retro mentioned this; same shape applies to mkui. Sprint 3 after CI is in place.

---

## What's bound vs. parked at end of Sprint 1

**Bound (changing behavior in Sprint 2):**
1. `miky batch rebase` defaults to sequential. `--merge parallel` only when explicit. (Memory saved.)
2. `miky project audit` runs *before* sprint planning, not after. Audit's Phase 1 = Sprint N+1's issue set.
3. Every sprint produces a `docs/sprint-N-plan.md` doc with the Miky Sprint 7 template (immutable goal, issue table, conflict surface, sequencing, risks, success criteria).

**Parked (Sprint 2 candidates):**
1. `docs/downstream-consumers.md`
2. `docs/CONTRIBUTING.md`
3. `docs/sprint-1-plan.md` retroactive (so future sprints can see how Sprint 1 *should* have been planned)

---

## Sprint 2 seeds (audit-driven)

Per Lesson 3, Sprint 2's issue set comes from the audit's Phase 1 (5 tasks).

- **Goal:** *Phase 1 of the audit — operational hygiene foundation*
- **Headline issues:**
  - Phase 1.1 (new) — `mkui-c` FFI safety (declare `unsafe extern`, add `// SAFETY:`)
  - Phase 1.2 (new) — `.github/workflows/ci.yml` (fmt + clippy + test gates)
  - Phase 1.3 (new) — Fix 8 `mkui-core` clippy errors (Defaults, FromStr, Display, rename `add`)
  - Phase 1.4 (new) — Declare `rust-version = "1.74"` + workspace `lints` table
  - **#7** — README rewrite (closes existing issue; Phase 1.5)
- **Cadence:** 1 batch, 5 PRs parallel — all 5 PRs are file-independent per audit's category-1 finding.
- **Sequencing:** 1.4 (MSRV trivial) → 1.2 (CI — once in, gates the rest) → 1.1 (mkui-c safety) → 1.3 (mkui-core clippy) → 1.5 / #7 (README absorbs anything else).
- **Tier projection:** Mechanical (1.4) / Template (1.2) / Substrate (1.1, 1.3) / Template (1.5). Per-tier operator-review-attention: 5 min / 10 min / 30 min × 2 / 15 min = ~90 min total review attention.
- **Sprint 2 plan doc:** to be written separately when the sprint launches (per Lesson 3: audit → plan doc → batch).
- **Deferred to Sprint 3:** #2 (full wgpu renderer), #9 (native boundary), #5 (PyO3).
- **Deferred to Sprint 4+:** Miky catalog (Chip, Dot, StatePill, etc.) — depends on Sprint 3's renderer.

---

**Sprint 1 closes here.**

`v0.3.0` shipped. Backend contract locked. Workspace test count 4×'d. Audit identifies 6.4/10 health, with the gap concentrated in operational hygiene that Sprint 2 closes. The next sprint plans fresh, this time from a `docs/sprint-2-plan.md` written *after* the audit.
