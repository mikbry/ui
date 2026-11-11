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

## Addendum: review round 1 — ownership must be decided once, not re-queried

The first review round found the initial implementation decided a key's or
gesture's `Routed` verdict by re-reading *current* router state on every
event, rather than locking it in once at the moment it was actually
decided. That breaks the moment state changes mid-hold:

- **Key ownership changes mid-press.** `route_key` re-checked
  `self.text_focus` on every event. Pressing `W` before a text field
  focuses returned `Ignored`; if the field focused before `W` was
  released, the release re-checked focus and returned `Consumed` — the
  host applied the press itself (it was told `Ignored`) and then never saw
  a matching release telling it to stop. Fixed by `key_ownership: HashMap<PhysicalKey,
  Routed>`: the verdict is decided once on the initial (non-repeat) press
  and replayed verbatim for every repeat and for the eventual release.
- **Escape key-repeat popped the modal stack repeatedly.** `KeyEvent::repeat`
  was discarded, so each OS repeat was treated as a fresh press — popping
  another modal every ~30ms the key stayed held, then disagreeing with the
  original press's verdict once the stack ran out. Fixed by the same
  `key_ownership` cache: a repeat with a tracked entry returns it without
  re-running `decide_key_press`.
- **Cancelling a drag disowned the rest of the physical gesture.**
  `route_cursor_moved`/the release handler read `pointer.armed()` directly;
  Escape and `CursorLeft` clear `armed` to stop the click from firing, which
  also made every subsequent event (including the eventual
  `MouseInput::Released`, still backed by a physically-held button) read as
  `Ignored` — handing the tail of an in-progress UI drag to the host's
  camera/look control. Fixed by `gesture_owned: bool`, tracked independently
  of `armed`: set from the press's hit result, left untouched by
  Escape/`CursorLeft`'s cancel, cleared only by the matching physical
  release (or a focus-loss cancel, next).
- **Window focus loss left a press armed.** `WindowEvent::Focused(false)`
  wasn't handled at all, so a press that armed a control, followed by an
  alt-tab away and an eventual release the window may or may not ever
  see, left the stale arm (and `gesture_owned`) in place for a later,
  unrelated release to wrongly fire. Fixed by routing `Focused(false)`
  through `pointer.cancel()` + clearing `gesture_owned`.

The shared lesson: this router's state (text focus, the modal stack,
`armed`) is mutable *host-visible* state a host can change between a key's
press and its release, or between an arm and the gesture's physical end.
Any verdict derived from "current state" rather than "state at the moment
the gesture/key began" is a latent inconsistency. `key_ownership` and
`gesture_owned` are the two places this contract now locks a verdict in at
the start of a gesture and replays it, rather than re-deriving it.

## Addendum: #188 — the dialog component lands, backdrop click + Tab trap

[`crate::components::dialog`] (backdrop + panel chrome) is the modal
stack's first real caller, closing the loop this ADR opened. Landing it
surfaced two gaps the original contract (modal stack + Escape precedence
only) didn't cover — a backdrop click and the keyboard focus trap — both
closed inside `InputRouter` rather than pushed onto callers:

- **`push_modal` now carries the dialog's panel rect and its explicit
  `owned` node set.** The original `modal_stack: Vec<NodeId>` had no
  geometry — enough for Escape, which doesn't care *where* the dialog is,
  but not enough to tell a backdrop click from a click on the dialog's
  own interior, or to know which controls are actually this dialog's.
  `ModalEntry { node, panel_rect, owned }` fixes both; `push_modal`'s
  signature changed to require them rather than adding separate setter
  calls, since every real dialog has a panel rect and a (possibly empty)
  control set the moment it opens.
- **Pointer arming and the Tab focus trap scope themselves to the
  topmost modal's `owned` set — identity, not geometry.** A first cut
  derived "belongs to this dialog" from whether a `HitTestEntry`'s rect
  (its center, for Tab) fell inside `panel_rect`. Review caught that this
  is wrong: a host-supplied hit-test list carries no guarantee that every
  entry geometrically inside the panel is actually this dialog's, nor
  that every entry outside it isn't. A lower stacked dialog's
  (now-covered) button, or a host control the caller forgot to omit once
  a modal opened, can sit at any rect — including one that happens to
  overlap the topmost dialog's panel — and geometric containment would
  silently let it fire its action or join the focus trap. `route`'s
  pointer-press handling now hit-tests only the subset of `hit_entries`
  whose `NodeId` is in the topmost modal's `owned` list before arming;
  `decide_tab_press` filters the same way. A miss against that scoped
  set still falls back to `panel_rect` for the backdrop/interior
  distinction — that part *is* inherently geometric (a backdrop click is
  "outside the dialog's painted footprint", which is a rect question),
  it's membership in the dialog that moved to identity.
- **While any modal is open, every primary-button press is `Consumed`,
  by exactly one of three outcomes**: it hit an *owned* interactive
  entry (ordinary click), it missed every owned entry but landed inside
  the panel rect (blank dialog chrome swallows it, nothing closes), or
  it missed every owned entry and landed outside the panel rect
  (backdrop click — pop the topmost modal). The alternative — only
  treating a miss as "backdrop" and otherwise falling through to
  `Ignored` — would let a modal leak clicks on its own padding/labels to
  whatever is behind it, which is the exact bug "nothing behind an open
  modal should react" exists to prevent.
- **The Tab focus trap has no persistent widget registry beyond the
  `owned` list — it derives the focus order from `hit_entries` every
  press**, the same way pointer hit-testing already works against the
  current frame's list rather than a retained tree (consistent with the
  walker's eager-rebuild model, ADR 0006). An owned `NodeId` not present
  in the current frame's `hit_entries` is simply absent from that
  frame's cycle, rather than an error.
- **Tab/Shift+Tab reuse the press/repeat lock-in lesson above.** Shift
  state is latched from `WindowEvent::ModifiersChanged` (winit delivers
  it as a separate event, not inline on `KeyEvent`); the trap only
  advances on a *fresh* Tab press — an OS key-repeat replays the cached
  verdict like every other key, so holding Tab down doesn't race through
  a dialog's controls once per repeat tick. `trap_focus` resets on every
  `push_modal`/`pop_modal`, so a freshly (re)topmost dialog always starts
  its own cycle from the beginning rather than resuming wherever a
  closed or now-covered dialog left off.
- **Gamepad focus traversal is explicitly out of scope.** The acceptance
  criteria mention "Tab/gamepad focus", but no gamepad input exists
  anywhere in this workspace yet (no `gilrs` or equivalent dependency,
  no gamepad `WindowEvent` equivalent) — there is nothing for this
  contract to route today. Tab is the load-bearing keyboard-only
  mechanism; gamepad navigation is deferred to a follow-up issue once a
  gamepad input source exists to drive it.
