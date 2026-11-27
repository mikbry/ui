# ADR 0015 — The component-promotion process, and Card as its second proof

## Status

Accepted (2026-11-20).

## Context

ADR 0013 defined the bar: a component is "done" only when it exists in
`mkui-core`'s shared contract and all three back ends (`mkui-wgpu`,
`mkui-web`, `mkui-console`) render it. At the time it was written, exactly
one component (`Button`) cleared that bar, and roughly twenty others
(card, badge, dot, dialog, toast, tooltip, slider, toggle, segmented
select, chip group, progress bar, scroll area, text field, panels, …)
existed only as `mkui-wgpu`-local drawing functions — callers supply every
rectangle by hand, and `mkui-core`/`mkui-web`/`mkui-console` don't know the
component exists at all.

#270 asked for three things: promote at least one more component beyond
`Button` end to end, fold #245's still-unimplemented `Button` `disabled`
work in as part of that first completed case (not a separate, disconnected
issue), and write down the repeatable process so the next promotion isn't
re-litigated from scratch. This ADR is that process, with `Card` as the
worked example and `Button`+`disabled` as the first completed case it
retroactively documents.

`NodeKind` lives in `mkui-runtime`, not `mkui-core`, as of ADR 0005 (#51) —
ADR 0001's "components live in `mkui-core`" framing is stale on that one
point. `mkui-core::components` is the ergonomic Rust builder layer that
*lowers into* `mkui-runtime::AppTree`/`NodeKind`; the contract itself is
the tree.

## Decision

### The checklist every promotion follows

1. **Pick the representation**: a new `NodeKind` variant plus a props
   struct, or an existing variant's props struct gaining a field. `NodeKind`
   is **not** `#[non_exhaustive]` — adding a variant is a deliberate,
   compiler-enforced breaking change: every exhaustive `match` across the
   workspace stops compiling until every backend has an arm for it. That
   is the feature, not a cost to work around; it is the mechanism that
   proves "all three back ends" rather than letting one silently fall
   through a wildcard arm.
2. **Decide: leaf or container?** A leaf (`Text`, `Button`) is measured by
   `mkui_layout::LeafMeasure` and has no children in practice. A container
   (`View`, now `Card`) participates in `mkui-layout`'s flex algorithm the
   same way `View` does — add it to every `NodeKind::Root | NodeKind::View(_)`
   site in `crates/layout/src/flex.rs` (`is_container`, `natural_width`,
   `layout_subtree`), not to `LeafMeasure`.
3. **Keep props minimal.** `NodeKind` derives `Eq`, so a props struct
   cannot hold a raw `f32`. Prefer an empty/near-empty struct (chrome comes
   from each backend's own theme — see `CardProps`) or small enums/bools
   over floats; route anything that needs fractional precision through
   `StyleClass`/`ResolvedStyle` instead.
4. **Keep `mkui-core`'s builder symmetric** with the existing ones (`View`,
   `Button`): a `class`/`children`/field-setter builder that lowers via
   `LowerToTree` into a `tree.push_*_unchecked` call. Register it in
   `LoweringRegistry::with_defaults`.
5. **Grep `NodeKind::` workspace-wide, tests included** before calling a
   promotion done — the compiler catches every exhaustive `match`
   (including ones inside `tests/` files, which still fail to compile),
   but not a `matches!` guard, a wildcard `_ =>` arm, or a non-exhaustive
   check in a place like `mkui-runtime::snapshot::KindSnapshot`,
   `mkui-console`'s `Selectable`/`Line` dispatch, or a FFI parity-test
   fixture. Every one of those needs its own new arm, not a silent
   fallthrough.
6. **Each backend renders through its own native primitive** — not a
   shared drawing routine ported three times:
   - `mkui-wgpu`: a `walk_node` match arm calling the same
     `components::*` scene function an old class-sniffing escape hatch
     (if one existed) already called. This does not guarantee identical
     pixels to the pre-promotion path — a scene function can itself fork
     on a Cargo feature (e.g. `components::card`'s `slug`-gated vector
     lane vs. a plain faceted `Quad`); the guarantee is architectural
     (wgpu becomes *a* renderer of the contract, not the sole owner of
     the component), not a pixel-diff promise.
   - `mkui-web`: a `render_node` match arm emitting a real DOM element,
     styled via a documented, importable backend recipe with application
     overrides — never inline styles duplicating a Rust theme. Verify computed
     styles in a browser consumer outside the showcase.
   - `mkui-console`: there is no nested-box model — a container's chrome
     becomes a fixed-width flattened `Line` (or pair of `Line`s, like
     `Card`'s top/bottom border rule) rather than a real rectangle.
     "Renders" here means *some* backend-native visual cue exists, not
     pixel parity with the GPU backend.
7. **Prove it through the canonical showcase**, not only unit tests.
   `examples/showcase-common/src/lib.rs` is the one UI definition every
   `*-showcase` example drives through a different backend via `mkui::run!`
   — per this project's own "identity-from-README, not
   identity-from-what-a-backend-currently-exposes" rule, a component that
   only appears in a crate's own test suite hasn't actually cleared the
   three-backend bar in the way a user would exercise it.
8. **Decide what's out of scope explicitly**, and say so in the PR: FFI
   constructors (`mkui-c`/`mkui-py`) for the new node, a vector/anti-
   aliased rendering path, additional visual states. Defer to the
   component's own tracking issue (if one exists) rather than letting the
   promotion PR grow unbounded.

### What stays backend-specific (never promoted)

- Anti-aliasing / vector-path math (`mkui-wgpu`'s `card_vector`, Slug
  Bézier annuli) — a rendering *technique*, not part of the component's
  identity.
- Raw CSS rule bodies and terminal escape-sequence styling — each
  backend's own idiom for applying the same semantic chrome.
- A backend's internal caching/dirty-tracking (`mkui-wgpu`'s `WalkCache`) —
  purely a performance detail of that one walker.

### Card: the second completed case

`NodeKind::Card(CardProps)` — `CardProps` is empty, like `ViewProps`: a
card's fill/border/corner-radius come entirely from each backend's theme
(`WgpuTheme::card()`, the web `.card` CSS rule, the console's box-drawing
border), never from a field on the node. It is a flex container, dispatched
identically to `View` in `mkui-layout`. `mkui-wgpu`'s `render_card` calls
`components::card`, the same theme-driven chrome function `render_view`'s
`bg-card` class escape hatch (#205) painted a plain `Quad` version of by
hand. The two are **not** byte-identical: under this crate's own default
`slug` feature, `components::card` paints the anti-aliased vector-path
lane (#141) rather than a faceted `Quad`, so the canonical showcase's card
now renders with smoother corners on wgpu than the pre-#270 `bg-card` View
did. What's unconditionally new is that `mkui-web` and `mkui-console`,
which never implemented `bg-card` at all, now render a real card too.

Web consumers opt into `crates/web/styles.css` or the identical public
`mkui_web::DEFAULT_COMPONENT_CSS` constant. The recipe supplies default Card
fill, foreground, border, radius, and shadow without installing a global theme.
It reads application-owned CSS variables and uses a low-priority cascade layer
and zero-specificity selectors so consumer classes can override it. The showcase
imports this same recipe; `examples/web-recipe-smoke` independently checks its
computed styles and overrides through the public web entry point. See the
[web recipe documentation](../../crates/web/README.md).

On wgpu, a Card's supported explicit foreground utility (for example,
`text-muted-foreground`) wins over the default `surface_foreground` token.
Descendants inherit that foreground unless they specify their own; leaving the
Card restores the ancestor's foreground for later siblings.

The richer scope #195 asked for (image/texture, multiple visual states,
a responsive grid) is **not** part of this promotion — #195 remains the
tracking issue for that, now with a real cross-backend `Card` node to
build on instead of a wgpu-only chrome helper.

### Text field: the third completed case (#312)

`NodeKind::TextField(TextFieldProps { value, placeholder, caret, anchor, disabled, invalid })` —
promoted from `mkui-wgpu`'s wgpu-local `components::text_field`/`TextEditState`
(#185), following the same checklist `Card` proved. Unlike `Card`, it is a
*mutable* leaf: a backend writes `value`/`caret`/`anchor` back into the
tree on every keystroke via `AppTree::set_text_field_value` (clamped to a
UTF-8 char boundary and line-break-stripped, the same safety
`TextFieldProps::new` applies at construction), rather than only ever
reading declaratively-set props the way `Button`/`Card` do. `caret`/
`anchor` are kept directional (an `anchor`, not a normalized `(start,
end)` selection) so a Shift+Left/Right or a native `<input>`'s
`selectionDirection` round-trips through the tree without losing which
end is the moving one.

Focus is a new concept this promotion introduces: `AppTree` carries a
singleton `focused_text_field: Option<NodeId>` (at most one field focused
at a time, mirroring a browser's own invariant) rather than a per-node
`focused: bool` — cheaper to maintain (no need to scan every other node
to clear its flag) and the natural shape for "exactly one" state.
`mkui-wgpu`'s `WgpuApp` sets it on click (reusing the existing
`HitTestEntry`/`ClickHit` pointer machinery — a text field's hit entry
just carries `action: None`) and clears it on Escape or a click
elsewhere; `mkui-web` sets it from the native `<input>`'s `onfocus`/
`onblur`. `mkui-wgpu`'s walker paints the caret only while the node is
the focused one.

`on_change`/a payload-carrying callback is deliberately absent:
`ActionRegistry` actions are `FnMut(&mut RuntimeCtx)` with no argument
slot for a new value, and the tree itself is the contract — any code
holding the `NodeId` reads the live value back directly. Clipboard
contents and IME composition state stay out of `TextFieldProps` entirely,
per the issue's own framing ("platform concerns each backend implements
against its own primitives, not shared state") — `mkui-wgpu`'s keyboard
routing reuses `TextEditState::from_parts`/`.text()`/`.caret()`/
`.anchor()` each frame as a transient bridge to the pre-existing, already-
tested editing algorithm (typing, Backspace/Delete, Home/End, Shift-
extend-selection, Ctrl+A), so #185's logic is exercised unchanged through
this new entry path — real OS clipboard and `winit::event::Ime` wiring
for `mkui-wgpu`, and caret/selection for `mkui-console` (typed input only,
append/backspace at the end — explicitly allowed as a documented
follow-up by #312's own acceptance criteria), are deferred to #317.

### Button + disabled: the first completed case, folded in

#245 asked for `disabled: bool` on `ButtonProps`/`NodeKind::Button`,
threaded through every binding — this had never actually landed (#245's
own issue body was aspirational; the field did not exist in the tree
until this PR). It is folded in here as the literal first instance of
step 6 above: `mkui-wgpu::walker::render_button` reuses
`components::button`'s existing `DISABLED_ALPHA_FACTOR` dim. The walker retains
one non-activating hit blocker matching the painted silhouette and corner radii:
reverse hit traversal stops there without exposing hover, arming, activation,
or expanded touch padding. Enabled buttons keep their normal hit behavior,
including hover for buttons without actions. The embedding input router consumes
blocked gestures and excludes disabled entries from its modal focus traversal.
`mkui-web` sets the native `disabled` attribute; the public recipe dims it while
keeping `pointer-events: auto`, so an enabled control underneath cannot receive
the click. Application hover effects should use `:enabled:hover`.
`mkui-console` dims the row and refuses to hand back its action from
`Selectable::on_activate`. `focused` is deliberately still absent — #245's
own scope note (a real focus-traversal manager, #203, doesn't exist yet)
still applies.

The original six-argument `mkui_app_button_child` C symbol and enabled semantics
are preserved. `mkui_app_button_child_with_disabled` adds a trailing `int`
(`0` enabled, any nonzero value disabled); the C++ wrapper uses it with
`bool disabled = false`. Rust keeps the original enabled-only `push_button` and
`push_button_unchecked` methods, adding `push_button_with_disabled` and
`push_button_with_disabled_unchecked`. Python retains `disabled=False`.
The disabled flag suppresses UI activation; directly firing a registered action
remains possible. The generated header, both C entry points, binding parity,
and Python default dispatch are covered by tests.

## Consequences

- The next promotion (tooltip, badge, dialog, …) follows this checklist
  instead of re-deriving it. Tooltip in particular is **not** "the easy
  next one" — it already has wgpu-side state (`TooltipState`) and a
  registered `NodeKind::Custom("tooltip")` renderer, but its anchor is an
  absolute pixel rect and it must opt out of flow entirely
  (`LeafMeasure::is_overlay`); promoting it needs an anchor-as-`NodeId`
  redesign first, which this ADR deliberately does not attempt.
- `NodeKind` growing a new variant is a one-time, compiler-enforced
  migration cost across every backend + FFI crate + parity-test fixture —
  accepted deliberately (point 1 above) rather than routed around via
  `NodeKind::Custom`, which would hide the new component from
  `mkui-layout`'s container handling and from the byte-identical snapshot
  parity gate's typed shape.
- `mkui-c`/`mkui-py` do not gain a `push_card`/`card_child` FFI
  constructor in this PR — a Rust-only consumer can build a `Card`, but a
  C/Python/C++ host cannot yet. Tracked as explicit out-of-scope (point 8),
  not a silent gap.
- ADR 0001's "`mkui-core` owns the component types" framing is now doubly
  stale (it already was for `NodeKind`'s crate home per ADR 0005) — this
  ADR does not amend ADR 0001 directly, since that drift is pre-existing
  and out of this issue's scope, but a future reader should treat ADR
  0005 and this ADR as the current statement of where the contract lives.

### Image: the fourth completed case (#196)

`NodeKind::Image(ImageProps)` — unlike `Card`, `Image` carries real
per-instance data (`source`, `fit`, intrinsic `width`/`height`), because
unlike chrome-only surface styling an image's content genuinely varies per
instance. It stays `Eq`-compatible (point 3 of the checklist above) by
construction: `fit` is a small enum, `width`/`height` are `u32`, and
`source` is either an opaque `u64`-wrapping `TextureHandle` or `Vec<u8>` —
no `f32`, no GPU resource type. Corner radius still comes from each
backend's own theme (`WgpuTheme::image()`, the web `.image` CSS rule), the
same "no field on the node" story `Card`'s chrome has.

A leaf, not a container (point 2) — but an unusual one: `Text`/`Button`
need a `LeafMeasure` callback because their content size depends on glyph
metrics this crate doesn't own; `Image`'s intrinsic size is already known
data on the node itself, so `mkui-layout`'s `Image` arm reads it directly.
The one real wrinkle: with no dedicated per-leaf `w-*`/`h-*` style override
in this engine (only `h-*` on a *container*'s own box exists), the only way
to give an `Image` a box whose aspect ratio diverges from its own — the
scenario `ImageFit` exists to handle — is `flex-1` growing its width via
the flex algorithm's ordinary per-child `avail_width` distribution. Without
`flex-1` the `Image` arm ignores `avail_width` entirely and keeps the
intrinsic width — load-bearing under a `Column`'s default stretch
alignment, which hands *every* child the full container width as
`avail_width` regardless of kind (see `layout_column_children`); without
that gate a plain `Image` dropped into an ordinary column would silently
stretch to the column's width. Height stays the image's own intrinsic
height regardless of `flex-1`. This is a narrower mechanism than a real
per-leaf size override would be, deliberately: adding one is a
`mkui-layout` concern bigger than this issue, not specific to `Image`.

Each backend renders through its own native primitive, same as every other
promoted component — but `Image` is also the first one needing a wholly
new GPU pipeline, not a reuse of the existing triangle lane: `mkui-wgpu`
had no texture-sampling infrastructure anywhere before this (text is
bitmap-tessellated or Slug vector, never atlas-sampled). The rounded clip
is a single per-fragment SDF evaluated across the whole textured quad —
no separate corner geometry exists to seam against, a structurally
stronger "no visible seams" guarantee than a tessellated rounded rect's
corner-fan approximation gets. `mkui-web` leans on the browser's own PNG/
WebP decoder (a `data:` URI `<img>`) rather than re-decoding bytes
`mkui-core` already validated once; `mkui-console` is a documented
`[image WxH]` stub, per point 6's "renders" bar (some backend-native visual
cue, not pixel parity).

Decode library choice (PNG via `png`, WebP via `image-webp`, both promoted
to real — non-dev — dependencies of `mkui-core`) and AVIF's exclusion (a
full AV1 decoder is a much bigger dependency than either format here) are
the issue's own decision, not this ADR's; `mkui-wgpu`'s pre-existing `png`
dev-dependency (golden-image test comparison) is unrelated and untouched.

Explicit out-of-scope (point 8), same deferral shape as Card's FFI gap:
`mkui-c`/`mkui-py` gain no `push_image`/`image_child` constructor (#320);
`EmbeddedRenderer::render_into` gains no live `Image` lane (#321 — an
`Image` primitive silently no-ops there today, the same gap `Backdrop`
already has). The *standalone* `Renderer` path is not deferred — it is
fully wired, reachable from a real consumer via
`WgpuApp::renderer`/`register_texture`/`unregister_texture`, and proven
by a GPU acceptance test that drives `build_image_pipeline`/
`ImageTextures` directly against the offscreen harness's device/queue
(the harness itself has no live per-frame command loop to patch, so that
is not a separate gap). JPEG/other formats are a follow-up, tracked
separately from this ADR.

### ScrollArea: the fifth completed case (#328)

`NodeKind::ScrollArea(ScrollAreaProps)` — empty, same reason `CardProps`
is: a backend's scroll offset is transient per-frame view state
(`mkui-wgpu`'s own `ScrollState`, a browser's native `overflow-y: auto`),
not something every binding needs to round-trip through the tree the way
`TextField`'s `value`/`caret` genuinely must.

A container (point 2), same shape as `Card` — no bespoke layout branch
needed. The one real wrinkle (#328's actual bug, predating this
promotion): `mkui-layout`'s `distribute_main_axis` used to shrink a
column's `View`/`Card` children proportionally with no floor below their
own natural content height, so a child could be compressed below what
its own subtree required and that subtree's children would end up
positioned inside the undersized box — visible overlap, not overflow.
Fixed first, as its own standalone commit: a plain `View`/`Card` column
child's shrink floor is now its own natural height (CSS has no
"min-content height" narrower than that), so it never actually shrinks;
any shortfall becomes visible overflow past the container's own
(budget-clamped) box. `ScrollArea` is the one exception — its shrink
floor is `0.0`, same as CSS's `min-height: auto` resolving to zero for a
scroll container, because absorbing overflow is its entire purpose.

Each backend renders through its own native scroll primitive: `mkui-web`
gets a real `overflow-y: auto` box — the browser owns position, wheel/
touch/keyboard input, and the scrollbar itself entirely natively, the
same "lean on the platform" story `TextField`'s `<input>` has.
`mkui-wgpu` paints the existing `components::scroll_area`/`scrollbar`
chrome (#194) — promoted into the shared contract's render path for the
first time; previously this widget had no live consumer anywhere in the
crate at all (dead code reachable only from its own unit tests) — and
wires real mouse-wheel input: `WgpuApp::apply_wheel_to_scroll_area`
hit-tests the latched cursor against the innermost `ScrollArea` under
it and mutates a host-owned `HashMap<NodeId, f32>` (offset is host-side
view state, never tree data, per `NodeKind::ScrollArea`'s own doc
comment — `NodeId`s are stable across frames since the tree persists,
so keying on them directly is safe). Each rebuild,
`walker::apply_scroll_offsets` translates every scrolled `ScrollArea`'s
descendant rects *after* `compute_layout` and *before* any painting —
one `mkui_layout::LayoutResult::translate` call per descendant — so
painted pixels, per-node layouts, and hit-test rects all agree on the
live offset with no separate bookkeeping. `WalkCache` keys on
`(subtree_revision, rect, foreground)`, and a `ScrollArea`'s *own* rect
never moves when its content scrolls, so `render_scroll_area` poisons
its own cache entry (`stats.saw_custom = true`, the same mechanism a
`Custom` node's unknowable external state already uses) — without it, a
second frame at a different offset would hit the stale cached paint
from the first. `mkui-console` renders it transparently, same as a
plain `View` — the terminal has no pixel-level viewport to clip against
and the user's own terminal emulator already provides scrollback, so
unlike `Card`'s chrome (a real visual cue drawable with box-drawing
characters) there is no console-native analogue to fall back to (point
6's "renders" bar is satisfied by "doesn't drop content from the flow",
the same bar `Image`'s `[image WxH]` stub clears).

**`mkui-wgpu` has no clip/scissor on scrolled content.** `Scene` carries
no generic clip-rect primitive today (confirmed:
`grep -rn "clip\|scissor" crates/wgpu/src` turns up only one narrow
`pass.set_scissor_rect` use in the Slug text-rendering fallback lane),
so an overflowing child simply draws past the box's own edge with no
clipping, scrolled or not. This is invisible for the shipped usage — the
whole page wrapped in one root-spanning `ScrollArea` with nothing else
sharing the window, so the window's own render-target bounds are the
only clip that ever mattered — but it is a real, general gap: **any**
`ScrollArea` with neighboring content (a sibling above/below it, not
only one nested inside another scrollable region) will paint its
scrolled-past content directly over that neighbor once scrolled.
Tracked as #336, not assumed safe-by-construction.

Explicit out-of-scope (point 8): drag/keyboard/gamepad input *on the
promoted component* for `mkui-wgpu` — `ScrollState` itself already
supports all three in addition to wheel (#194), but each needs its own
input-routing design (hit-testing the thumb through a drag gesture, a
focus model for keyboard, a gamepad abstraction that doesn't exist yet)
meaningfully bigger than the wheel path this promotion did ship;
tracked as #335. The clip/scissor gap above is #336. Real per-area
console scrolling is #337. Min-content width for the row (width) axis'
own shrink floor (a pre-existing, orthogonal gap this promotion's column-
axis fix did not touch) is #338. `mkui-c`/`mkui-py` constructors are
#339, same shape as every prior promotion's FFI gap.

### Component status (2026-11-26)

| Component | `mkui-core`/`mkui-runtime` | `mkui-wgpu` | `mkui-web` | `mkui-console` | Status |
|---|---|---|---|---|---|
| Button | `NodeKind::Button` (+`disabled`, #245) | yes | yes | yes | **Done** |
| Card | `NodeKind::Card` (#270) | yes | yes | yes (border rule) | **Done** (chrome only — see #195 for image/grid/states) |
| Text field (Input) | `NodeKind::TextField` (+`disabled`/`invalid`, #312) | yes (typing/caret/selection; clipboard/IME wiring deferred — #317) | yes (native `<input>`) | yes (typed input; caret/selection deferred — #317) | **Done** (#185's editing logic unchanged; #317 tracks the deferred platform integrations) |
| Image | `NodeKind::Image` (#196) | yes (new textured-quad pipeline) | yes (`<img>` + placeholder for the texture source) | yes (`[image WxH]` stub) | **Done** (standalone `Renderer` only — `EmbeddedRenderer`/offscreen live lane, FFI constructors deferred) |
| Scroll area | `NodeKind::ScrollArea` (#328) | yes (scrollbar chrome via `components::scroll_area` + live mouse-wheel input; drag/keyboard/gamepad on the promoted component deferred — #335) | yes (native `overflow-y: auto`) | yes (transparent pass-through — no console-native scroll primitive, #337) | **Done** (clip/scissor for non-root-spanning use — #336; FFI constructors — #339) |
| Badge, Dot, Tooltip, Toast | — | `mkui-wgpu`-only (badge/dot/tooltip reachable via `NodeKind::Custom`, wgpu-registered only) | no | no | Not promoted |
| Dialog, Slider, Toggle, Segmented select, Chip group, Progress bar, Panels | — | `mkui-wgpu`-only | no | no | Not promoted |

## Alternatives considered

- **Route every new component through `NodeKind::Custom`** instead of a
  dedicated variant, avoiding the exhaustive-match migration cost
  entirely. Rejected: `Custom`'s `type_name` + opaque JSON `props` is the
  *extension* slot for types `mkui-core` does not and should not know
  about (the `TestWidget`/`menu_list` proof); a shadcn-catalogue component
  that is going into the permanent shared contract belongs in the typed
  shape, where `mkui-layout`'s container handling and the snapshot parity
  gate both see it structurally, not string-keyed.
- **Promote Tooltip instead of Card**, since it already has more
  wgpu-side infrastructure (state machine, a registered `Custom`
  renderer). Rejected for *this* proof: tooltip's absolute-pixel anchor
  and mandatory overlay/flow-exclusion semantics are real design debt
  that would make this promotion about redesigning the anchor
  relationship, not about proving the checklist. Card has zero
  interactive state and no anchor — "well-isolated" per #270's own
  framing — so it proves the pattern without conflating two different
  pieces of work. Tooltip is next, explicitly carrying that extra design
  work, not a surprise discovered mid-promotion.
- **Give `Card` real props (`corner_radius: f32`, a `selected: bool`)**
  now, anticipating #195. Rejected: `NodeKind` derives `Eq`, ruling out a
  raw float outright, and an unused `selected` field with no consumer
  yet would be exactly the kind of speculative surface this project's
  own conventions avoid. #195 adds fields when it has a concrete need for
  them.
