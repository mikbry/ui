# Downstream consumers of `mkui`

> **This document exists because a cold orchestrator planning the next sprint cannot reach into sibling repos.** `mkui` is internal substrate — the projects that consume it determine which gaps matter, which components must ship next, and which "renderer realities" must paint pixels first. Without this document, sprint planning depends on operator side-channel knowledge of `mikbry/stonesketch`, `mikbry/miky-internal`, and other consumer repos. That dependency surfaced as the load-bearing project-self-description gap in mkui's Sprint 1 retro (2026-05-20). Per Miky's correction note on 2026-05-21, the gap is **not yet promoted to a separate skill** — `project-self-description.skill.md` would require a third independent project to need the same artifact (Sandi Metz's rule of three). Until then, this file is **the example future projects can copy from**, not a skill's canonical reference.
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

These are the sprints where consumer needs and mkui capabilities synchronize. **Revised 2026-05-21** after Miky's app deadline (~3 weeks to usable mkui) surfaced via operator review and StoneSketch's `crates/render` (2 854 lines of production wgpu) was confirmed available for extraction into `mkui-wgpu`. Pre-revision roadmap had Miky catalog starting Sprint 4; revised roadmap pulls it forward to Sprint 2 by reframing Issue #2 as substrate-tier extraction (not novel-surface greenfield) and shipping first atoms alongside the renderer.

| Sprint | mkui delivers | Consumer can adopt |
|---|---|---|
| Sprint 1 (closed 2026-05-20) | Backend contract + `mkui-wgpu` primitives (component catalog, theme, builders, tessellation — upstreamed from `stonesketch-gui`) | StoneSketch reads the API but doesn't render through mkui yet |
| Sprint 2 (current, 4 days) | **Renderer foundations + first 3 atoms.** Batch 3: CI workflow + `mkui-core` clippy/std-trait fixes. Batch 4: port `stonesketch-render`'s HUD pipeline → `mkui-wgpu`; new `mkui-text` crate (cosmic-text + atlas behind `PlatformTextSystem` trait); shadcn-aligned `Badge` + `Dot` + `StatePill` atoms. | Miky's app team begins integration smoke against a real wgpu surface with real text |
| Sprint 3 (~7 days) | **Window chrome + structural + remaining atoms/inputs.** Window, Titlebar, Sidebar (shadcn-aligned), StatusBar, NeedsYouRail. ModeSwitcher (Tabs variant), SearchInput (Input slot APIs), FilterChip (Toggle composition), Kbd, Avatar, RoleBadge/TierTag/LedgerChip (Badge variants), SegmentedControl (ToggleGroup), Button-variants-6. | **Miky's app reaches "usable mkui" — window chrome + atoms + inputs render end-to-end** |
| Sprint 4 (~7 days) | **Data rows + composites + deferred Sprint 2 hygiene.** PRRow, IssueRow, AgentCard, NeedsYouItem, AuditRow, SprintCard (UiBuilder methods, not new components). ClientBanner, Advisory, BatchHeader, GatesStrip composites. Catch-up: mkui-c FFI safety (audit 1.1), MSRV declaration (audit 1.4), README rewrite (audit 1.5 / #7). | **Miky's app catalog-complete** — full Operator Console v0 surface renders through mkui |
| Sprint 5+ | StoneSketch adoption: `mkui-wgpu` adopted; `stonesketch-gui` retired or thin-re-exports. Per-tier-color theme extension pattern hardened. Phase 2.x audit cleanup (thiserror migration, `#[forbid(unsafe_code)]` rollout, native boundary decision). | StoneSketch finishes migration |
| Sprint 6+ | Platform-native text backends (CoreTextSystem, DirectWriteSystem behind the existing `PlatformTextSystem` trait) if Miky's app smoke surfaces pixel-mismatches that matter. Font/atlas pipeline polish. | App-store-grade text quality if needed |

**Caveats:**
- This roadmap is the **planning artifact**, not a commitment. Audit-driven planning per Sprint 1 retro Lesson 3 means each sprint re-runs the audit and re-derives its issue set.
- "Sprint N delivers" assumes the prior sprint's success criteria were met. If Sprint 2 doesn't close CI green + paint real text, Sprint 3 doesn't start the structural work.
- The Miky catalog count was **~30 components in the pre-revision framing**; per [`docs/components/miky-to-shadcn-mapping.md`](components/miky-to-shadcn-mapping.md) the actual first-class component count is **~17** (8 direct shadcn mappings + 9 Miky-specific) with 16 compositions/variants riding on those primitives. The roadmap above ships first-class components per sprint; compositions land alongside as UiBuilder methods.
- The shadcn alignment is binding: where shadcn has an equivalent for a Miky-named component, mkui's canonical API uses the shadcn name + variants. Miky's design-language names are aliases. See the mapping doc.
- The 3-week deadline depends on Sprint 2's renderer-extraction being substrate-tier (4-round budget) rather than novel-surface (4+ rounds with overrun risk). Per Sprint 7 retro's pattern, novel-surface PRs reliably need 4 rounds; if either render or text PR hits a 5th round, Sprint 2 extends rather than dropping scope.

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

**Last updated:** 2026-05-21 — created as an example future projects can copy from; not promoted as a separate skill until a third independent project surfaces the same need. Roadmap section revised same day to reflect Miky-deadline-driven re-prioritization (catalog work pulled forward from Sprint 4 to Sprint 2 via `stonesketch-render` extraction + `cosmic-text` integration).
