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

### Component status (2026-11-20)

| Component | `mkui-core`/`mkui-runtime` | `mkui-wgpu` | `mkui-web` | `mkui-console` | Status |
|---|---|---|---|---|---|
| Button | `NodeKind::Button` (+`disabled`, #245) | yes | yes | yes | **Done** |
| Card | `NodeKind::Card` (#270) | yes | yes | yes (border rule) | **Done** (chrome only — see #195 for image/grid/states) |
| Badge, Dot, Tooltip, Toast | — | `mkui-wgpu`-only (badge/dot/tooltip reachable via `NodeKind::Custom`, wgpu-registered only) | no | no | Not promoted |
| Dialog, Slider, Toggle, Segmented select, Chip group, Progress bar, Scroll area, Text field, Panels | — | `mkui-wgpu`-only | no | no | Not promoted |

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
