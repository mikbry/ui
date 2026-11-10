# ADR 0009 — Track the host's `wgpu` release; no skew window

**Status:** Accepted (2026-11-10, `mikbry/ui#174`, parent tracker #173)

## Context

The root `Cargo.toml`'s `[workspace.dependencies]` pinned `wgpu = "29.0.1"`.
Cargo's default caret requirement on that string resolves to
`>=29.0.1, <30.0.0`. A real embedding host building against `mkui-wgpu`
needed `wgpu` 30.0.1, whose own caret requirement (`>=30.0.1, <31.0.0`) does
not overlap `29.0.1`'s at all. A workspace depending on both `mkui-wgpu` and
such a host would therefore resolve **two** separate `wgpu` releases into its
dependency graph — confirmed by `cargo tree` before this change.

That duplication is a real blocker, not a cosmetic one. `wgpu`'s public types
— `Device`, `Queue`, `Surface`, `Texture`, and friends — are opaque handles
tied to a specific `wgpu` crate version. Two incompatible copies resolved into
one binary means a host cannot hand `mkui-wgpu` a `wgpu::Device` it created
itself, or receive a `wgpu::Texture` back from it: the types simply don't
match, a hard compile error. Any future API that lets a host and `mkui-wgpu`
share a `wgpu::Device`, `Surface`, or other resource needs both sides to
resolve to the exact same `wgpu` crate first — version unification is the
precondition, enforced by Cargo's resolver only when both sides' version
requirements overlap.

`wgpu` does not promise its public API stays source-compatible from one
release to the next — each top-level version bump (29 → 30, as this change's
own six call-site fixes in `render/mod.rs` and `render/offscreen.rs`
confirm: `RequestAdapterOptions`, `SurfaceConfiguration`, `Queue::present`,
`RenderPipelineDescriptor::vertex.buffers`, and `BufferSlice::get_mapped_range`
all changed shape) can carry breaking changes. Cargo's default caret
requirement already treats each such bump as SemVer-incompatible, which is
the correct, conservative default — the risk this ADR guards against is
someone *widening* that requirement to paper over the gap.

## Decision

**Track the host ecosystem's current `wgpu` release with Cargo's ordinary
caret requirement, and bump it promptly when a host needs the next one —
never widen it into a multi-release range.** `mkui-wgpu` and
`mkui-vector2d-wgpu` depend on `wgpu` via the workspace-level `wgpu = { workspace = true }`,
and the single source of truth is the `[workspace.dependencies]` entry in the
root `Cargo.toml`. This issue bumps that entry from `"29.0.1"` (`>=29.0.1, <30.0.0`)
to `"30.0.1"` (`>=30.0.1, <31.0.0`).

No skew window is offered: `mkui-wgpu` does not attempt to compile against a
range spanning two `wgpu` releases, and today exposes no API that accepts a
host-supplied `wgpu::Device`, `Queue`, `Surface`, or `Instance` at all — the
only resource accessors in the crate are `OffscreenRenderer::device()` /
`::queue()` getters for a device `mkui-wgpu` itself created
(`crates/mkui-wgpu/src/render/offscreen.rs`). Building a constructor that lets
a host inject its own `wgpu` resources is a separate, not-yet-built feature,
tracked under parent issue #173 — this ADR only settles what `mkui-wgpu`'s
own `wgpu` version promise is, so that future feature has a stable version
story to build on. What a host author can rely on today:

- **Guaranteed:** a host whose own `wgpu` requirement overlaps `mkui-wgpu`'s
  current workspace pin (today, anything Cargo resolves within
  `>=30.0.1, <31.0.0`) and that also depends on `mkui-wgpu` resolves a single
  `wgpu` in its dependency graph — `cargo tree -i wgpu` shows one entry, not
  two. That is the precondition any future resource-sharing API would need;
  it does not by itself hand the host a way to pass `wgpu` types across the
  `mkui-wgpu` boundary, since no such API exists yet.
- **Not guaranteed:** a host requiring a `wgpu` release outside that range
  (29.x or earlier, 31.x or later) resolves two separate `wgpu` copies and
  cannot unify with `mkui-wgpu` at all. Such a host must wait for
  `mkui-wgpu`'s pin to move (a reviewed dependency-bump PR, same shape as this
  one) or vendor/patch its own `wgpu` requirement to match `mkui-wgpu`'s
  current one.

A `wgpu` bump in `mkui-wgpu` is treated like any other breaking dependency
bump in a pre-1.0 crate (see `CHANGELOG.md`'s stated policy): it lands on a
`mkui` minor version bump, documented in the `CHANGELOG.md` "Changed"
section, and is expected to require fixing whatever call sites the new
`wgpu` release broke — this change's `render/mod.rs` and
`render/offscreen.rs` edits are the template for what that fix looks like.

## Consequences

- Host authors embedding `mkui-wgpu` must track the `[workspace.dependencies]`
  `wgpu` entry in this repo's root `Cargo.toml` and keep their own `wgpu`
  requirement overlapping it. There is no compatibility shim to fall back on
  if they lag behind or get ahead of it.
- Every `wgpu` bump in `mkui-wgpu` is a potential unification break for any
  downstream host, even though it may land on a `mkui` minor version bump
  rather than a major one (pre-1.0 policy). A `mkui` release whose CHANGELOG
  "Changed" section names a `wgpu` bump is the signal for host authors to
  check their own requirement.
- `mkui-wgpu` keeps a single, unambiguous `wgpu` version to test, document,
  and reason about — no conditional compilation or shim layer per supported
  `wgpu` release, which would multiply the surface of every future
  `wgpu`-facing change.
- A workspace that needs both `mkui-wgpu` and a host on a different `wgpu`
  release, with no intent to share GPU resources between them, is unaffected
  by this policy: Cargo will resolve two `wgpu` versions without complaint as
  long as neither side's code needs to pass `wgpu` types across the boundary.
  This policy — and this issue's first acceptance criterion — only cover the
  unification case: a workspace where both sides need one shared `wgpu`.
- This ADR does not add a host-resource-injection API to `mkui-wgpu`. A host
  still cannot hand `mkui-wgpu` its own `wgpu::Device` today; this change only
  removes the version mismatch that would make such an API impossible to add
  later. That API remains future work under #173.

## Alternatives considered

**A permissive version range on the workspace `wgpu` requirement (e.g.
`">=29, <31"`), so Cargo can resolve to whichever release the host already
has.** Rejected: `wgpu` does not keep its public API source-compatible across
releases (confirmed again by this change's six API-break fixes), so a range
wide enough to span two releases would compile against only whichever end
Cargo's resolver happens to pick, and silently fail to compile — or worse,
compile against the wrong API shape if a transitive dependency forces a
pick — for a host expecting the other end. That failure would surface as a
surprise on an unrelated dependency bump rather than an explicit, reviewed
`mkui-wgpu` pin change, which is strictly worse than today's narrow-but-honest
caret requirement.

**A `wgpu`-version-agnostic shim crate between `mkui-wgpu` and the host**,
re-exporting a backend-neutral handle type `mkui-wgpu` converts to/from
whatever `wgpu` release it is pinned to. Rejected as disproportionate to the
problem: it would require `mkui-wgpu` to maintain conversion shims for every
`wgpu` type a host might want to share (device, queue, surface, textures,
buffers), in perpetuity, for a benefit — simultaneous multi-release support —
that no host in this project's ecosystem has asked for, and that still
requires the resource-injection API (#173) to exist before it would do
anything. The simpler fix (keep `mkui-wgpu`'s pin current, bump it when a host
needs the next release) costs one reviewed PR per `wgpu` release instead of an
ongoing shim layer.

**No code change; document a skew policy only (point hosts at "wait for the
next bump" without touching the workspace `wgpu` pin).** Rejected: this
issue's first acceptance criterion requires a workspace depending on both
`mkui-wgpu` and a host pinned to `wgpu` 30.0.1 to build with no duplicate
`wgpu` in `cargo tree`. A doc-only change cannot satisfy that — `29.0.1`'s
caret requirement never overlaps `30.0.1`'s regardless of what the docs say.
The pin had to move; this ADR exists to make the resulting policy (track
promptly, never widen the range) explicit for the *next* bump, so the same
question is not re-litigated at `wgpu` 31.
