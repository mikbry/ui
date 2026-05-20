# Sprint 2 Plan — "Operational hygiene foundation"

**Window estimate:** 2026-05-21 → 2026-05-24 (3-4 day sprint per the mkui calibration in `mikbry/agent-skills#2`; file-dense workspace makes collision-surface dominate, not throughput)
**Tag target:** `v0.4.0`
**Batches:** 1 — `mkui-batch-3-phase-1-hygiene` (single batch, 5 file-independent PRs)
**Capacity:** 5 active issues. All five are Phase 1 of `docs/audit-report.md`. File-independence per the audit's category-1 finding means parallel is safe.

---

## Sprint goal (immutable)

> **Land Phase 1 of the audit roadmap — make the workspace's operational hygiene match the architecture's quality.** Wire `.github/workflows/ci.yml`, fix `mkui-c`'s 11 soundness errors, fix `mkui-core`'s 8 std-trait-shadowing API bugs, declare MSRV, and rewrite the README. Exit criteria: `cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings` exits 0 against `main`; CI is green on a fresh PR.

Three threads:

1. **CI as ground truth** — `.github/workflows/ci.yml` is the load-bearing artifact. Without it, every Sprint 1 PR triaged "is this fmt issue NEW or pre-existing on main?" manually (~30 min/PR of operator attention). CI replaces that triage with a server-side gate.
2. **Soundness + API correctness** — `mkui-c` shipping `pub extern "C" fn` without `unsafe` makes the FFI surface unsound by Rust's standards (11 hard `clippy::not_unsafe_ptr_arg_deref` errors). `mkui-core` shipping inherent `to_string` and conflicting `from_str` silently shadows std traits — a downstream consumer bringing `std::str::FromStr` into scope breaks. Both fixes are mechanical-to-substrate tier; both are blocking any "external consumer is the smoke" experiment.
3. **Discoverability discipline** — README claims `mkui-native` is the WGPU backend (false post-PR #12), advertises v0.2.0 (workspace is at v0.3.0), and presents a Python build flow that doesn't work on Python 3.14. Issue #7 is the canonical fix; it lands in this sprint.

---

## Sprint 2 issue set

| # | Title | Effort | Role |
|---|---|---|---|
| **Phase 1.4 (mechanical, ships first)** | | | |
| new | Declare `rust-version = "1.74"` + workspace `lints` table | **small** | MSRV floor; unblocks every other PR's clippy enforcement |
| **Phase 1.2 (template, ships second)** | | | |
| new | Add `.github/workflows/ci.yml` (fmt + clippy + test gates) | **small** | server-side ground truth; once landed, every subsequent PR gates on it |
| **Phase 1.1 (substrate, ships third)** | | | |
| new | `mkui-c` FFI safety — declare `unsafe extern "C" fn` + `// SAFETY:` blocks | **medium** | resolves 11 clippy errors + the soundness gap audit's highest-priority finding |
| **Phase 1.3 (substrate, ships fourth)** | | | |
| new | Fix 8 pre-existing `mkui-core` clippy errors (Default impls, std-trait shadowing, `add` rename) | **medium** | resolves the std-trait ambiguity traps; API correctness, not style |
| **Phase 1.5 (template, ships last)** | | | |
| **#7** | README: position mkui as internal open framework, reflect v0.3.0 + `mkui-wgpu` presence | **medium** | absorbs everything else, lands last so it can cite the actual ci.yml + MSRV |

Five issues. mkui calibration table says 3-4 per batch; we're at 5 because the audit's Phase 1 has exactly 5 file-independent tasks and splitting would create artificial sprint boundaries. The dependency chain is **sequential merge order**, not file collision.

**Issues to file before launch:** Phase 1.1, 1.2, 1.3, 1.4 are not yet GitHub issues. Phase 1.5 = existing #7.

---

## Conflict surface analysis

The audit's category-1 finding says all five Phase 1 tasks touch disjoint files. Verified here:

### 1. `Cargo.toml` (workspace package)

Touched by:
- **Phase 1.4** — adds `rust-version = "1.74"` to `[workspace.package]`; adds workspace `[lints]` table
- **Phase 1.3** — possibly removes the `derive_more` dep if the rename of `StyleClass::add` lets us drop a derive

**Resolution:** Phase 1.4 lands first (no dep changes). Phase 1.3 layers on top — if it touches `Cargo.toml` at all, it's the `[workspace.dependencies]` block, disjoint from `[workspace.package]`'s MSRV line.

### 2. `.github/workflows/ci.yml`

Touched by:
- **Phase 1.2** only — creates the file

Zero conflict. Future Phase-1 PRs are gated by it once it merges.

### 3. `crates/mkui-c/src/lib.rs`

Touched by:
- **Phase 1.1** only — declares every `pub extern "C" fn` as `pub unsafe extern "C" fn`, adds `// SAFETY:` blocks

Zero conflict with any other Phase 1 task. The file is `mkui-c`-only.

### 4. `crates/mkui-core/src/{style.rs, theme.rs, headless/{button,text,toggle}.rs}`

Touched by:
- **Phase 1.3** only — adds `Default` impls, replaces inherent `to_string` with `Display`, replaces conflicting `from_str` with `FromStr`, renames `StyleClass::add` to something non-conflicting

Zero conflict with any other Phase 1 task. Phase 1.3's downstream churn (`StyleClass::add` rename will hit every call site in `mkui-web`/`mkui-console` that passes class names) is included in the same PR so the workspace stays green.

### 5. `README.md`

Touched by:
- **Phase 1.5** only — rewrites sections 343-385 (Current Focus + Crate Layout), 144-194 (mkui-native vs mkui-wgpu), 271-300 (Python build flow)

Zero conflict with any other Phase 1 task. README lands last so it can describe `ci.yml` + MSRV + the corrected `mkui-c` safety contract.

### Low-conflict / additive issues

- All five PRs touch different roots: `Cargo.toml` vs `.github/` vs `mkui-c/` vs `mkui-core/` vs `README.md`. The only ambiguity is **`mkui-core/src/lib.rs`** if Phase 1.3 adds `#![forbid(unsafe_code)]` (which is actually Phase 2.4 work — keeping it out of Phase 1 for scope discipline).

---

## Batch composition

### mkui-batch-3-phase-1-hygiene (5 agents, sequential merges)

**Goal:** ship Phase 1's 5 tasks in dependency order. The order is dictated by what gates what:

| # | Issue | Why this batch | Estimated PR size |
|---|---|---|---|
| Phase 1.4 | MSRV + lints table | unblocks every PR's clippy enforcement | small |
| Phase 1.2 | `.github/workflows/ci.yml` | server-side gate; once landed, every later PR runs against it | small |
| Phase 1.1 | `mkui-c` FFI safety | resolves 11 clippy errors; soundness gap | medium |
| Phase 1.3 | `mkui-core` clippy fixes | resolves 8 clippy errors + std-trait shadowing | medium |
| Phase 1.5 / #7 | README rewrite | absorbs everything; cites the now-merged ci.yml + MSRV | medium |

**Why these five together:**
- All five are Phase 1 of the same audit roadmap; landing 4-of-5 leaves the workspace in an inconsistent half-fixed state.
- All five are file-independent per the conflict-surface analysis above.
- 5 agents == miky's batch cap but exactly matches Phase 1's structure. mkui's calibration (3-4 per batch) is exceeded by one — accepted because all five are sequenced (not truly parallel) and each PR is small-to-medium.
- Sprint 1 retro's Lesson 1 (sequential rebases, not parallel) applies in full: each PR rebases sequentially as the prior one lands.

**Codex review required (per `feedback_route_pr_review.md` and the Sprint 7 Lesson 2 tier table):**

- **Phase 1.4** — **no Codex / 1 round**. Single-line MSRV declaration + lints table. Tier: mechanical.
- **Phase 1.2** — **1 round**. CI YAML file; can be copied from `mikbry/miky/.github/workflows/ci.yml` as the template. Tier: template.
- **Phase 1.1** — **3 rounds**. Genuinely substrate — every `extern "C" fn` needs an `// SAFETY:` block describing the pointer-validity contract. Tier: substrate. Per Sprint 7 Lesson 2: 3 rounds, not 4, because the *pattern* is well-established (UnsafePtrArgDeref is canonical Rust).
- **Phase 1.3** — **2 rounds**. Typed-primitive tier — adding `Default` impls and migrating to `FromStr`/`Display` traits is a standard refactor. Mid-batch churn (call-site renames for `StyleClass::add`) makes it borderline substrate; 2 rounds is honest.
- **Phase 1.5 / #7** — **2 rounds**. README + maybe 2-3 ADR stubs in `docs/architecture/`. Template tier. Codex catches drift between claims and code.

Sprint-level Codex budget: **9 rounds total** (0 + 1 + 3 + 2 + 2 = 8, +1 buffer for Phase 1.1 if needed). At ~10 min/round of orchestrator-paste cycle, that's ~80 min of Codex routing across the sprint.

**Note:** mkui doesn't currently run Codex in the loop. The tier classification is *forward-looking* — the same per-tier attention budget applies to *operator review attention* in the interim. Per-tier human-attention projection: 5 / 10 / 30 / 20 / 15 = **~80 min total operator review attention** across the sprint.

---

## Sequencing within mkui-batch-3-phase-1-hygiene

Strict merge order: **Phase 1.4 → 1.2 → 1.1 → 1.3 → 1.5**.

### Phase A — Phase 1.4 lands first (Day 1, ~hour)

Agent starts immediately. Single file edit (`Cargo.toml`). Adds `rust-version = "1.74"` + optional `[workspace.lints]` block. Verify with `cargo build --workspace --exclude mkui-py` (no MSRV error).

After Phase 1.4 merges: `miky batch rebase mkui-batch-3-phase-1-hygiene --after-merge <PR>` — **sequential, not parallel** per Sprint 1 retro Lesson 1.

### Phase B — Phase 1.2 lands second (Day 1, ~half-day)

Agent creates `.github/workflows/ci.yml`. Template from `mikbry/miky` (Rust workspace with cargo fmt + clippy + test). Configure:
- Trigger on `push` to `main` + `pull_request`
- Jobs: `fmt` (cargo fmt --check), `clippy` (cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings), `test` (cargo test --workspace --exclude mkui-py)
- Cache `~/.cargo` + `target/` per the standard `actions/cache@v4` pattern
- Matrix: Linux + macOS at minimum; Windows optional (Sprint 1 retro flagged the cross-platform gap)

After Phase 1.2 merges: every subsequent PR is gated by CI. The "manual gate triage" tax from Sprint 1 ends.

### Phase C — Phase 1.1 lands third (Day 2, ~day)

Agent works on `mkui-c/src/lib.rs`. Per the audit:
- Re-declare every `pub extern "C" fn` as `pub unsafe extern "C" fn`
- Add `// SAFETY:` block above each `unsafe { ... }` block (the audit lists every line number: 76, 92, 98, 121, 129, 135, 159, 167, 183, 204, 221)
- Update `crates/mkui-c/include/mkui.h` (the cbindgen-generated header) if needed — likely auto-regenerated, verify
- Document the pointer-validity contract in `crates/mkui-c/src/lib.rs:1-30` (crate-level doc-comment)
- Update the C and C++ example files if any (`examples/`?) to reflect that callers are responsible for the contract

Pre-push gates (now enforced by CI): `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace --exclude mkui-py`. **CI is the source of truth.**

After Phase 1.1 merges: clippy on `main` drops from 19 errors to 8.

### Phase D — Phase 1.3 lands fourth (Day 2-3, ~day)

Agent works on `mkui-core/src/{style.rs, theme.rs, headless/}`:
- Add `impl Default for ButtonBuilder { ... }` (`headless/button.rs`)
- Add `impl Default for TextBuilder { ... }` (`headless/text.rs`)
- Add `impl Default for ToggleBuilder { ... }` (`headless/toggle.rs`)
- Remove inherent `Text::new(content) -> TextBuilder` (returns Self, fix elsewhere)
- Replace inherent `StyleClass::to_string` with `impl Display for StyleClass`
- Replace inherent `ColorTheme::from_str` with `impl FromStr for ColorTheme`
- Rename `StyleClass::add` to `StyleClass::push` or `StyleClass::with_class` (avoid `std::ops::Add` confusion). Update **every** call site in `mkui-web`, `mkui-console`, `mkui-wgpu`, and `examples/` in the same PR — Sprint 7 retro Lesson 1's broader-scope rule.

After Phase 1.3 merges: clippy on `main` drops from 8 errors to 0 (or the remaining count is documented in the PR description with rationale).

### Phase E — Phase 1.5 / #7 lands last (Day 3, ~half-day)

Agent rewrites README sections per the audit (`README.md:343-385, 144-194, 271-300`):
- Drop the "Phase 1: Foundation (COMPLETED)" line claiming features that don't exist
- Update "Current Focus" from v0.2.0 → v0.4.0 (this sprint's target tag)
- Rewrite crate-layout section to mention `mkui-wgpu` (added PR #12), correctly describe `mkui-native` as a placeholder (tracked in #9)
- Mark `mkui-py` build flow as broken on Python 3.14 (link to #5)
- Add new "Recommended current use" section: `mkui-core` (stable), `mkui-web` (stable), `mkui-wgpu` (primitives stable, renderer experimental), `mkui-console` (stable)
- Add (optional, if scope allows) `docs/architecture/0001-mkui-core-as-contract.md` ADR stub per audit recommendation 10.2

Operator-witnessed: read the rendered README on GitHub after merge to confirm no claims are still wrong.

---

## Sprint 2 risks (from the planning view)

### Risk 1 — `StyleClass::add` rename has a wide call-site surface

The audit notes `StyleClass::add` is called from `mkui-web/src/components.rs` and elsewhere via `.class()` chains. Renaming will touch every example app + every backend. If the call sites aren't exhaustively updated in the same PR, the workspace fails to compile mid-Phase-1.3.

**Mitigation:** the Phase 1.3 issue body must explicitly list "rename `StyleClass::add` workspace-wide" as part of the AC. The agent runs `grep -rn 'StyleClass::add\|\.add("' crates/ examples/` (or `cargo check` until clean) before pushing.

### Risk 2 — CI workflow has macOS-specific failures

`mkui-wgpu`'s scene primitives use no platform-specific code today, but `mkui-web` pulls in `wasm-bindgen`/`web-sys` and `mkui-console` uses `crossterm`. CI on Linux should pass for all non-wasm crates; the wasm builds need `cargo build --target wasm32-unknown-unknown`. First time wiring this — may take a re-spin.

**Mitigation:** Phase 1.2's agent runs the workflow locally first via `act` (or just runs the same shell commands). If wasm build needs special config (`wasm-pack`, target install), document in `ci.yml` comments.

### Risk 3 — Phase 1.1 `mkui-c` FFI safety has soundness subtleties

Adding `// SAFETY:` blocks isn't just a formality — the contract for `mkui_app_add_button(*mut MkuiApp, *const c_char, *const c_char)` needs to specify: (a) `app` must point to a valid `MkuiApp` previously returned from `mkui_app_new()`, (b) both `*const c_char` pointers must be valid C strings (NUL-terminated, valid UTF-8 *or* the function documents it accepts non-UTF-8), (c) the strings must outlive the call. If the agent under-specifies, the safety comments are decorative.

**Mitigation:** the Phase 1.1 issue body cites the Rust reference's FFI section + `std::ffi::CStr::from_ptr` safety docs as the contract template. Verify each block answers: pointer validity, lifetime, ownership transfer, NUL-termination guarantees.

### Risk 4 — README rewrite drifts from reality during the sprint

Phase 1.5 lands last but writes claims about `ci.yml` (1.2), MSRV (1.4), `mkui-c` safety (1.1), and `mkui-core` correctness (1.3). If any of 1.1–1.4 land in a different shape than the issue body promised (e.g. MSRV picks 1.75 instead of 1.74), the README inherits the drift.

**Mitigation:** the Phase 1.5 agent's first step is to run `cargo --version && grep rust-version Cargo.toml && cat .github/workflows/ci.yml | head -30` and quote from the actual files, not from the audit's prescriptions.

### Risk 5 — Sprint 1's "novel-surface 4-round" pattern may apply to Phase 1.1

`mkui-c` FFI safety is technically a known-pattern fix (audit calls it "well-established"), but every `// SAFETY:` block in a Rust workspace is bespoke — there's no `cargo fix` for this. If the agent under-specifies, a second-round operator review surfaces it; if it over-specifies, comments are noise.

**Mitigation:** budget 3 rounds explicitly. If a 4th round becomes necessary, that's a signal that the FFI surface needs deeper redesign (per Sprint 7 retro: 4-round PRs caught real bugs, not nits). Adjust sprint scope rather than crashing through.

---

## Sprint 2 success criteria

- [ ] **Sprint goal met:** `cargo clippy --workspace --exclude mkui-py --all-targets -- -D warnings` exits 0 against `main`; CI is green on a fresh PR
- [ ] **All 5 active issues merged** (Phase 1.1, 1.2, 1.3, 1.4, 1.5/#7); no follow-ups deferred (any new discoveries get `parked` label, don't reshape the sprint)
- [ ] **`v0.4.0` tagged** via `miky post-batch` (idempotent post-#176)
- [ ] **Sprint 1 retro's binding lessons applied:**
  - Lesson 1: every `miky batch rebase` call defaults to sequential (no `--merge parallel`)
  - Lesson 2: pre-push gates are now CI-enforced (Phase 1.2 makes them real, not theatre)
  - Lesson 3: audit-driven planning — this Sprint 2 plan IS the canonical example for the cross-project meta-skill (per Miky's reply on 2026-05-20)
- [ ] **Audit refreshed** after Sprint 2 close (per #180 — staleness warning fires if not re-run)
- [ ] **The 3 owed bug filings** from Sprint 1 retro land on `mikbry/miky`:
  - `miky pr diff` subcommand missing
  - `.claude-audit-instructions.md` not in default gitignore
  - Rebase prompt assumes CI exists
- [ ] **`docs/downstream-consumers.md`** lands (per Miky's reply: canonical example for `project-self-description.skill.md`); don't slim it
- [ ] **`docs/CONTRIBUTING.md`** lands (referenced by audit-staleness warning; closes the dangling reference)
- [ ] **Sprint 2 retro filed** per the per-sprint discipline; includes:
  - Did the 3-round Codex/operator-attention budget hold for Phase 1.1?
  - Did CI catch any regressions that operator-side triage would have caught in Sprint 1 style?
  - Did the README rewrite drift-vs-reality risk materialize?
  - Did the calibration table (mkui = 3-4 issues/batch) feel right at 5? Bump down for Sprint 3?

---

## Post-Sprint-2 setup for Sprint 3

The retro at Sprint 2 close will revisit:

### Promote Sprint 1 + Sprint 2 retro's deferred items
- **`#[forbid(unsafe_code)]`** rollout — Phase 2.4 in the audit roadmap; small, Sprint 3 candidate.
- **`thiserror::Error` migration** for `MkuiError` — Phase 2.3; medium, depends on the bridge `mkui/src/lib.rs` error conversion paths.
- **`#[non_exhaustive]`** on the growing enum surface — Phase 2.5; mechanical, Sprint 3 candidate.

### Sprint 3 candidate themes
- **Phase 2 of the audit** — issues #2 (full wgpu renderer), #9 (native boundary decision), #5 (PyO3 fix). These are the "renderer reality" issues I'd originally pitched for Sprint 2 before the audit landed; the audit's sequencing pushed them to Sprint 3.
- **#2 is the headline** — novel-surface tier, 4-round Codex budget, dependency on Sprint 2's CI being green (so the wgpu work has a clean gate).
- **#9 follows #2** — once the wgpu renderer is real, the native-boundary decision (fold mkui-native into mkui-wgpu, or vice versa) becomes obvious.

### Continued substrate work
- **Phase 3 of the audit** — `deny.toml` + `cargo audit`, per-item rustdoc, ADRs, criterion benches. Mostly Sprint 4 work.
- **Miky catalog readiness** — once Sprint 3's renderer lands, Sprint 4 starts shipping the components Miky's `DESIGN.md` lists (Chip, Dot, StatePill, ...). Atomic + cheap once the renderer paints pixels.

The general shape of Sprint 3 is "the renderer becomes real." Sprint 2's job is to make Sprint 3 boringly executable: CI green, clippy clean, MSRV declared, README accurate, FFI sound.

---

## Carry-forward from Sprint 1

`mkui-batch-2-backend-contract` named issues #3/#4/#6/#10 — all merged 2026-05-20. No epic-split carry-forward; each closes its own scope. The carry-overs to flag for Sprint 2:

1. **`docs/audit-report.md`** committed in Sprint 1 close-out batch (so #180's staleness warning treats Sprint 1 as fresh). The Sprint 2 audit refresh is **not** required mid-sprint; only at sprint close.
2. **`docs/CONTRIBUTING.md`** committed in Sprint 1 close-out batch — referenced by `miky post-batch` but didn't exist; this Sprint 2 plan inherits the document.
3. **`docs/sprint-1-retro.md`** committed in Sprint 1 close-out batch as the source-of-truth for what binding behavior change is now in effect.

The Sprint 1 close-out commit lands `audit + retro + this plan + downstream-consumers + CONTRIBUTING` as one cohesive `docs:` commit on `main`. Per `mikbry/agent-skills#2` (Miky's reply 2026-05-20): committing audit + retro + Sprint 2 plan **is** Sprint 1's formal close, satisfying all 8 Sprint-Done gate items per `docs/CONTRIBUTING.md` (post-#216) §"Sprint and batch discipline."

---

## Discipline notes

This is the **third sprint plan written to the depth of `mikbry/miky-internal/docs/sprint-7-plan.md`** in a non-miky project (after `mikbry/marabot/.../sprint-2-plan.md`). Both prior plans surfaced cross-project gaps that landed in `mikbry/agent-skills#2`:

- **Marabot Sprint 2 plan:** sprint-plan-artifact gap — CONTRIBUTING described the invariants but didn't list this artifact as a required deliverable at sprint-creation time
- **mkui Sprint 2 plan (this doc):** audit-driven-planning gap — `miky project audit` should run *before* the first batch, not just before sprint close

This plan's existence is itself signal: the cross-project meta-skill (`ground-in-current-state-before-planning.skill.md`) integrates both gaps into one principle, and this plan is the third canonical example demonstrating the skill's portability.

**What's different from Sprint 1's planning:**

1. **Audit is the substrate.** Sprint 1 was planned from operator vision ("renderer reality"); Sprint 2 is planned from the audit's Phase 1 task table. The audit knew about `mkui-c` unsoundness in 8 minutes; operator vision would have missed it.
2. **Goal sentence states the exit criterion in measurable terms.** Sprint 1's implicit goal was "lock the contract"; Sprint 2's explicit goal includes the literal cargo invocation that exits 0.
3. **Conflict surface analysis happens BEFORE batch creation.** Sprint 1 hit a #3 vs #4 collision on `mkui-console/src/high_level.rs` mid-flight; Sprint 2 verifies file independence at plan time.
4. **Sequential rebase is bound, not optional.** Sprint 1 retro Lesson 1 (memory `feedback_miky_rebase_sequential.md`) is now the default operator behavior; `--merge parallel` requires explicit user opt-in.
5. **CI absence is named as the primary friction.** Sprint 1 ran without CI and absorbed ~3 hours of manual gate triage. Sprint 2's Phase 1.2 fixes this in the second PR of the batch; remaining 3 PRs are CI-gated.
6. **Calibration table acknowledged.** Five issues exceeds mkui's calibrated 3-4/batch; the plan documents why (Phase 1 has exactly 5 file-independent tasks) and the Sprint 3 retro question explicitly asks if the bump felt right.

---

**Sprint 2 starts here.**

`v0.3.0` shipped; the backend contract is locked; the audit identifies the operational hygiene gap as the single biggest blocker to "external consumer is the smoke" experiments. Sprint 2's job: close that gap. Then Sprint 3 can be the renderer.
