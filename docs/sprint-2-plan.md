# Sprint 2 Plan — "Renderer foundations + first Miky atoms"

**Window estimate:** 2026-05-21 → 2026-05-24 (4-day sprint; reshape vs. original plan documented in §"Discipline notes")
**Tag target:** `v0.4.0`
**Batches:** 2 — `mkui-batch-3-hygiene` (days 1-2) + `mkui-batch-4-renderer-and-atoms` (days 2-4, overlapping start)
**Capacity:** 6 issues across 2 batches (3 + 3) — file-independence verified in §"Conflict surface analysis"

---

## Sprint goal (immutable)

> **Paint real pixels on a wgpu surface with production-quality text, expose the first three Miky-aligned atoms, and gate everything behind CI.** Sprint 2 lands the load-bearing renderer + text plumbing the rest of the catalog will ride on, plus the minimum hygiene (CI + `mkui-core` clippy/API fixes) that makes downstream review honest. The audit's Phase 1 hygiene items not directly on the renderer's critical path (mkui-c safety, MSRV, README) are deferred to Sprint 4 with explicit rationale below.

Three threads:

1. **Renderer reality** — port `stonesketch-render`'s production HUD pipeline (surface management, MSAA picker, accumulator) into `mkui-wgpu`. Drop the 3D-specific passes (scene, shadow, AO, selection outline) that StoneSketch owns; keep the 2D HUD path. Replace the existing bitmap glyph fallback in `mkui-wgpu/src/tessellation.rs` with real text via a new `mkui-text` crate (cosmic-text + glyphon-shaped atlas, behind a `PlatformTextSystem` trait — see [`docs/research/mkui-text-state-of-the-art.md`](research/mkui-text-state-of-the-art.md)).
2. **First Miky atoms in shadcn alignment** — ship `Badge` (covering Chip/RoleBadge/TierTag/LedgerChip as variants), `Dot` (Miky-specific status circle), and `StatePill` (Miky-specific 9-state agent encoding). All three are listed as required in `mikbry/miky-internal/DESIGN.md` lines 522-531; the shadcn↔Miky alignment is documented in [`docs/components/miky-to-shadcn-mapping.md`](components/miky-to-shadcn-mapping.md). Where shadcn has an equivalent (Badge → shadcn Badge), the shadcn API + variant naming is canonical.
3. **CI as ground truth + mkui-core API correctness** — Sprint 1 retro's Lesson 2 (CI absence = pre-push gates are unenforceable theatre) closes in this sprint. CI gates fmt/clippy/test on every push. The 8 pre-existing `mkui-core` clippy errors (Default impls + std-trait shadowing) are real downstream-breaking API bugs and must land before any atom code rides on top — both because the renderer/atoms work consumes `mkui-core` types and because catalog work shouldn't carry the broken-`main` triage tax described in retro Lesson 2.

---

## Sprint 2 issue set

| # | Title | Effort | Tier | Role |
|---|---|---|---|---|
| **Batch 3 (hygiene)** | | | | |
| new | `feat(common): add .github/workflows/ci.yml (fmt + clippy + test gates)` | small | template | server-side ground truth; gates everything after it lands |
| new | `fix(core): mkui-core std-trait shadowing + Default impls + StyleClass::add rename` | medium | substrate | resolves 8 clippy errors + the std-trait ambiguity traps; workspace-wide call-site rename for `add` is in-scope |
| **Batch 4 (renderer + atoms)** | | | | |
| new | `feat(wgpu): port stonesketch-render's HUD pipeline into mkui-wgpu` | medium-large | substrate | wgpu::Surface, MSAA picker, swapchain config, accumulator, instanced sprite pipeline — extraction not greenfield (see Risk 1) |
| new | `feat(text): mkui-text crate wrapping cosmic-text + glyphon-shaped atlas behind PlatformTextSystem trait` | medium-large | substrate | per `docs/research/mkui-text-state-of-the-art.md`. New crate. Codex-reviewable after the research doc + this PR. |
| new | `feat(wgpu): first Miky atoms — Badge (with Chip/RoleBadge/TierTag aliases) + Dot + StatePill` | medium | template | per `docs/components/miky-to-shadcn-mapping.md`. Ships shadcn-aligned API where applicable. |
| **Defaults — assumed in plan but not new issues** | | | | |
| (carry) | `mkui-wgpu` consumers receive the new renderer through existing `Mkui`/`WgpuApp` types in `mkui-wgpu/src/high_level.rs` — no new issue, scope rider on the renderer PR |

Six new issues filed before batch creation (5 above + 1 winit-integration if it doesn't fold into the renderer PR — decided at issue-filing time). The mkui calibration (3-4 per batch) is satisfied: 3 in each batch.

---

## Deferred from Sprint 2 → Sprint 4

These items appear in the audit roadmap or in #7 but are pushed to Sprint 4 to make room for renderer + atom work that unblocks Miky's app:

| Audit phase | Item | Why deferred | Sprint target |
|---|---|---|---|
| 1.1 | mkui-c FFI safety (11 `unsafe extern` declarations) | Not on Miky's path (Miky's app is Rust + macOS, not C/C++). CI lands in Batch 3 anyway, making the errors visible. | Sprint 4 |
| 1.4 | Declare `rust-version = "1.74"` + workspace `lints` table | Trivial; no functional impact; fits cleanly with #7 README work. | Sprint 4 |
| 1.5 / #7 | README rewrite to reflect v0.3.0 + mkui-wgpu + Python broken-on-3.14 disclaimer | Non-blocking; better written after Sprint 2 ships actual capability the README can describe accurately. | Sprint 4 |

These deferrals **explicitly violate audit-strict sequencing** ("never feature on top of red clippy"). Justification: Miky's app deadline (~3 weeks to usable mkui) doesn't fit a hygiene-only Sprint 2. The audit's #1 finding (mkui-c soundness) stays unfixed for ~2 weeks longer. CI lands in Sprint 2 anyway, making the errors *visible* even when not yet fixed. This trade-off is logged here for Codex / future-retro review.

---

## Conflict surface analysis

### 1. `.github/workflows/ci.yml`
Touched by Batch 3's CI issue only. Zero conflict.

### 2. `crates/mkui-core/src/{style.rs, theme.rs, headless/{button,text,toggle}.rs}`
Touched by Batch 3's mkui-core clippy fixes only. The `StyleClass::add` rename is workspace-wide (mkui-web, mkui-console, mkui-wgpu, examples/ all call it) — **all rename call-sites land in the same PR** per Sprint 7 broader-scope rule.

### 3. `crates/mkui-wgpu/src/{renderer.rs, app.rs, high_level.rs, lib.rs, Cargo.toml}`
Touched by Batch 4's renderer PR — the largest surface in the sprint. Adds new dependencies (`wgpu`, `winit` on native target, `bytemuck`, `pollster` — see `stonesketch-render/Cargo.toml` for reference). Updates `mkui-wgpu/src/tessellation.rs` to delegate text glyph triangles to `mkui-text`.

### 4. `crates/mkui-text/` (new directory)
Touched only by Batch 4's text PR. Zero conflict with other PRs (new crate). Adds `cosmic-text`, `swash`, `etagere`, `wgpu` deps. Workspace `Cargo.toml` adds `crates/mkui-text` to members.

### 5. `crates/mkui-wgpu/src/components.rs` + new `badge.rs` / `dot.rs` / `state_pill.rs`
Touched by Batch 4's atoms PR. **Depends on mkui-core's `StyleClass::add` rename landing first** — if atoms reference `StyleClass`, they must use the renamed API. Sequencing handled by merge order, not file-independence: Batch 3's mkui-core fix merges before Batch 4's atoms.

### 6. `Cargo.toml` (workspace package)
Touched by Batch 4's renderer PR (adds wgpu/winit deps) and Batch 4's text PR (adds cosmic-text/etagere/mkui-text crate to workspace members). Disjoint sections of the file. Sequential merge resolves any line-overlap trivially.

### Low-conflict / additive issues
- All Batch 4 PRs touch new files (`mkui-text/`, atoms files) or distinct existing files. The renderer PR is the only one rewriting existing files (`mkui-wgpu/src/renderer.rs` + tessellation.rs).
- Batch 3 PRs are file-independent from Batch 4 PRs. Batch 3 lands first; Batch 4 starts with `--no-launch` and rebases against post-Batch-3 main.

---

## Batch composition

### Batch 3 — `mkui-batch-3-hygiene` (2 agents, days 1-2)

| # | Issue | Why this batch |
|---|---|---|
| ci | `feat(common): add .github/workflows/ci.yml` | unblocks every later PR; gates the renderer work |
| core | `fix(core): mkui-core std-trait shadowing + Default impls + StyleClass::add rename` | atoms can't ship on top of `mkui-core` with broken std-trait shadowing |

**Why these two together:**
- File-independent (different roots).
- Both gate Batch 4: renderer needs CI to enforce gates honestly; atoms need `mkui-core` clean to consume types.
- 2 agents in this batch + 3 in Batch 4 = 5 total active agents over the sprint, well within calibration.

**Tier projection:** template (1) + substrate (2) → ~3 rounds operator review attention.

### Batch 4 — `mkui-batch-4-renderer-and-atoms` (3 agents, days 2-4)

| # | Issue | Why this batch |
|---|---|---|
| render | `feat(wgpu): port stonesketch-render's HUD pipeline into mkui-wgpu` | headline; load-bearing for everything after |
| text | `feat(text): mkui-text crate wrapping cosmic-text + atlas behind PlatformTextSystem trait` | text rendering — mandatory for any Miky-grade UI |
| atoms | `feat(wgpu): first Miky atoms — Badge + Dot + StatePill` | first deliverable Miky's app team can render |

**Why these three together:**
- All three are downstream of Batch 3's CI + mkui-core fixes.
- Atoms have a soft dependency on text (Badges contain labels) and a hard dependency on render (something has to paint them). But all three can develop in parallel — atoms can test against the existing bitmap text path while the text PR is in flight, then rebase onto cosmic-text.
- 3 agents matches batch cap and Sprint 7 retro's "novel-surface 4-round" guidance (render + text are both substrate, atoms are template).

**Tier projection:** substrate (4 rounds × 2) + template (2 rounds × 1) → ~10 rounds operator review attention. Sprint 7 retro flagged that 3 substrate PRs in one batch is the upper bound for operator attention; we're at 2 + 1, which fits.

---

## Sequencing within batches

### Batch 3 (days 1-2)
1. **CI lands first** (~half-day). YAML file copy from `mikbry/miky/.github/workflows/ci.yml` adapted for mkui's `cargo test --workspace --exclude mkui-py` invocation. Once merged, all later PRs are CI-gated.
2. **mkui-core fixes land second** (~1 day). Workspace-wide `StyleClass::add` rename happens in this PR. CI catches any missed call-sites.

### Batch 4 (days 2-4)
Atoms agent + render agent + text agent all launch on day 2 against post-Batch-3 main. Three parallel iTerm sessions.

1. **Text lands first** (day 3) — once cosmic-text wiring is verified, render's text-path can swap from bitmap to atlas.
2. **Render lands second** (day 3-4) — biggest PR, includes the winit `ApplicationHandler` shell so downstream apps (Miky's app) can consume it.
3. **Atoms land last** (day 4) — atoms rebase onto the merged render+text, swap their text emission from the bitmap fallback to the real atlas path.

Merge order: text → render → atoms. Sequential rebases per Sprint 1 retro Lesson 1.

---

## Risks (from the planning view)

### Risk 1 — Renderer PR is "porting" but the ported code is 2 854 lines

The render PR's framing is "extraction, not greenfield" — `stonesketch-render` is a working production wgpu renderer. But the actual code to extract is non-trivial: 788 lines of ambient occlusion (drop), 800 lines of selection outline (drop), 2054 lines of `lib.rs` mixing the HUD pipeline with the 3D scene pipeline. The HUD-extraction needs careful surgery so the 3D-specific structures don't leak into mkui-wgpu's interface.

**Mitigation:** the renderer issue body must explicitly list "drop these passes: scene_pass, shadow_pass, ambient_occlusion_pass, selection_outline_pass — keep: surface management, MSAA picker, HUD pipeline, accumulator." Cite the relevant `stonesketch-render` line ranges. The agent reads `stonesketch-render/src/lib.rs` first and produces a "kept vs dropped" diff against it before writing any mkui-wgpu code.

### Risk 2 — cosmic-text is ~40 transitive deps and ~2 MB binary delta

Per the research doc, cosmic-text pulls swash + rustybuzz + fontdb + unicode-bidi + unicode-script. Total transitive count is significant. mkui-core stays clean (zero text deps), but `mkui-text` becomes the heaviest crate in the workspace.

**Mitigation:** put cosmic-text in a separate `mkui-text` crate (already planned). Make `mkui-wgpu`'s text dependency optional behind a feature (`default = ["text"]`). StoneSketch and other future consumers who want their own font path set `default-features = false` and provide a `PlatformTextSystem` impl. This pattern matches the `glyphon`-style packaging Codex will recognize.

### Risk 3 — Atoms PR has hidden dependency on text rendering for layout/measurement

`Badge` and `StatePill` measure their text label to compute width. If the text PR is in flight, atoms agent measures against the bitmap path which has different metrics than cosmic-text.

**Mitigation:** atoms PR uses `PlatformTextSystem::measure(...)` rather than hard-coding glyph dimensions. The trait shape (per the research doc §5) gives `measure → LayoutRun` cleanly. Atoms work against the trait, not the implementation; the cosmic-text vs bitmap difference is invisible to atom code. Verify this assumption in the atoms issue body — if the trait isn't shaped for measurement, file a sub-issue.

### Risk 4 — Sprint 7 retro's "novel-surface 4-round Codex pattern" likely applies to render + text

Both PRs are genuine novel surface for mkui. The render PR extracts from a working reference but the *integration into mkui's existing app/renderer/high_level shape* is new. The text PR is new wiring entirely. Sprint 7 retro Lesson 2 says novel-surface PRs reliably need 4 rounds.

**Mitigation:** budget 4 rounds explicitly for render + text. The 4-day window assumes this. If either PR hits a 5th round (per Sprint 7 retro's reviewer-loop PR pattern), the sprint extends rather than skipping the round. **Honest budget + adjust mid-flight.**

### Risk 5 — Miky's `DESIGN.md` text requirements (JetBrains Mono + SF Pro + tabular-nums) may surface gaps

cosmic-text supports tabular numerals via OpenType `tnum` feature (passed through to swash). SF Pro and JetBrains Mono are both standard sfnt fonts. But Miky's design specifies very tight glyph metrics that may not match what cosmic-text produces (slightly different hinting vs CoreText).

**Mitigation:** Sprint 2's atoms work uses cosmic-text. If Miky's app smoke-tests reveal pixel-mismatches that matter, Sprint 4 or 5 adds a `CoreTextSystem` impl behind the same trait. The research doc §9 engages this counterargument; it's accepted, not silently absorbed.

### Risk 6 — winit ApplicationHandler shape for the App primitive

`stonesketch/apps/native/main.rs` is 1 653 lines of winit + wgpu glue. Some of that is StoneSketch-specific. The mkui equivalent should be ~300-500 lines (Window-spawning, event loop wiring, surface acquisition, frame callback). If it grows beyond that, the abstraction is wrong.

**Mitigation:** the renderer PR's scope explicitly includes a `mkui_wgpu::App` shell that downstream apps consume. Target: minimal viable winit `ApplicationHandler` that calls into the renderer per frame. If the renderer PR's diff exceeds ~1 500 lines, split the App shell into its own PR (would make Batch 4's count 4, not 3 — still in budget).

---

## Sprint 2 success criteria

- [ ] **Sprint goal met:** a `mkui_wgpu::App` example app launches a real winit window, paints Badge + Dot + StatePill via cosmic-text-rendered glyphs on a real wgpu surface.
- [ ] **All 6 active issues merged** (3 per batch); follow-ups deferred only with explicit `parked` label + Sprint 3 sequencing rationale.
- [ ] **`v0.4.0` tagged** via `miky post-batch`.
- [ ] **Sprint 1 retro's binding lessons applied:**
  - Lesson 1: every `miky batch rebase` defaults to sequential (memory `feedback_miky_rebase_sequential.md` in effect)
  - Lesson 2: CI gates fmt/clippy/test on every PR push (Batch 3 lands this in PR 1)
  - Lesson 3: this Sprint 2 plan's content was derived from the audit's roadmap + Miky's downstream needs + StoneSketch's existing code — not from operator vision
- [ ] **Audit refreshed** at sprint close (per `mikbry/miky#180` staleness warning).
- [ ] **3 cross-project bug filings on `mikbry/miky`** (deferred from Sprint 1's formal close):
  - `miky pr diff` subcommand missing (#237 ✓ filed 2026-05-21)
  - `.claude-audit-instructions.md` not in default gitignore template (#238 ✓ filed 2026-05-21)
  - Rebase prompt assumes CI exists (#239 ✓ filed 2026-05-21)
- [ ] **Codex review on the text decision** — `docs/research/mkui-text-state-of-the-art.md` and `docs/components/miky-to-shadcn-mapping.md` go through external review before the atom PR merges.
- [ ] **Sprint 2 retro filed** with the standard 7-section shape; includes:
  - Did the 4-round substrate budget hold for render + text?
  - Did the cosmic-text choice produce production-quality text on macOS for Miky's design?
  - Did Miky's app team actually consume the Sprint 2 deliverable end-to-end?
  - Did the calibration table (mkui = 3-4 issues/batch, 4-day window) feel right at 3+3 across two batches?

---

## Post-Sprint-2 setup for Sprint 3

The retro at Sprint 2 close will revisit:

### Promote Sprint 1 + Sprint 2 retro's deferred items
- Phase 2.4 (`#[forbid(unsafe_code)]`) — small, Sprint 3 or 4.
- Phase 2.3 (`thiserror::Error` migration for `MkuiError`) — medium, depends on the bridge.

### Sprint 3 candidate themes — "Window chrome + structural"
- **Structural batch:** Window, Titlebar, Sidebar (shadcn-aligned), StatusBar, NeedsYouRail. Five issues, calibrated for the batch cap.
- **Inputs + remaining atoms batch:** Button-variants (extend existing), SearchInput, FilterChip, Tabs, Kbd, Avatar.
- Atoms still owed from the Miky catalog: RoleBadge (Badge variant), TierTag (Badge variant), LedgerChip (Badge with icon), Kbd, Avatar, SegmentedControl (ToggleGroup).
- Per the audit's Phase 2: issue #2 is now (mostly) landed via Sprint 2 — confirm at retro whether the renderer is "complete" or whether residual work belongs in Sprint 3.

### Sprint 4 candidate themes — "Data rows + composites + deferred hygiene"
- Data rows (PRRow, IssueRow, AgentCard, NeedsYouItem, AuditRow, SprintCard) — all compositions, all UiBuilder methods, not new components.
- Composites (ClientBanner, Advisory, BatchHeader, GatesStrip).
- Deferred hygiene catches up: mkui-c safety (audit 1.1), MSRV (audit 1.4), README rewrite (audit 1.5 / #7).

The general shape of Sprint 3 is "window chrome → first usable Miky app." Sprint 4 ships data rows + composites. Per `docs/downstream-consumers.md`, **Miky's app is usable end of Sprint 3 (~14 days from now)** and catalog-complete end of Sprint 4 (~21 days). The 3-week deadline holds.

---

## Discipline notes — what changed from the prior Sprint 2 plan

This is the **second revision** of `docs/sprint-2-plan.md`. The first revision (committed in 53da01a) was audit-strict: 5 PRs of Phase 1 hygiene, no features. That plan ran into a downstream-consumer-deadline constraint surfaced in subsequent operator review:

- Miky's macOS app needs usable mkui within ~3 weeks (3-4 mkui sprints).
- At audit-strict pace, Sprint 2 = hygiene, Sprint 3 = renderer (#2), Sprint 4 = catalog atoms start. Total ~5-6 sprints before Miky's app has usable structural + atoms.
- The deadline doesn't fit.

The reshape:
1. **Phase 1 hygiene trimmed** to CI (1.2) + mkui-core clippy (1.3). mkui-c safety (1.1), MSRV (1.4), README (1.5/#7) deferred to Sprint 4. This is **a deliberate audit-discipline trade-off** documented in §"Deferred from Sprint 2".
2. **Sprint 2 now ships Issue #2 (real wgpu renderer)** — but reframed as extraction from `stonesketch-render`'s existing production code, not greenfield. Substrate tier, not novel-surface.
3. **First-text and first-atoms land in Sprint 2** — cosmic-text via the new `mkui-text` crate (per the dedicated research doc) + shadcn-aligned atoms (per the dedicated mapping doc).
4. **shadcn is the canonical naming + variant target** — where shadcn has an equivalent for a Miky component, the shadcn API wins. The Miky design language can keep its names (Chip, RoleBadge, etc.) but the implementation maps to `Badge` with variants.

This revision is **the second canonical example of the audit-driven planning principle** captured in Sprint 1 retro Lesson 3 (the first canonical example being the v1 plan in 53da01a). The principle generalizes to: **"plans must integrate audit findings + downstream-consumer deadlines + existing-code reality + design-system alignment."** When any of those four inputs changes, the plan revises. They all changed between 2026-05-20 (v1 commit) and 2026-05-21 (this v2 commit):
- Audit findings: same.
- Downstream-consumer deadline: surfaced via operator review (3 weeks to usable mkui for Miky).
- Existing-code reality: surfaced via reading `astoneer/stonesketch` (stonesketch-render is 2 854 lines of production wgpu; mkui doesn't need greenfield rendering).
- Design-system alignment: surfaced via explicit shadcn-naming guidance from operator.

The revision pattern is captured in the close-out commit message and will inform the Sprint 2 retro's "what we learned about planning" section.

---

## Carry-forward from Sprint 1

`mkui-batch-2-backend-contract` named issues #3/#4/#6/#10 — all merged 2026-05-20. No carry-forward. The Sprint-Done gate items for Sprint 1 close were satisfied by commit 53da01a + ef16fdc (audit + retro + sprint-2 plan v1 + corrections); this revision (sprint-2 plan v2) extends rather than replaces the close-out per the audit-driven-planning principle above.

---

**Sprint 2 starts here.** Two batches, six PRs, four days. Miky's app gets first usable mkui at end of Sprint 3 (~14 days from now); catalog-complete end of Sprint 4 (~21 days). The 3-week deadline holds.
