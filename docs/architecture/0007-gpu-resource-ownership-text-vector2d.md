# ADR 0007 — GPU resource ownership for wgpu text and `mkui-vector2d`

## Status

Accepted (Sprint 7, 2026-06-11).

## Context

Sprint 7 introduces one native glyph lane (Slug-style direct-GPU outline
rendering, foreshadowed in [ADR 0002](0002-mkui-text-own-the-stack.md) and
[ADR 0006](0006-wgpu-declarative-bridge.md)) and the first slice of a
backend-neutral 2D vector contract (`mkui-vector2d`). Both land on top of the
existing wgpu surface + HUD pipeline ([ADR 0004](0004-mkui-wgpu-2d-hud-pipeline-port.md)).

Before any of that code is written, mkui needs an explicit, acyclic resource
boundary. The failure mode this ADR forecloses is a cyclic or ambiguous split
where, say, `mkui-text` reaches for a `wgpu::Device` to upload a glyph mask, or
the Slug curve/band encoder ends up owning a `Surface` — coupling the
backend-neutral text/vector contracts to wgpu and making a future backend swap
a rewrite of all four crates instead of two.

Two of the four crates named here do not exist yet (`mkui-vector2d`,
`mkui-vector2d-wgpu`). This ADR defines the ownership and dependency contract
they must be built to satisfy; it deliberately ships **no implementation code**
(issue [#64](https://github.com/mikbry/ui/issues/64) out-of-scope). The
implementation of the crates and pipelines is [#65](https://github.com/mikbry/ui/issues/65)
and later.

## Decision

Split GPU and CPU resource ownership across four crates along a strictly
one-way, acyclic dependency graph. No crate below owns a resource listed in its
"Must not own" column.

### Ownership table

| Crate | Owns | Must not own |
|---|---|---|
| `mkui-text` | font IDs / registry, font bytes / parsing, metrics, preparation / layout, render-class selection, glyph outlines / masks | WGPU buffers, bind groups, shaders, pipelines, device, queue, surface |
| `mkui-vector2d` | backend-neutral paths / curves, Slug curve + band encoding, canonical outline cache keys, retained CPU blobs | WGPU types / resources, surface / frame lifecycle |
| `mkui-vector2d-wgpu` | vector / Slug GPU buffers, bind groups, WGSL, pipelines; initialization from borrowed WGPU context | window / surface ownership, text measurement / font registry |
| `mkui-wgpu` | instance / adapter / device / queue, window surface, offscreen target integration, frame lifecycle, render passes, ordered cross-pipeline command submission | font parsing, Slug curve / band algorithm |

### Dependency direction

The graph is explicit and cycle-free:

```text
mkui-vector2d        -> mkui-text
mkui-vector2d-wgpu   -> mkui-vector2d + wgpu
mkui-wgpu            -> mkui-text + mkui-vector2d + mkui-vector2d-wgpu
```

`mkui-vector2d-wgpu` receives **borrowed** `device` / `queue` /
target-format inputs from `mkui-wgpu` at initialization. It does **not** depend
on `mkui-wgpu` (no back-edge) and does **not** own a `Surface`. This is the
load-bearing acyclicity guarantee: the GPU-resource crate for vector/Slug is a
leaf that `mkui-wgpu` drives, not a peer that reaches back up into surface or
frame lifecycle.

### CPU contract vs. GPU integration

- **`mkui-text`** stays backend-neutral, exactly as [ADR 0002](0002-mkui-text-own-the-stack.md)
  committed. It owns glyph **outlines / masks** — the CPU-side geometry — and
  never touches a `wgpu` type. Uploading a mask to the GPU is the consumer's
  job, not `mkui-text`'s.
- **`mkui-vector2d`** owns the Slug curve + band **algorithm** and the canonical
  outline **cache keys / retained CPU blobs**. It is the backend-neutral
  encoded-data contract: a `wgpu` implementation and a hypothetical future
  backend both consume the same encoded bands and cache keys. It owns no GPU
  type and no surface/frame lifecycle.
- **`mkui-vector2d-wgpu`** is the only place vector/Slug GPU resources live:
  the GPU buffers holding encoded bands, the bind groups, the WGSL, the
  pipelines. It is initialized **from a borrowed wgpu context** and owns no
  window or surface.
- **`mkui-wgpu`** retains everything [ADR 0004](0004-mkui-wgpu-2d-hud-pipeline-port.md)
  and [ADR 0006](0006-wgpu-declarative-bridge.md) gave it: instance / adapter /
  device / queue, the window `Surface`, offscreen target integration, frame
  lifecycle, render passes, and the **ordered cross-pipeline command
  submission** that interleaves the bitmap/UI HUD pass and the Slug pass.

### Layers that survive a backend replacement

Replacing wgpu requires rewriting **`mkui-vector2d-wgpu` + `mkui-wgpu`** —
the resource / pass integration — while **`mkui-text` and `mkui-vector2d`**
survive unchanged: their semantics and encoded-data contracts (glyph
outlines/masks, Slug bands, cache keys, retained CPU blobs) are backend-neutral
by construction. This is the whole point of the split: the expensive-to-author
CPU stack (font parsing, Slug encoding) is written once and is not held hostage
to the GPU backend choice.

### Bitmap default + `slug` feature gate

- Bitmap / retro text remains a **first-class render lane and the production
  default in v0.10.0**, consistent with [ADR 0002](0002-mkui-text-own-the-stack.md)
  ("the `BitmapTextSystem` path stays as the permanent debug-fallback and
  visual-regression oracle"). Sprint 7 adds the native glyph lane; it does not
  replace the default.
- Cargo feature **`slug` is default-off**. It gates the Slug-specific WGPU
  integration (`mkui-vector2d-wgpu` pipelines and their `mkui-wgpu` wiring) —
  **not** the backend-neutral text contract. `mkui-text` and `mkui-vector2d`
  build and behave identically whether or not `slug` is enabled; the feature
  toggles only the GPU lane.

### Ordered render-command semantics

`Scene::primitives` order **is** the semantic paint order. The cross-pipeline
submission in `mkui-wgpu` may **batch adjacent same-lane commands only** (a run
of bitmap/UI primitives into one HUD pass, a run of Slug primitives into one
Slug pass); it may **not reorder** commands. A bitmap primitive that appears
after a Slug primitive in `Scene::primitives` paints after it. This preserves
the painter's-order contract end users rely on while still allowing the
backend to coalesce same-lane work for pass efficiency.

### Relationship to ADR 0006

This ADR fills in the "future Slug / mkui-vector2d integration" seams
[ADR 0006](0006-wgpu-declarative-bridge.md) reserved:

- The retained `Mkui::with_scene` raw-scene escape hatch is the low-level
  surface direct-GPU experiments (Slug, vector2d) build against, exactly as
  ADR 0006 §"`Mkui::with_scene` as the retained low-level escape hatch"
  anticipated.
- The HUD tessellation pipeline (ADR 0004) is preserved verbatim; the Slug lane
  layers **beside** it under `mkui-wgpu`'s ordered submission, not on top of the
  walker. ADR 0006 §"Consequences" already noted "any future tessellation work
  (Slug-style outline text per ADR 0002) drops in without disturbing the
  bridge" — this ADR makes the resource boundary that keeps that true explicit.

## Consequences

- The four-crate boundary is fixed before a line of `mkui-vector2d` /
  `mkui-vector2d-wgpu` is written, so the implementation issue
  ([#65](https://github.com/mikbry/ui/issues/65)) inherits an unambiguous
  target rather than discovering the boundary mid-build.
- A future backend swap (e.g. a browser WebGPU validation target, Sprint 8+) is
  a rewrite of two crates, not four. The CPU text/vector contracts are insulated
  from the GPU backend choice by construction.
- `mkui-vector2d-wgpu` taking **borrowed** device/queue/target-format — rather
  than owning them or depending on `mkui-wgpu` — keeps the graph acyclic and
  makes the GPU-resource crate independently testable against a throwaway wgpu
  context.
- The `slug` default-off gate means v0.10.0 ships the new lane dormant: the
  production default stays bitmap, and the Slug path is opt-in until it is
  promoted (Sprint 8+). No consumer is forced onto the native glyph lane before
  it is production-ready.
- The "batch adjacent same-lane only, never reorder" rule constrains the
  cross-pipeline submitter: it cannot globally sort primitives by lane for
  fewer pass switches. That is a deliberate cost paid to preserve painter's
  order; the win is that `Scene::primitives` stays the single source of paint
  truth across all backends.

## Alternatives considered

- **One `mkui-vector2d-wgpu` crate that both encodes Slug bands and owns the
  GPU resources.** Rejected. Folding the backend-neutral encoder into the
  wgpu-resource crate would couple the Slug algorithm to `wgpu`, so a future
  backend would have to re-derive the encoding instead of reusing it. The
  encode/CPU contract (`mkui-vector2d`) and the GPU-resource lane
  (`mkui-vector2d-wgpu`) are split precisely so the algorithm survives a backend
  replacement unchanged.

- **`mkui-vector2d-wgpu` depends on `mkui-wgpu` (owns or borrows a `Surface`).**
  Rejected. That introduces a back-edge (`mkui-wgpu -> mkui-vector2d-wgpu ->
  mkui-wgpu`) — a cycle — and hands surface/frame lifecycle to a crate whose
  job is buffers and pipelines. Borrowed device/queue/target-format inputs give
  it everything it needs to allocate GPU resources without owning the surface or
  closing the cycle.

- **`mkui-text` uploads glyph masks to the GPU directly (depends on `wgpu`).**
  Rejected. It breaks the [ADR 0002](0002-mkui-text-own-the-stack.md)
  backend-neutrality commitment and would force `mkui-text` to pull the
  ~150-crate `wgpu` transitive tree — the same objection ADR 0006 raised against
  putting `WgpuRenderable` in `mkui-runtime`. `mkui-text` owns CPU-side
  outlines/masks; the consumer owns the upload.

- **Make the native glyph lane the v0.10.0 production default and drop the
  bitmap path.** Rejected. ADR 0002 committed the bitmap path as the permanent
  debug-fallback and visual-regression oracle. The native lane is new in Sprint
  7 and unproven; shipping it default-on (and removing the oracle it would be
  validated against) inverts the risk. Bitmap stays default; `slug` is
  default-off until Sprint 8+ promotion.

- **Allow the cross-pipeline submitter to globally reorder primitives by lane
  for fewer pass switches.** Rejected. It would break painter's order — a Slug
  primitive could paint over a later bitmap primitive — violating the
  cross-binding semantic that `Scene::primitives` order is paint order.
  Adjacent-same-lane batching captures most of the pass-switch win without
  touching ordering.

## Out of scope (reserved for future sprints)

- **Implementing the crates / pipelines.** This ADR is the contract;
  [#65](https://github.com/mikbry/ui/issues/65) and later build to it.
- **Building a custom RHI above WGPU.** `mkui-vector2d-wgpu` targets wgpu
  directly via a borrowed context; there is no intermediate hardware
  abstraction layer.
- **General vector lanes beyond the Sprint 7 glyph slice.** Browser WebGPU
  validation, analytic primitives, icons, general curved strokes, and a
  production-default Slug lane are Sprint 8+.
