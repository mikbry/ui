# Contributing to `mkui`

> **This document is the audit-referenceable source for sprint discipline in `mkui`.** It exists primarily to close the dangling reference from `miky post-batch`'s audit-staleness warning ("*Stale audits are a sprint-close blocker per docs/CONTRIBUTING.md*") and to make the Sprint-Done gate visible to cold orchestrators planning the next sprint. The substance below is the mkui-specific adaptation of `mikbry/agent-skills#2`'s sprint-and-batch discipline.

---

## Sprint and batch discipline

mkui follows the sprint-and-batch shape defined in `mikbry/agent-skills#2`:

1. **Sprint** — a 3-4 day calendar window (mkui calibration: file-dense workspace, 3-4 issues per batch) with an immutable sprint goal.
2. **Batch** — a parallel-launched set of issues whose worktrees ship PRs against the same sprint goal. mkui's calibration is 3-4 issues per batch; the audit's Phase 1 (5 file-independent tasks) is the documented exception.
3. **Sprint plan** — a `docs/sprint-N-plan.md` artifact written *before* batch creation (per Sprint 1 retro Lesson 3: audit-driven planning). Shape mirrors `mikbry/miky-internal/docs/sprint-7-plan.md`: immutable goal, issue table with effort tiers, conflict-surface analysis, sequencing, risks, success criteria.
4. **Sprint retro** — a `docs/sprint-N-retro.md` artifact written *at sprint close*. Shape mirrors `mikbry/miky-internal/docs/postmortems/2026-05-19-sprint-7-retro.md`: what shipped, did the sprint plan survive contact, top-3 binding lessons, what broke tactically, parked candidates.

### Sprint-Done gate (8 items)

A sprint is **formally closed** when all 8 items below are satisfied. The Sprint 1 retro (2026-05-20) named this gate explicitly per Miky's reply on `mikbry/agent-skills#2`; the gate replaces ambiguous "decide you're done" rituals.

| # | Item | How mkui satisfies it |
|---|------|------------------------|
| 1 | Every planned PR merged or swapped (no silent drops) | `miky batch status <batch> --all-done` reports all issues as `done` |
| 2 | `miky post-batch` ran cleanly | The fast-forward, version bump, and cleanup all OK'd |
| 3 | Tag landed | `git tag --list 'v*' \| tail -1` shows the bumped tag |
| 4 | Wave notes committed (if applicable) | Implicit in batch state for single-batch sprints; explicit file for multi-batch |
| 5 | Sprint retro written | `docs/sprint-N-retro.md` exists and is committed |
| 6 | Top-3 lessons bound to behavior changes | The retro names each lesson + its memory file or skill-promotion target |
| 7 | Parked candidates seeded for next sprint | The retro's "parked candidates" section is non-empty (or explicitly "none") |
| 8 | Audit status non-stale (per `mikbry/miky#180`) | `docs/audit-report.md` exists and was produced within the current sprint window |

**Practical close-out commit pattern.** A single `docs: close-out sprint-N` commit on `main` that contains:

- `docs/audit-report.md` (fresh, regenerated via `miky project audit`)
- `docs/sprint-N-retro.md`
- `docs/sprint-(N+1)-plan.md` (the next sprint's plan, since the retro feeds the next plan)
- Any retro-bound documentation updates (e.g. this `CONTRIBUTING.md`, `docs/downstream-consumers.md`)

This single commit satisfies items 5, 7, and 8 simultaneously. Items 1-4 are satisfied by `miky post-batch` running cleanly upstream of the close-out commit. Item 6 is satisfied by the retro's content.

---

## Audit refresh policy

Per `mikbry/miky#180`, `miky post-batch` warns when `docs/audit-report.md` is missing or older than the current sprint's window. The warning is a **sprint-close blocker** in `mkui` because:

- Sprint planning is audit-driven (Sprint 1 retro Lesson 3 — binding behavior change).
- An out-of-date audit produces an out-of-date sprint plan, which produces issue selection misaligned with the actual state of the workspace.

**The cadence:**

1. **Run `miky project audit --stack rust`** before drafting any new sprint plan. Mkui is a Rust workspace; this is the right stack.
2. **The audit writes `docs/audit-report.md`.** Commit it in the close-out batch (see "Sprint-Done gate" above).
3. **Sprint N+1's plan picks issues from the audit's Phase 1 / 2 / 3 task tables.** Operator vision *can* contribute candidates but does not override the audit's prioritization without explicit rationale in the plan doc.
4. **The audit is read alongside `docs/downstream-consumers.md`.** The audit says what's broken; downstream-consumers says who's blocked. The intersection is the sprint backlog.

If a sprint plan diverges from the audit's prioritization, the plan doc must explicitly justify the deviation (e.g. "Sprint 3 ships Phase 2.8 ahead of Phase 2.1 because StoneSketch unblocks immediately"). Operator preference without rationale is a planning smell.

---

## Commit and PR discipline

### Conventional commit format

Per `.miky/issue-conventions.md` (gitignored but documented in this section since the conventions are shared project state):

- **Types:** `feat`, `fix`, `refactor`, `docs`, `test`, `chore`, `perf`, `style`, `build`, `ci`
- **Scopes:**
  - mkui-specific: `core`, `wgpu`, `native`, `web`, `console`, `py`, `c`, `common`, `rsx`, `examples`
  - miky-defaults inherited: `auth`, `api`, `agent`, etc. — most are irrelevant to mkui
- **Description:** lowercase, no trailing period

Example: `feat(wgpu): upstream stonesketch GUI primitives into mkui-wgpu`

### PR shape

- One issue per PR. Use `Closes #N` in the body so `miky batch refresh` picks it up automatically.
- Body sections (in order): **Summary**, **What landed**, **Acceptance criteria** (linked to the issue's criteria), **Scope notes** (what's intentionally NOT addressed), **Verification** (the commands run), **Test plan** (checkboxes the reviewer can verify).
- Co-authored-by Claude where applicable: `Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>`
- No `--no-verify` or sign-bypass. Pre-push gates exist for a reason.

### Pre-push gates (post-Sprint-2)

Once Sprint 2's Phase 1.2 lands `.github/workflows/ci.yml`, the gates are:

```bash
cargo fmt --all -- --check
git diff --check origin/main...HEAD
cargo test --workspace --exclude mkui-py
cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings
```

CI enforces these on every PR push. Local pre-push hooks may run them as a courtesy but are not required.

**Before Sprint 2's Phase 1.2 lands** (i.e. during Sprint 2 itself), agents working on the batch run the gates locally and report results in the PR body — the operator verifies before merge. Sprint 1 documented the manual triage tax (~30 min/PR); Sprint 2 closes it.

---

## Where things live

- **Sprint plans:** `docs/sprint-N-plan.md`
- **Sprint retros:** `docs/sprint-N-retro.md`
- **Audit report:** `docs/audit-report.md`
- **Downstream-consumer map:** `docs/downstream-consumers.md`
- **Architecture ADRs (Sprint 3+):** `docs/architecture/000N-*.md`
- **Issue conventions (project-specific scopes):** `.miky/issue-conventions.md` (gitignored; documented in this CONTRIBUTING)
- **Per-issue ship prompts:** `.miky/issue-N-ship-prompt.md` (gitignored; ephemeral)
- **Claude Code per-worktree state:** `.claude/` (gitignored)
- **Tool-generated agent briefs:** `.claude-agent-instructions.md`, `.claude-audit-instructions.md` (gitignored)
- **VS Code multi-root workspace:** `*.code-workspace` (gitignored; per-developer)

---

## Cross-project signal

mkui is the second non-miky project (after `mikbry/marabot`) modeling miky's sprint-and-batch discipline. Per the 2026-05-20 cross-project check-in, every gap mkui surfaces is signal that miky's own orchestrator sessions wouldn't find. The current bound contributions are tracked in `mikbry/agent-skills#2`:

- **Audit-driven planning** (Sprint 1 retro Lesson 3) → `ground-in-current-state-before-planning.skill.md` integration
- **Project self-description** (Sprint 1 retro Caveat 3) → `project-self-description.skill.md` (new skill; `docs/downstream-consumers.md` is the canonical example)
- **Calibration table** (Sprint 1 retro Caveat 1) → `sprint-and-batch-discipline.skill.md` (existing skill, mkui rows added)

When future sprint retros surface new gaps, file them as comments on `mikbry/agent-skills#2` or as bugs on `mikbry/miky` per the filing-split convention (mkui files 3 owed bugs at Sprint 1 formal close; subsequent retros follow the same pattern).

---

**Last updated:** 2026-05-20 (Sprint 1 close-out batch).
