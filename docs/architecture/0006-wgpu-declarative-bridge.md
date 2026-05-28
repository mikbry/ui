# ADR 0006 — `mkui-wgpu` declarative bridge over `mkui-runtime::AppTree`

## Status

Accepted (Sprint 5, 2026-05-27).

## Context

After Sprint 4 ([ADR 0005](0005-mkui-runtime-portable-substrate.md))
shipped `mkui-runtime` — the arena-backed `AppTree` substrate every
binding builds into — five of the six backends consumed it directly:
`mkui-web` walked it into the DOM, `mkui-console` flattened it for the
terminal, `mkui-c` / `mkui-py` built into it, `mkui-native` re-exported it.

`mkui-wgpu` was the odd one out. It still exposed only the pre-substrate
HUD primitive API — `Mkui::with_scene(scene).run()` — inherited from the
2D HUD pipeline port ([ADR 0004](0004-mkui-wgpu-2d-hud-pipeline-port.md)).
A wgpu user could not write the same declarative
`Mkui::new()?.child(View::new().child(...))` they wrote against the web
and console backends. `examples/showcase-common::create_showcase_ui` —
the cross-binding canonical demo — only ran on web + console.

The closed-not-merged PR #50 attempted this bridge pre-substrate; the
design surfaced architectural gaps that Sprint 4 closed. This sprint
finishes the job against the substrate that now exists.

## Decision

Implement the wgpu declarative bridge as a thin layer on top of the
existing tessellation pipeline.

### Component placement

- **`WgpuRenderable` trait + `WgpuRendererRegistry`** live in
  `mkui-wgpu`, **not** `mkui-runtime` or `mkui-core`. This mirrors
  [`mkui-web::WebRendererRegistry`](../../crates/mkui-web/src/render.rs)
  and matches the Codex round-7 Q1 ratification (renderer traits are
  backend-local; the substrate stays renderer-agnostic).
- **Walker** (`crates/mkui-wgpu/src/walker.rs`) — consumes
  `mkui_runtime::AppTree` and emits scene primitives via the existing
  tessellation pipeline. No new GPU pipeline state is introduced; the
  pipeline boundary documented in ADR 0004 (`Scene` → HUD pass) holds.
- **Input router** (`crates/mkui-wgpu/src/input.rs`) — cursor tracked
  from `WindowEvent::CursorMoved`, hit-test in reverse paint order
  against the per-frame `Vec<HitTestEntry>` collected during the walk,
  click semantics on release. Logical/physical DPI conversion uses the
  window's `scale_factor`.
- **Built-in registrations** at `WgpuRendererRegistry::with_defaults()`
  ship `BadgeRenderer` + `DotRenderer` so the scene-primitive atoms
  ported in ADR 0004 stay accessible through the AppTree path. The
  built-in node kinds (`View` / `Text` / `Button`) render through fixed
  paths in the walker.

### Eager rebuild on dirty signal

Frames are rebuilt eagerly when the runtime's dirty flag is set. There
is **no** incremental tree diffing in v0.6.0 — diffing without a
performance baseline is premature optimisation (Codex round 7). When an
action fires, it sets `RuntimeCtx::mark_dirty`; the wgpu input handler
observes the flag and calls `window.request_redraw()` from inside the
event loop — **never** from inside the action closure (Sprint 4
anti-pattern guard).

### Closure capture

Hit entries carry cloned `Option<ActionId>` handles, not references
into the tree. Actions fire through `tree.actions().fire(id)` only
after the per-frame hit-test borrow has ended — collect first, invoke
after. This is the Sprint 4 retro's load-bearing rule applied forward;
violating it would re-introduce the "closure holding `RefCell` borrow
across rebuild" failure mode the substrate explicitly closed.

### `Mkui::with_scene` as the retained low-level escape hatch

`Mkui::with_scene(scene).run()` is **retained as the low-level escape
hatch**, not deprecated. The declarative `Mkui::new()?.child(...).run()`
API is the public identity end users build their apps against; the
wgpu-specific `Scene` constructor coexists for renderer tests, custom
HUDs, headless tessellation demos, and future direct-GPU experiments
(Slug glyph rendering, mkui-vector2d primitives — Sprint 7+). Both
surfaces are documented public API.

This matches the Codex round-10 Q5 ratification and Sprint 5
acceptance criterion #14. The alternative — deprecating `with_scene`
in v0.6.0 — would force consumers building renderer tests or custom
HUDs onto a workaround path before the cross-binding declarative API
covers their use case. The declarative API is the *recommended* surface
for app code; it is not the *only* surface.

The wgpu backend's `Scene` API is intentionally an
implementation-of-the-HUD-pipeline surface, not a cross-binding
contract. That is the source of the asymmetry vs. web/console (which
have no equivalent low-level surface): the HUD pipeline (ADR 0004) is
the load-bearing wgpu-specific layer, and giving it a public
constructor is the right way to expose it without re-exporting the
tessellator's internals.

### Layout v1

The walker implements a deliberately small layout: top-down vertical
flow with class-driven padding + gap + text/button sizing. This is the
minimum that renders `examples/showcase-common::create_showcase_ui`
recognisably end-to-end. Full flex / grid layout is deferred (Sprint
7+); the eager-rebuild model lets a richer layout drop in without
re-architecting the walker.

## Consequences

- `examples/showcase-common::create_showcase_ui` now renders on the
  wgpu backend through the same declarative API the web and console
  backends use. `examples/showcase-common/src/lib.rs` is byte-unchanged
  (Codex round-7 Q6 ratification).
- `examples/atoms-on-wgpu` is re-introduced — the 12-badge grid + dot
  showcase + title text from the v0.5.x closed-not-merged #48 demo,
  now built through `tree.push_custom("badge", …)` /
  `tree.push_custom("dot", …)`. The renderers ship in
  `WgpuRendererRegistry::with_defaults()` so users get them without
  registration.
- `examples/native-showcase` is added — the cross-binding canonical
  showcase running on wgpu via `mkui::run!(create_showcase_ui, wgpu)`.
- `mkui-wgpu::Mkui::with_scene` is **retained** as the documented
  low-level escape hatch. Recommended app code uses the declarative
  `Mkui::new()?.child(...).run()` API; renderer tests, custom HUDs,
  and direct-GPU work continue to use `with_scene`. Both surfaces
  coexist as documented public API.
- The HUD tessellation pipeline (ADR 0004) is preserved verbatim. The
  bridge layers **above** it — walker → `Scene` → tessellator → HUD
  pass — so any future tessellation work (Slug-style outline text per
  ADR 0002) drops in without disturbing the bridge.
- `mkui-wgpu/Cargo.toml` gains a `mkui-runtime` dependency (Sprint 4
  deliberately deferred this edge — the wgpu backend was the only
  binding that hadn't taken the substrate).

## Alternatives considered

- **Put `WgpuRenderable` in `mkui-runtime`.** Rejected. The runtime is
  renderer-agnostic by design (Codex round-7 §8); pulling in a wgpu
  trait would force `mkui-runtime` to either depend on `wgpu` (a
  150-crate transitive tree, deeply backend-specific) or to expose
  trait-object boilerplate that the wgpu side then re-wraps. Backend-
  local placement matches the web pattern that already works.
- **Incremental tree diffing.** Rejected for v0.6.0. The substrate is
  too new — there are no criterion benches yet, no live-node counter,
  no production scenes large enough to know whether diffing pays for
  its complexity. Eager rebuild on dirty signal is the documented
  Codex round-7 baseline; diffing becomes Sprint 8+ work after the
  audit Phase 4 performance bench scaffolding lands.
- **Match the web backend's `thread_local!` active-tree pointer.**
  Rejected. The web backend's `thread_local!` exists because
  `wasm_bindgen` closures must own their captures (`'static`) and
  cannot borrow `&AppTree` into each onclick. The wgpu input handler
  has direct access to the app's `Rc<RefCell<AppTree>>` through the
  event loop, so the global pointer would be pure ceremony — and a
  re-entry hazard. Codex round 10 Q3 anti-pattern-list flagged this
  ahead of time.
- **Deprecate `Mkui::with_scene` in v0.6.0.** Rejected per Codex
  round-10 Q5. Both shapes ARE equally supported, just for different
  use cases: declarative for app code, `with_scene` for
  direct-to-renderer paths (renderer tests, custom HUDs, headless
  tessellation, future Slug / mkui-vector2d integration). Deprecating
  the low-level surface before its successor exists would force
  consumers onto a workaround path for a use case the declarative API
  is not designed to cover. The two surfaces target different layers
  of the stack and coexist by design.

## Out of scope (reserved for future sprints)

- **Shared `mkui-layout` crate / module.** This sprint ships a
  minimal **wgpu-local** layout pass inside the walker (Codex
  round-10 Q3 Option A): top-down vertical flow with class-driven
  padding + gap + text/button sizing, just enough to render
  `examples/showcase-common::create_showcase_ui` recognisably on
  wgpu. A future sprint may extract layout to a `mkui-layout` shared
  module/crate consumed by `mkui-runtime` + every backend (Codex
  round-10 Q3 Option D) once web/console/wgpu/native need true
  layout parity. The wgpu-local pass is intentionally scoped to the
  current showcase tokens; the shared layer is the right place for
  general flex / grid semantics, breakpoint resolution, and
  cross-binding layout snapshots.
- **Visual regression / screenshot diff infrastructure.** Sprint 7+.
- **Hover state, responsive breakpoints, CSS transitions.** Sprint 7+
  — depend on either the wgpu-local layout v2 or the shared
  `mkui-layout` extraction, whichever lands first.
- **Incremental tree diffing.** Sprint 8+, after criterion benches
  exist (see "Alternatives considered" above for rationale).
