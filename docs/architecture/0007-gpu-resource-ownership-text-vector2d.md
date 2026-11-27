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

- Bitmap / retro text remains a **first-class render lane and the permanent
  debug-fallback / visual-regression oracle**, consistent with
  [ADR 0002](0002-mkui-text-own-the-stack.md). It is not removed or
  deprecated by the promotion below.
- Cargo feature **`slug` was promoted to `mkui-wgpu`'s crate default on
  2026-11-17**, per this ADR's own anticipated "Sprint 8+ promotion" criteria
  (see Alternatives Considered below — those criteria are now met). The
  chevalier mkui-slug-rewrite mission (2026-07-28 to 2026-07-31) validated the
  glyph lane against a ratified reference adapter (max Δ ≤ 1/255 across 24
  golden-image comparisons on real GPU hardware), and `button`/`card`/`dot`
  (#140/#141/#142) each shipped a GPU-acceptance-tested `VectorPath`
  rendering mode on top of it. `slug` still gates the Slug-specific WGPU
  integration (`mkui-vector2d-wgpu` pipelines and their `mkui-wgpu` wiring) —
  **not** the backend-neutral text contract; `mkui-text` and `mkui-vector2d`
  build and behave identically regardless. `--no-default-features` retains
  the original bitmap-only / triangulated-`Quad` behavior unchanged, and CI
  (`mkui-wgpu-no-slug`, the `feature-slug` job's new no-default-features
  lines) keeps that path from silently rotting now that it's no longer the
  default anyone exercises by accident.
- **The `slug` Cargo *feature* default was threaded through the declarative
  bridge on 2026-11-26 (#311, first pass).** Before this date, nothing
  anywhere in `mkui` → `examples/showcase-common` → `examples/native-showcase`
  pinned `default-features = false` on `mkui-wgpu`, so the 2026-11-17
  crate-level default leaked through by accident, with no designed opt-out:
  `native-showcase` had no `[features]` table at all, so
  `--no-default-features --features wgpu` was not a valid invocation. This
  pass replaced the accident with a real default at each layer: `mkui` gains
  its own `slug` feature, default-on (mirroring `mkui-wgpu`'s own default),
  forwarded through a *weak* dependency edge (`slug = ["mkui-wgpu?/slug"]` —
  a no-op unless `wgpu` is also enabled, so it never drags `mkui-wgpu` into a
  `web`/`console` build), and `examples/showcase-common` /
  `examples/native-showcase` mirror the same shape one layer at a time. Each
  crate pins `default-features = false` on its downstream dependency so
  `--no-default-features --features wgpu` genuinely suppresses `slug`
  at every layer instead of it leaking back in through whichever
  dependency's own default happened not to be pinned — CI verifies this
  (not just a successful compile) by checking whether `mkui-vector2d-wgpu`,
  the Slug GPU-resource crate `mkui-wgpu` pulls in only when its `slug`
  feature is active, is present in `cargo tree`'s resolved dependency
  graph. `console-showcase`/`web-showcase` are
  unaffected — Slug has no console/web lane, and `mkui-wgpu` never enters
  their build graph. `atoms-on-wgpu`/`examples/text` keep their own
  independent, pre-existing
  `slug` features unchanged (out of scope for #311); `atoms-on-wgpu` and
  `host-embed-wgpu` have the same accidental-leak bug this entry fixes at
  the bridge layer, tracked separately as #323.
  **Correction (2026-11-26, same day):** this pass did NOT make
  `native-showcase` render *component text* through the Slug lane, contrary
  to what an earlier draft of this entry claimed. It genuinely did flip
  non-text vector lanes already gated purely on the Cargo feature (e.g.
  `button`/`card`'s `VectorPath` chrome, #279) — the false claim was
  specifically about text. No code anywhere in the declarative bridge
  (`crates/mkui`, `crates/wgpu/src/walker.rs`, `crates/wgpu/src/builder.rs`,
  or `crates/wgpu/src/app.rs`) ever called
  [`Renderer::set_slug_font`](../../crates/wgpu/src/render/mod.rs) — the one
  call that actually selects which registered `FontId` component text routes
  through. `Renderer::slug_font_id` therefore stayed `None` (its byte-
  identical-to-bitmap default) regardless of which Cargo features were on, so
  a plain `cargo run -p native-showcase --release` compiled with `slug` and
  still rendered bitmap *text* at runtime. Caught by re-verifying with `grep`
  for `set_slug_font` call sites rather than trusting the feature-flag state
  — the only call site anywhere in the workspace was
  `examples/host-embed-wgpu`'s own hand-wired one, which bypasses the
  declarative bridge entirely. The fix is the entry immediately below.
- **The runtime wiring gap above was closed on 2026-11-26 (#311, second
  pass).** `mkui_wgpu::high_level::Mkui` gained `with_slug_font(text_system,
  font_id)` (wgpu+`slug`-only), which pairs a Slug-registered
  `CompositeTextSystem` with the `FontId` to route component text through —
  refusing the pair if `font_id` isn't actually a
  `TextRenderClass::Slug`-class face on that system, so a mismatched pair
  fails loudly at construction rather than silently degrading to bitmap at
  render time. `run()` threads both into `WgpuApp` (`set_slug_font_id`,
  alongside the existing `text_system` retention from #66), and `resumed`
  now calls `Renderer::set_slug_font` on the freshly constructed renderer —
  the exact call `examples/host-embed-wgpu`'s `resumed` already makes by
  hand against `EmbeddedRenderer`, reached here through the bridge instead.
  `crates/mkui` re-exports `CompositeTextSystem`/`FontId` from `mkui-wgpu`
  (wgpu+`slug`-gated) and forwards `with_slug_font`, so a consumer names
  only its existing `mkui` dependency. `examples/native-showcase` is the
  first declarative-bridge consumer to opt in: it embeds the same Abel
  (Google Fonts, SIL OFL 1.1) fixture `host-embed-wgpu` already does, under
  the same "this example only, never the library" rule (#259) — font choice
  stays a host/app decision, not something `mkui-wgpu` bundles for every
  consumer. `--no-default-features --features wgpu` skips the whole block
  (native-showcase's own `slug` Cargo feature is off), so that invocation is
  unchanged: bitmap, byte-identical to pre-#311. `console-showcase`/
  `web-showcase` are unaffected — no Slug lane exists for either backend.
  `component-gallery` (`create_gallery_ui`) does not yet call
  `with_slug_font` either, so it stays on the bitmap lane despite also
  building with `slug` on by default; wiring it the same ~5-line way is
  deferred to [#330](https://github.com/mikbry/ui/issues/330).
- **The `component-gallery` gap above was closed on 2026-11-27 (#330).**
  `examples/component-gallery/src/main.rs` is now `run!`'s own `wgpu` arm,
  expanded by hand the same way `native-showcase`'s was for #311: it embeds
  the same per-example Abel fixture and calls `.with_slug_font(text_system,
  font_id)` on the `Mkui` `create_gallery_ui()` returns, before `.run()`.
  Unlike `native-showcase`, `component-gallery` had no `[features]` table
  of its own at all before this change — it only built with `slug` on
  because its `showcase-common`/`mkui` dependencies defaulted it on, which
  is enough to pull `mkui-vector2d-wgpu` into the dependency graph but not
  enough for `#[cfg(feature = "slug")]` to gate anything in
  `component-gallery`'s own `main.rs` (a crate's `cfg(feature = ...)` only
  sees its own declared features, never a dependency's). `component-gallery`
  therefore gains the same `default = ["wgpu", "slug"]` feature table and
  `default-features = false` dependency pins #324 gave `native-showcase`, so
  `cargo run -p component-gallery --release` renders Slug text with no
  extra flags and `--no-default-features --features wgpu` is a real,
  CI-verified bitmap opt-out here too.

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
- The `slug` default-off gate meant v0.10.0 shipped the new lane dormant: the
  production default stayed bitmap, and the Slug path was opt-in until
  promoted. **Promoted 2026-11-17** (see "Bitmap default + `slug` feature
  gate" above) once the Sprint 8+ validation this gate was waiting on
  actually happened.
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
  bitmap path.** Rejected *at the time* (Sprint 7, 2026-06-11): ADR 0002
  committed the bitmap path as the permanent debug-fallback and
  visual-regression oracle, and the native lane was new and unproven —
  shipping it default-on (and removing the oracle it would be validated
  against) would have inverted the risk. This was explicitly conditional,
  not permanent: "`slug` is default-off until Sprint 8+ promotion." The
  promotion criteria were met by 2026-11-17 (chevalier mission validation,
  three components shipping `VectorPath` rendering) — see "Bitmap default +
  `slug` feature gate" above. **The bitmap path itself is not dropped**;
  only the default flipped. Keeping both halves of this entry (the original
  rejection and its later resolution) is deliberate — it's the record of why
  the gate existed and why it was safe to lift, not something to delete.

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
