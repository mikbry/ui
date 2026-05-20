# Downstream consumers of `mkui`

> **This document exists because a cold orchestrator planning the next sprint cannot reach into sibling repos.** `mkui` is internal substrate — the projects that consume it determine which gaps matter, which components must ship next, and which "renderer realities" must paint pixels first. Without this document, sprint planning depends on operator side-channel knowledge of `mikbry/stonesketch`, `mikbry/miky-internal`, and other consumer repos. That dependency surfaced as the load-bearing project-self-description gap in mkui's Sprint 1 retro (2026-05-20) and is the canonical example for `project-self-description.skill.md` on `mikbry/agent-skills#2`.
>
> **Maintenance contract:** every active consumer below has a status section. When a consumer's needs change (new components required, renderer becomes blocking, deprecation pending), update that section in the same sprint. The doc isn't optional context — it's the *shape* of mkui's roadmap.

---

## The framing

`mkui` is a Rust UI framework with five backends:

- `mkui-core` — domain-free component contract (View / Text / Button / Toggle / ThemeSelector + headless logic)
- `mkui-wgpu` — scene primitives + declarative builders for GPU surfaces (no real GPU pipeline yet — Phase 2.8 of `docs/audit-report.md`)
- `mkui-web` — Web/WASM DOM renderer
- `mkui-console` — Terminal/TUI renderer using `crossterm`
- `mkui-native` — Placeholder; boundary vs `mkui-wgpu` undecided (issue #9)
- `mkui-c` / `mkui-py` — FFI shells around `mkui-core`

`mkui` is **not** a general-purpose UI framework competing with GPUI, egui, or iced. It is the *shared substrate* for the family of projects below. Decisions in `mkui` should be made by reading what these consumers need, not by reading the UI-framework landscape.

When a sprint planner asks "what should mkui ship next?", the answer is determined by:
1. **What downstream consumers are currently blocked on.** (Section "Active consumers" below.)
2. **What downstream consumers will be blocked on in the next sprint.** (Section "Roadmap dependencies" below.)
3. **What the audit (`docs/audit-report.md`) says is broken** — operational substrate that consumers need but can't articulate.

The intersection of 1, 2, and 3 is the sprint backlog. Operator vision unconstrained by 1 or 2 produces sprints that ship "renderer reality" before consumers can use a renderer (Sprint 1's first plan misfire, corrected by Sprint 1 retro Lesson 3).

---

## Active consumers

### 1. `mikbry/stonesketch` (concepting editor — top-priority consumer)

- **Repo:** `/Users/mik/dev/astoneer/stonesketch` (local checkout — open-source pending)
- **Status:** Production-adjacent. StoneSketch's own `crates/gui` crate currently holds the GUI primitives that `mkui-wgpu` is upstreaming (per PR #12 / issue #8). The convergence path is documented in `astoneer/stonesketch/docs/gui.md`.
- **What StoneSketch needs from mkui:**
  - **Component catalog:** `card`, `button`, `slider`, `chip_group`, `scrollbar`, `ScrollArea`, `heading`, `text`, `panel`, `titled_panel` — **shipped in `mkui-wgpu` (PR #12, 2026-05-20)** ✓
  - **Theme system:** `HudTheme`, `ButtonVariant`/`Size`/`State`, `TextVariant`, `ThemeTokens` (cva-style resolver) — **shipped** ✓
  - **Declarative row layer:** `UiBuilder<T>`, `NumberRow`, `ListRow` — **shipped** ✓
  - **`ScrollArea` + selection-outline primitives** — **shipped** ✓
  - **Real GPU pipeline:** `mkui-wgpu` currently has scene primitives and a tessellation pass; StoneSketch's `apps/native` and `apps/web` shells need an actual `wgpu::Device` + `wgpu::Queue` + render pipeline. **Blocked on Phase 2.8 of the audit roadmap (issue #2).** Sprint 3 candidate.
  - **Font + atlas pipeline:** StoneSketch's `crates/font` crate parses TrueType + emits SDF/MSDF atlases. Long-term, mkui should own this; today, StoneSketch keeps it. **Not on mkui's roadmap before Sprint 5.**
  - **Cross-platform shell:** native (winit/wgpu) + web (canvas/WebGPU). mkui-wgpu's `prelude` is shaped to match; the convergence is "one-adapter" once #2 lands.
- **What StoneSketch consumes today:** Nothing yet — `stonesketch-gui` is the still-canonical implementation. After #2 ships, StoneSketch will either (a) replace `stonesketch-gui` with `mkui-wgpu` directly, or (b) keep `stonesketch-gui` as a thin re-export adapter. Decision deferred per `gui.md` step 5 of the convergence path.
- **Sprint 1 impact:** None directly — StoneSketch is downstream of `mkui-wgpu`'s primitives, not its renderer, and the primitives already shipped.
- **Sprint 2 impact:** None directly — Sprint 2 is Phase 1 (operational hygiene); StoneSketch's adoption waits on Phase 2's renderer.
- **Sprint 3 expectation:** `mkui-wgpu` becomes a real wgpu backend (Phase 2.8 / issue #2). At sprint close, StoneSketch's `crates/view` should be able to depend on `mkui-wgpu` and remove `stonesketch-gui`.

### 2. `mikbry/miky-internal` Operator Console (Miky native macOS app — second-priority consumer)

- **Repo:** `/Users/mik/dev/miky-internal` (private — references public `miky` CLI)
- **Status:** Design phase. The Operator Console is a future Mac-native app (per `docs/OPERATOR_CONSOLE_DESIGN.md` and `DESIGN.md`) that will aggregate state across VS Code, Codex, Xcode, GitHub, iTerm, and Claude Code sessions. **Not yet implemented.** Design fully specified.
- **What the Operator Console needs from mkui:**
  - **Component vocabulary explicitly named in `DESIGN.md`:** *"Stable component names that the miklabs/ui (mkui) Rust+WGPU implementation must expose by these names."* The catalog is:
    - **Atoms:** Chip, Dot, StatePill, RoleBadge, TierTag, LedgerChip, Kbd, Avatar
    - **Inputs:** Button (6 variants — primary, attn, default, ghost, sm-default, sm-ghost), SearchInput, SegmentedControl, FilterChip, Tabs (pane + sub)
    - **Structural:** Window, Titlebar, Sidebar (accordion tree), SidebarFoot popover, NeedsYouRail, StatusBar, ModeSwitcher, RunControl, SessionButton, ResizeHandle
    - **Data rows:** PRRow, IssueRow, AgentCard, NeedsYouItem, AuditRow, SprintCard
    - **Composites:** ClientBanner, Advisory, BatchHeader, GatesStrip
  - **Design tokens:** density (compact/comfortable/spacious), light+dark `ThemeTokens`, 5 motion primitives (`pulse`, `pulse-urgent`, `spin`, `banner-slide-in/out`, `resize-handle`), `tabular-nums`, dual-family text (SF Pro + JetBrains Mono).
  - **Real GPU renderer** (same dependency as StoneSketch).
- **What the Operator Console consumes today:** Nothing — the app doesn't exist yet. The design doc treats mkui as the substrate to be built.
- **Sprint 1 impact:** None directly — the catalog hasn't started shipping.
- **Sprint 2 impact:** None directly — Phase 1 hygiene work is invisible to a not-yet-implemented consumer.
- **Sprint 3 expectation:** None — Sprint 3 closes the renderer, but the catalog doesn't ship until renderer is real.
- **Sprint 4+ expectation:** The catalog starts shipping component-by-component, ~5-10 components per sprint. The full catalog above is ~25-30 components; at the calibrated 3-4 issues/batch (per `mkui` calibration), the catalog represents **~6-8 sprints of work**.

### 3. `mikbry/mik.video` (video editor — third-priority consumer, deprioritized)

- **Repo:** `/Users/mik/dev/mikbry/mik.video` (status unverified at the time of this writing)
- **Status:** Per `astoneer/stonesketch/docs/gui.md` line 28: *"miklabs/ui and mik.video now use [the layered architecture]."* StoneSketch positions `mik.video` as the proof-of-concept that the `mkui-core → mkui-web → mkui-wgpu → app` split works — *"web tree on `mkui-core` / `mkui-web`, scene mockup on `mkui-wgpu`, app-specific behavior above both."*
- **What `mik.video` consumes from mkui:** Same `mkui-core` contract as the others; some `mkui-web` use; some `mkui-wgpu` use for scene-side rendering.
- **Sprint impact:** No sprint-level dependency known. If `mik.video` blocks on a feature, the issue will be filed against `mikbry/ui` directly with the `mik.video` consumer named.

---

## Roadmap dependencies (forward-looking)

These are the sprints where consumer needs and mkui capabilities synchronize:

| Sprint | mkui delivers | Consumer can adopt |
|---|---|---|
| Sprint 1 (closed 2026-05-20) | Backend contract + `mkui-wgpu` primitives | StoneSketch can read `mkui-wgpu`'s API surface but cannot render yet |
| Sprint 2 (this) | Operational hygiene (CI, clippy, MSRV) | StoneSketch confirms `mkui-wgpu` is build-stable on its platforms |
| Sprint 3 (next) | Real wgpu renderer (issue #2); native boundary (issue #9) | **StoneSketch starts replacing `stonesketch-gui` with `mkui-wgpu`** |
| Sprint 4 | Miky catalog Phase 1 — Atoms (Chip, Dot, StatePill, RoleBadge, TierTag, Kbd, Avatar) | **Operator Console starts implementing the atom-level scaffolding** |
| Sprint 5 | Miky catalog Phase 2 — Inputs (Button variants 6, SearchInput, SegmentedControl, FilterChip, Tabs) | Operator Console can render basic forms |
| Sprint 6 | Miky catalog Phase 3 — Structural (Window, Titlebar, Sidebar, NeedsYouRail, StatusBar, ModeSwitcher, RunControl, ResizeHandle) | Operator Console renders the full window chrome |
| Sprint 7 | Miky catalog Phase 4 — Data rows (PRRow, IssueRow, AgentCard, NeedsYouItem, AuditRow, SprintCard) | Operator Console renders live data |
| Sprint 8 | Miky catalog Phase 5 — Composites (ClientBanner, Advisory, BatchHeader, GatesStrip) | Operator Console reaches v0 feature-completeness |

**Caveats:**
- This roadmap is the **planning artifact**, not a commitment. Audit-driven planning per Sprint 1 retro Lesson 3 means each sprint re-runs the audit and re-derives its issue set. If Phase 2.8 (real wgpu renderer) turns out to take 2 sprints instead of 1, the catalog shifts by 1 sprint.
- "Sprint N delivers" assumes the prior sprint's success criteria were met. If Sprint 2 doesn't close clippy red on `main`, Sprint 3 doesn't start.
- The catalog phasing is *suggested* by `OPERATOR_CONSOLE_DESIGN.md`'s grouping (Atoms → Inputs → Structural → Data rows → Composites). Phases can reorder if specific Operator Console panels become higher priority — but the dependency direction (atoms before composites that use them) is fixed.

---

## What this document is NOT

- **Not a complete dependency graph.** Examples like `mkui-c` consumers (external C/C++ users) aren't tracked here yet because there are no real consumers of those bindings. When that changes, add a section.
- **Not a marketing document.** Don't optimize the prose for a public-README tone. The audience is *another agent or operator* planning the next sprint, not a downstream user evaluating mkui.
- **Not a substitute for `docs/audit-report.md`.** The audit tells you what's broken; this doc tells you who's blocked. Both feed sprint planning; neither replaces the other.
- **Not a substitute for the consumer repos' own docs.** This doc points at `astoneer/stonesketch/docs/gui.md`, `miky-internal/DESIGN.md`, etc. — it does not duplicate them. When a consumer's needs change, this doc updates the *summary*; the consumer repo's docs hold the authoritative specification.

---

## Update cadence

- **Every sprint retro must check this doc.** If a consumer's status changed during the sprint (new component shipped, dependency unblocked, deprecation), the retro updates the relevant section in the same commit.
- **Every sprint plan must reference this doc.** The "headline issues" come from consumer needs (this doc) intersected with audit findings (`docs/audit-report.md`) intersected with operator priorities. If the plan doc names an issue that doesn't trace back to a consumer or an audit finding, that's a planning smell.
- **A new consumer triggers an immediate update**, not a sprint-boundary update. When a sibling repo first depends on `mkui-core` (or any backend), add a new section in the same PR that introduces the dependency.

---

**Last updated:** 2026-05-20 (Sprint 1 close-out batch — created as the canonical example for `project-self-description.skill.md` per `mikbry/agent-skills#2`).
