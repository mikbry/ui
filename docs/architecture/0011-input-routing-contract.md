# ADR 0011 — Input routing contract for embedding hosts

## Status

Accepted. Sprint 8, 2026-11 (#176, parent tracker #173).

## Context

Tracker #173 opens `mkui-wgpu` to a third-party `wgpu` host (a game or
interactive engine) that draws mkui inside its own frame instead of through
a window mkui owns. Today `mkui-wgpu` only ships the standalone shell: a
`winit::application::ApplicationHandler` (`WgpuApp`, ADR 0006) that owns the
entire event loop and never has to answer "did I want this event?" because
there is no other consumer of the stream.

An embedding host is different. It owns the `winit` event loop — it must,
since it also drives its own gameplay input (movement, camera look,
hotkeys) — and forwards events into mkui. Per event, it needs a yes/no
answer: did mkui consume this, or is it still the host's to act on? Without
that answer, clicking a HUD button also fires a weapon, and dragging a
slider also spins the camera.

Two sibling issues in the same tracker wave are still in flight and this
contract does not wait on either:

- #175 (host-owned GPU/frame) will give the embedding mode its actual entry
  point — a borrowed-device constructor distinct from `WgpuApp`'s
  surface-owning one. The routing contract doesn't need that seam to exist;
  it only needs a `&winit::WindowEvent` and the current frame's hit-test
  list, both of which already exist today.
- #185 (text field) and #188 (modal dialog) will be the first real
  components to call this contract's focus/modal hooks. Building the
  contract against mock nodes now, rather than waiting for those
  components, means they land against an already-settled seam instead of
  negotiating it under their own PR.

Hit-testing also had a latent correctness gap this contract needed closed
first: `pointer::hit_test` treated every `HitTestEntry` as a plain
rectangle, so a click in the corner of a rounded button — a pixel the
renderer chamfers away from the painted silhouette — still registered as a
hit. An embedding host trusts mkui's "consumed" verdict completely; a
hit-test that disagrees with what's on screen breaks that trust silently.

## Decision

Add `InputRouter` (`mkui-wgpu::input_router`), a standalone routing engine
independent of `WgpuApp`, plus a shared `Routed` vocabulary type in
`mkui_core::input` so other backends can speak the same consumed/ignored
contract when they build their own embedding seam.

1. **`Routed` lives in `mkui-core`.** `Consumed` / `Ignored` is backend-
   agnostic vocabulary, not a `wgpu` concept — any backend's future
   embedding bridge reports the same two-value answer. The concrete
   routing mechanics (hit-testing, capture, focus) stay backend-local,
   per `mkui-core`'s contract-crate charter (ADR 0001).

2. **Pointer capture lasts the whole gesture, not just press/release.**
   `InputRouter::route` consults the existing press-to-arm state machine
   (`pointer::PointerState`) and reports every `CursorMoved` as `Consumed`
   while a node is armed — even once the cursor has left that node's rect.
   A plain "hit-test this one event" router would let a fast drag's
   mid-gesture motion fall through to the host's camera-look the instant
   the cursor crosses the control's edge; capturing for the gesture's
   duration is the only shape that satisfies "dragging a slider never
   reaches the host's camera/look controls."

3. **Escape checks a modal stack before anything else.** `push_modal` /
   `pop_modal` track open dialogs (topmost last); `route` pops the top
   modal and reports `Consumed` before it ever looks at pointer or
   text-focus state. With no modal open, Escape still cancels mkui's own
   in-progress state (an armed drag, a focused text field) so a stray
   release can't resurrect it, but falls through to the host once that's
   done — the host's own pause menu gets Escape when mkui has nothing open.

4. **A focused text field claims every non-Escape key, unconditionally.**
   `focus_text(node)` makes `route` report every other key `Consumed` —
   not just WASD/arrows, every key — while that focus holds. A text field
   that only special-cased gameplay-looking keys would break the moment a
   host rebound a different key to a gameplay action; "a composing field
   owns the keyboard" is the only version of this rule a host can rely on
   without reading mkui's keybind list.

5. **Hit-testing is corner-exact.** `HitTestEntry` now carries the same
   `CornerRadii` the renderer paints with, and `types::rect_contains_rounded`
   clips each corner the same way `tessellation::clamp_corner_radii` does
   before `tessellation::push_rounded_rect_fill` paints it. `pointer::hit_test`
   (used by both the standalone shell and `InputRouter`) was switched to the
   rounded test, so this fix benefits `WgpuApp` too, not just the new router.

`InputRouter` is deliberately not wired into `WgpuApp`'s standalone event
loop. The standalone shell owns its whole window already — it has no host
to report "consumed" to, and its `window_event` handler is tuned against
multiple hard-won resize/redraw regressions (#93, #99, #101) that this
issue has no reason to touch. `InputRouter` is additive: a new, independently
tested entry point ready for #175's eventual host example to call.

## Consequences

- An embedding host has a single call (`InputRouter::route`) that answers
  "consumed or not" for every pointer/keyboard `WindowEvent`, without
  needing to understand mkui's internal node tree.
- `pointer::hit_test`'s rounded-corner fix is shared by the standalone
  shell — any rounded button in an existing `WgpuApp` app now rejects
  clicks in its chamfered corners, a latent bug fixed as a side effect.
- The modal stack and text-focus slot are currently driven by test code
  only; #185 (text field) and #188 (dialog) are expected to call
  `focus_text`/`blur_text` and `push_modal`/`pop_modal` respectively once
  they land. Until then they're inert but load-bearing seams, not dead code
  — the alternative (waiting for #185/#188 first) would have put this
  contract's shape up for renegotiation inside two unrelated PRs.
- `InputRouter` duplicates a small amount of logic `WgpuApp::window_event`
  already has inline (press-to-arm wiring, Escape-cancels-drag). A future
  cleanup could have `WgpuApp` delegate to `InputRouter` internally so
  there is exactly one pointer/keyboard routing implementation; deferred
  here to avoid touching the standalone shell's regression-sensitive event
  loop in the same change that introduces the new contract.
- Non-primary mouse buttons (right/middle click) are not yet part of the
  contract — `route` reports them `Ignored` unconditionally. Scope decision:
  the acceptance criteria only exercise the primary button; a future issue
  can extend `route_mouse_input` once a component needs secondary-click
  behavior.

## Alternatives considered

- **Wire the contract directly into `WgpuApp`'s `ApplicationHandler`
  impl**, giving every `WgpuApp` instance a host-facing "was this
  consumed" return value. Rejected: `WgpuApp::window_event` returns `()`
  (the `ApplicationHandler` trait signature), so this would require either
  a side-channel (a field the host polls after calling `window_event`) or a
  parallel non-`ApplicationHandler` entry point — which is exactly what
  `InputRouter` is, minus the risk of destabilizing the tuned resize/redraw
  state machine already living in that `impl` block.
- **Only consume the exact event that lands on a hit-test, not the whole
  gesture.** Simpler, but fails the slider acceptance criterion outright —
  the moment a drag's cursor crosses the control's edge, the host would
  start seeing `CursorMoved` again mid-drag.
- **Let the host ask mkui "is anything focused/armed right now?" instead
  of mkui returning a per-event verdict.** Pushes the yes/no decision onto
  the host, which would have to re-derive mkui's own capture/focus logic to
  use it correctly. A per-event `Routed` keeps that logic in exactly one
  place.
