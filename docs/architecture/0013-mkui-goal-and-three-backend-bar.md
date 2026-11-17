# ADR 0013 — mkui's goal and the shared-contract/three-backend bar

## Status

Accepted (2026-11-16).

## Context

#173 (opened 2026-11-06) scoped the active roadmap around one embedding
host's full game-UI needs (menus, HUD, world-anchored elements, dialogs,
settings) without reconciling that scope against the README's long-standing
shadcn/ui framing, and it became the de facto roadmap. A 2026-11-16 goal and
scope review found: `mkui-core` still has three node kinds (`View`, `Text`,
`Button`); the shared layout layer hasn't started (the wgpu walker is a
vertical stack); the style-class parser panics on any class outside 39
hardcoded tokens, including the README's own first example; and roughly
twenty components exist only as `mkui-wgpu` drawing functions, absent from
the shared contract, web, or console.

## Decision

mkui's goal is the shadcn/ui component catalogue, in Rust, on one shared
contract, rendered by every back end. A component is "done" only when it
exists in `mkui-core`'s contract **and** all three back ends
(`mkui-wgpu`, `mkui-web`, `mkui-console`) render it — not any two.

Host-shaped work splits three ways:

- **mkui** — the shared contract and its primitives (layout, styling,
  focus, layering, text, state, and generic mechanisms like a per-node
  transform or an anchored-placement helper).
- **`mkui-hud`** — what a second, different embedding host could reuse
  unchanged (a host-embedding example, its profiling harness, a reusable
  anchoring helper).
- **The host itself** — whatever carries that specific host's own look or
  meaning (a game-specific indicator's styling, a dialogue bubble's skin,
  a game's specific counters). This does not live in this repository at
  all, not even in `mkui-hud`.

Every from-scratch foundation (layout, text, styling, and the rest) follows
the same engineering discipline:

- Name correctness tolerances and gaps explicitly; no hidden fudge factors.
- Fix the mechanism a failing case exposes, not the individual case.
- Small-code bias; no caching or optimization ahead of a measured cost.
- Design docs are dated and falsifiable — state what's true as of when,
  and how to check it.

### The three-question test

For any proposed feature, in order:

1. Is it in the shadcn/ui catalogue, or a primitive those components need
   (layout, styling, focus, layering, text, state)?
2. Can it be expressed in the shared contract and rendered by every back end?
3. Does it carry meaning from a specific host's domain (a heading, a world
   position, a game state)?

**Yes, yes, no** → it belongs in mkui.
**Yes on question 3** → it belongs in the host, or in `mkui-hud` if a
second host could reuse it unchanged — never in `mkui-core` or a backend
crate.

## Consequences

- New issues against `mkui-core`/`mkui-wgpu`/`mkui-web`/`mkui-console` are
  checked against the three-question test before being filed.
- `mkui-hud` is a new, explicitly second-class crate: useful to
  embedding-game hosts, never a dependency of `mkui-core` or any backend,
  and not held to the three-backend bar.
- `mkui-web`'s target architecture changes from DOM rendering to a WebGPU
  canvas sharing `mkui-wgpu`'s renderer — a larger change than it looks,
  since the current DOM backend (static, mount-once) is not a smaller
  version of the canvas target, it's a different one.
- Existing issues scoped around one host's specific needs are re-triaged:
  some stay (they serve any application), some move to `mkui-hud`, some
  close outright (they turned out to need no bespoke mkui component at
  all once the underlying generic primitive exists).

## Alternatives considered

- **Leave #173 as the roadmap and let host-shaped work keep going under
  `mkui-core`/`mkui-wgpu`.** Rejected — this is the status quo that
  produced the imbalance: the GPU renderer and text stack are the large
  majority of the workspace's code, while the shared contract itself still
  has three node kinds.
- **Move all host-specific work out of this repository, with nothing in
  between.** Rejected — a separate `mkui-hud` crate keeps genuinely
  reusable host-adjacent work (an embedding example, a profiling harness,
  a positioning helper) discoverable and reusable without re-litigating,
  per issue, whether it's "host enough" to live elsewhere entirely.
- **Define "done" as any two back ends rendering a component.** Rejected
  — `mkui-web`'s planned architecture (a WebGPU canvas sharing
  `mkui-wgpu`'s renderer) means two GPU-rendered call sites could satisfy
  "two back ends" without ever exercising `mkui-console`'s very different
  rendering model. All three is the bar that actually proves
  cross-backend portability.
