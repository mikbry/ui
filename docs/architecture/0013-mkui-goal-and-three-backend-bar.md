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

## Appendix: the `mkui-hud` boundary test (2026-11-19, #290)

`crates/hud` was scaffolded (#290) as an empty workspace member before
any code moved into it. This section states the concrete test for what
belongs there, since "what a second, different embedding host could reuse
unchanged" (above) is not yet sharp enough to settle real cases on sight.

**The test, in order:**

0. **Does it extend a type `mkui-core`/`mkui-wgpu` already owns** (a method
   on `EmbeddedRenderer`, a new `Primitive` variant, a pass inside
   `mkui-wgpu`'s own render pipeline)? If yes, it lands in that crate, full
   stop — `mkui-hud` can never be a dependency of `mkui-core` or any backend
   crate (see Consequences), so ownership alone rules it out regardless of
   how reusable the feature sounds.
1. **Does it need `mkui-wgpu`'s host-embedding mode (#173/#175) to mean
   anything?** If it's a general CPU/GPU primitive with no dependency on
   being embedded inside a host's own frame/device, it is not HUD-host
   plumbing at all — question 2 below decides where it actually goes.
2. **Is it a true cross-backend primitive** — something `mkui-core` could
   contract and all three back ends (`mkui-wgpu`, `mkui-web`,
   `mkui-console`) could eventually render, per the three-question test
   above? If yes, it belongs in `mkui-core`/`mkui-wgpu`, not `mkui-hud`,
   even if the only consumer today is a HUD-shaped host.
3. **Would it work unchanged for a second, different embedding host** doing
   its own game-style HUD on top of `mkui-wgpu`'s host-embedding mode — same
   code, no edits, just different content/config? If yes, it belongs in
   `mkui-hud`.
4. **Does it carry a specific host's own look or meaning** (a game's
   specific counter, a dialogue bubble's skin, a particular indicator's
   styling/thresholds)? If yes, it belongs in that host's own repository —
   not `mkui-core`, not `mkui-hud`, not this repository at all.

`mkui-hud` is the answer only when (0) doesn't apply, (1) is yes, (2) is no,
and (3) is yes; (4) is a standing exclusion that overrides all of the above.

**First candidate dispositions**, settling the crate target each should
build against without implementing their scope (none move today):

- **#230** (CPU/GPU/memory measurement harness) — **stays in
  `examples/host-embed-wgpu`, for now.** #263 already shipped the
  generic capture primitives this needs — `EmbeddedRenderer::render_into_timed`,
  `last_frame_ui_vertex_bytes`, `last_frame_slug_bytes` — and per test
  question 0 those land in `mkui-wgpu` itself
  (`crates/wgpu/src/render/embedded.rs`), not `mkui-hud`, since they
  extend a type `mkui-wgpu` already owns. What #230 still needs beyond
  that — the real-hardware runs, the budget check against *this* scene's
  "full HUD at 1080p" numbers, and profiling `Renderer::render`'s
  re-tessellation cost — is example-scene-specific today: the
  `frame_stats` formatting and field set live inline in
  `examples/host-embed-wgpu/src/main.rs`, not as a standalone module. This
  narrows the Decision section's and Consequences' "profiling harness" bullets
  above to: *if* that formatting/aggregation is later factored out of
  `main.rs` into something scene-independent, *that* module is the
  `mkui-hud` candidate — the capture primitives it would wrap already live
  in `mkui-wgpu`, and the real-hardware measurement runs themselves stay an
  example/CI concern regardless.
- **#250** (host-embedded blur support + large-radius perf tier) — **stays
  in `mkui-core`/`mkui-wgpu`.** `BackdropBlur` and `EmbeddedRenderer` are
  both `mkui-wgpu`'s own types (`crates/wgpu/src/render/backdrop.rs`,
  `embedded.rs`); wiring `BackdropBlur` into `EmbeddedRenderer::render_into`
  and adding a downsample/upsample perf tier both extend `mkui-wgpu`'s
  existing render pipeline. Test question 0 resolves this one before
  question 3 (which would otherwise point toward `mkui-hud`, since the
  *behavior* is reusable across embedding hosts) is even reached —
  `mkui-hud` cannot depend on `mkui-wgpu`, so a change to `mkui-wgpu`'s own
  type cannot live there no matter how reusable it sounds.
- **#262** (HUD typeface swap) — **stays in `examples/host-embed-wgpu`.**
  A specific font choice for that example's specific HUD labels carries that
  host's own look (test question 4); a different host embedding mkui would
  pick its own typeface, not inherit this one.

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
