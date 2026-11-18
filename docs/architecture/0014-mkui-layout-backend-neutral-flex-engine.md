# ADR 0014 — `mkui-layout`: a backend-neutral flex layout engine over `AppTree`

## Status

Accepted (2026-11-18, issue #63 — F1 of the mkui goal/course-correction
foundations, 2026-11-16).

## Context

Sprint 5's wgpu bridge (ADR 0006) shipped a deliberately minimal,
wgpu-local layout pass inside `mkui-wgpu`'s `walker.rs`: top-down vertical
flow with class-driven padding, a fixed inter-sibling gap, and
character-count button sizing. That module's own docs always named this as
the Sprint 5 bridge move, with "a future `mkui-layout` shared module" as
the reserved seam — it was never meant to become the permanent
architecture, and it does not implement real flex semantics (no row
direction, no wrap, no grow/shrink, no justify/align).

Per the 2026-11-16 mkui goal/course-correction review, this is F1: the
first of several from-scratch foundations that need to land before the
component catalogue (F4) can keep growing on borrowed time from a
backend-local shortcut. The review also set the engineering discipline
every from-scratch foundation issue (including this one) follows: name
correctness tolerances and gaps explicitly rather than hiding them behind
special cases, fix the mechanism a failing case exposes rather than
patching the individual case, keep a small-code bias, and write design
docs that are dated and falsifiable.

## Decision

Add `mkui-layout`, a new crate between `mkui-runtime` and every backend —
sibling to `mkui-core`, not a replacement for it (same posture ADR 0005
took when it carved `mkui-runtime` out from under `mkui-core`).

- **Input:** `mkui_runtime::AppTree` + the `ResolvedStyle` already attached
  to each node. No new parsing, no new tree type.
- **Leaf sizing is a callback, not a dependency.** `Text`/`Button`/`Custom`
  leaves report their intrinsic size through a caller-supplied
  `LeafMeasure` trait. `mkui-layout` depends only on `mkui-runtime` (plus
  `serde_json`, already transitively required by `NodeKind::Custom`) —
  never on `mkui-text`, `wgpu`, `web-sys`, or `crossterm` as a real
  dependency. This mirrors `mkui-text`'s own "own the stack" posture (ADR
  0002) one layer up: the layout engine owns its algorithm, not a vendored
  or hard-wired text stack.
- **Output:** absolute border-box `Rect`s keyed by `NodeId`, via a small
  `LayoutResult` (a pre-order `Vec` for deterministic iteration plus a
  `HashMap` for `get(id)` lookup — `NodeId` has no `Ord`, so there is no
  sorted view to expose). A hidden subtree, or one rooted at a node
  `LeafMeasure::is_overlay` reports `true` for, gets no entry at all.
- **The flex algorithm is written from scratch** — row/column direction,
  `flex-wrap`, `flex-grow`/`flex-shrink`, `justify-content`/`align-items`,
  `gap`, and a generic min/max clamp mechanism (today exercised only by
  `max-w-4xl`'s 896px clamp). No external layout crate (`taffy`, `stretch`,
  `yoga`, …) — consistent with ADR 0002's precedent for `mkui-text`.
- **Named tolerances, not hidden special cases.** `flex.rs`'s module docs
  enumerate every place this engine's behavior diverges from a full CSS
  flexbox implementation (shrink only applies to `View` children; stretch
  is a no-op on leaves; margins and `mt-auto` only resolve within a
  Column-direction container; `flex-wrap` has no effect in Column
  direction; `container` contributes no width constraint by itself). Each
  is tied to a concrete reason — either CSS itself has no such concept
  (max-content height), or no token in the current showcase set exercises
  the combination — rather than silently special-cased in the code with no
  explanation.
- **`justify-center` joins Tier 1.** The README's own quickstart example
  (`"flex-1 items-center justify-center"`) was a Tier 3 parse error before
  this issue — and worse, silently dropped *every* token on that string
  (including `flex-1` and `items-center`) when reached through the
  `push_view_unchecked` path `mkui-c`/`mkui-py` use, because
  `AppTree::alloc_node` falls back to `ResolvedStyle::default()` on any
  parse error via `unwrap_or_default()`, with no panic. Fixed at the
  source (`mkui-runtime`'s class parser), not papered over in
  `mkui-layout`.
- **The `mkui-wgpu` walker does not call this crate yet.** See
  "Consequences" below and issue #287.

## Consequences

- **A second, correct flex implementation now exists alongside the
  walker's old approximation**, intentionally, for one release cycle.
  `walker.rs`'s layout math stays as-is; its module docs now name
  themselves transitional and point at `mkui-layout` + issue #287 instead
  of "a future shared module" (routine doc update, not a new ADR for that
  file — rewriting `walker.rs` to actually consume `mkui-layout`'s rects
  is real surgery, since its `WalkCache` keys subtree reuse on
  `cursor_y`/`content_x`/`content_width` threaded through a single
  render-and-measure pass, not on an already-computed rect. Swapping in a
  separate layout pass means redesigning that cache, which deserves its
  own reviewed PR rather than riding along inside this one.
  `crates/mkui-wgpu/tests/mkui_layout_interop.rs` proves the two are
  *compatible* (a `mkui-layout` consumer can emit `mkui_wgpu::types::Rect`
  from `mkui-layout`'s output) without making that swap yet.
- **`mkui-console` and `mkui-web` can adopt the same engine** once a
  `LeafMeasure` exists for their own text systems — this crate has no
  wgpu-specific assumption baked in anywhere (verified: `grep -rn "wgpu::\|web_sys::\|crossterm::" crates/mkui-layout/src` is empty).
- **`ResolvedStyle`'s JSON shape gained one field** (`justify_center`).
  `grep -rn "justify_between" crates examples bindings` (run 2026-11-18)
  found no mirror of the struct outside `mkui-runtime::style` itself and no
  checked-in golden snapshot JSON, so this is a size-only change to the
  `snapshot` feature's output — no other file needed updating.
- **Falsifiable claim, dated 2026-11-18:** `grep -n "mkui_layout" crates/mkui-wgpu/src/walker.rs`
  returns nothing below its module doc comment. Re-run that grep to check
  whether issue #287 has landed.

## Alternatives considered

- **Adopt an external layout crate (`taffy`, `stretch`, `yoga-rs`).**
  Rejected — contradicts the explicit 2026-11-16 decision to write this
  in-house, consistent with the "own the stack" posture ADR 0002 already
  established for `mkui-text`. An external crate would also drag in its
  own geometry/measurement types, working against this issue's "no
  renderer/device/surface types" acceptance criterion.
- **Fold the algorithm into `mkui-core`.** Rejected. `mkui-core` is the
  pure-contract crate (ADR 0001) that already owns a thin `layout::Layout`
  description type (direction/justify/align/gap/padding, no algorithm). A
  real flex *algorithm* is substantial logic that deserves its own crate
  boundary and its own test suite, the same reasoning ADR 0005 used to
  carve storage/parsing out of `mkui-core` into `mkui-runtime`.
- **Rewrite `walker.rs` to call `mkui-layout` in this same PR.** Rejected
  for blast-radius reasons: `walker.rs` is ~1700 lines with an existing
  cache (`WalkCache`, #229) whose invalidation key is built from the
  render pass's own cursor state, plus hit-test and primitive emission
  interleaved with the layout math in one recursive walk. Decoupling those
  concerns correctly is a second, independently reviewable change — tracked
  as issue #287 — not something to bundle into the PR that establishes the
  engine itself.
- **Make `mkui-layout` depend on `mkui-text` directly** (e.g. a
  `TextSystem`-shaped trait bound) instead of a crate-local `LeafMeasure`
  callback. Rejected — it would make every `mkui-layout` consumer, even a
  pure unit test, pull in the text-stack crate and its `BitmapTextSystem`/
  `CompositeTextSystem` machinery just to lay out a tree. The callback
  keeps this crate's own test suite dependency-light (a deterministic fake
  measurer, no glyph metrics) while a real backend wires the identical
  `&dyn mkui_text::TextSystem` it already owns into the same seam — proven
  by this crate's own `canonical_showcase_layout_with_real_bitmap_text_system`
  test and by `mkui-wgpu`'s `mkui_layout_interop.rs`.
