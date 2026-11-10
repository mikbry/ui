# ADR 0012 — Host-owned GPU and frame: `EmbeddedRenderer`

## Status

Accepted (Sprint 8, 2026-11-10).

## Context

Issue [#173](https://github.com/mikbry/ui/issues/173) opens `mkui-wgpu` up so
a third-party `wgpu`-based game or interactive engine can host mkui as its UI
layer, drawn directly inside the host's own `wgpu` frame — not through a
window of mkui's own, and not through an HTML layer. This issue, E2
([#175](https://github.com/mikbry/ui/issues/175)), is the hosting seam
itself: a host-owned GPU and frame mode distinct from the existing
winit-`ApplicationHandler`-owned "standalone" shell ([ADR 0004](0004-mkui-wgpu-2d-hud-pipeline-port.md),
[ADR 0006](0006-wgpu-declarative-bridge.md)).

[`Renderer`](../../crates/mkui-wgpu/src/render/mod.rs) creates its own
`wgpu::Instance`, requests its own adapter / device / queue, negotiates its
own `Surface` from an `Arc<winit::window::Window>`, and owns the per-frame
command encoder, submission, and present call end-to-end. A host engine
already has all of that — its own instance, adapter, device, queue, and a
frame target (its swapchain view) that its own 3D scene has already drawn
into. Making `mkui-wgpu` usable inside that host means it must not create a
second instance, a second surface, or a second window; and it must not
introduce a second command-buffer submission or an extra texture copy to
get mkui's pixels onto the host's frame — the acceptance bar is one frame,
one queue submission, no extra copy.

[ADR 0007](0007-gpu-resource-ownership-text-vector2d.md) already established
the shape of this discipline one layer down: `mkui-vector2d-wgpu` takes a
**borrowed** `device` / `queue` / target-format from `mkui-wgpu` at
initialization, rather than owning them or depending back on `mkui-wgpu`
(which would be a cycle). This ADR extends the same borrowed-resources
discipline to the top of the stack — `mkui-wgpu` itself, when a host (not
`mkui-wgpu`) owns the instance/surface/window.

## Decision

Add [`EmbeddedRenderer`](../../crates/mkui-wgpu/src/render/embedded.rs) as a
second, parallel entry point alongside [`Renderer`] + [`WgpuApp`]'s
`ApplicationHandler` shell. The existing winit-owned path is unchanged and
stays the documented path for apps that don't embed in a host.

- `EmbeddedRenderer::new(device: &wgpu::Device, target_format: wgpu::TextureFormat, color_space: EmbeddedColorSpace)`
  borrows the host's device and the format of the host's own frame target.
  It creates no `wgpu::Instance`, no adapter, no `wgpu::Surface`, no window.
  It owns exactly one GPU resource: the UI render pipeline, built once
  against the host's format.
- `EmbeddedRenderer::render_into(&self, device, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, target_size, scene, text_system)`
  records one UI render pass into the **host's own** encoder and view, with
  `wgpu::LoadOp::Load` — it never clears, so the host's already-drawn 3D
  scene survives underneath. It never calls `queue.submit`, never acquires a
  surface texture, and never calls `present`. The host does all three,
  exactly once per frame, around this call: record its own 3D pass into the
  frame view, call `render_into` against the same encoder and view, then
  `queue.submit` once and present.
- Color space: the standalone `Renderer` always composites into a linear
  intermediate target and defers the sRGB encode to a separate present pass,
  because it picks the final surface format itself. `EmbeddedRenderer` has
  no such freedom — the host's target format is given — so it draws
  directly into it in one pass. `target_format.is_srgb()` alone cannot pick
  the right fragment entry point: formats where it's `false` split into two
  groups needing *opposite* handling — a plain 8-bit UNORM format
  (`Bgra8Unorm`) stores encoded sRGB bytes and needs a manual OETF, while a
  linear float format (`Rgba16Float`, an HDR intermediate) stores
  already-linear values and needs no encode at all. An initial version of
  this renderer inferred the choice from `is_srgb()` alone and silently
  mishandled linear float targets (caught in review before merge); the
  shipped API instead takes an explicit `EmbeddedColorSpace` — `Linear` or
  `EncodedUnorm` — so the host, which alone knows what its target format
  means, decides. `Linear` draws through `fs_ui_triangles` (pass-through,
  linear blend); `EncodedUnorm` draws through `fs_ui_triangles_encode_srgb`
  (manual OETF, gamma-space blend — see the module docs for why this one
  case is a documented Phase 1 simplification rather than a second
  intermediate-plus-blit pass, which would itself be the "extra texture
  copy" the embedding contract rules out).
- Phase 1 scope: the triangle lane only (UI shapes, bitmap text, borders —
  everything every component renders through today).
  `Primitive::SlugGlyph` / `Primitive::VectorPath` primitives are silently
  skipped by `render_into` (the same no-op `tessellate_primitives` already
  gives them off their dedicated lane), not rejected. Wiring the Slug/vector
  lane into the embedded path is deferred to a follow-up issue so this ADR's
  scope stays the hosting contract itself, not every render lane's parity
  with it.

## Consequences

- A host that already owns a `wgpu::Device` and a per-frame `TextureView`
  can draw an mkui HUD into it with exactly the calls above — no second
  instance, no second surface, no second submission, no copy between an
  mkui-owned texture and the host's own.
- `EmbeddedRenderer` and `Renderer` now both exist as documented, permanent
  entry points, exactly as `Mkui::new()` (declarative) and `Mkui::with_scene`
  (raw-scene escape hatch) already coexist per ADR 0006 — neither is
  deprecated by the other. A consumer picks `Renderer`/`WgpuApp` for a
  window of mkui's own, or `EmbeddedRenderer` to draw inside a host's frame.
- The borrowed-device discipline ADR 0007 established for
  `mkui-vector2d-wgpu` now has a second, independent precedent one layer up,
  reinforcing it as the workspace's general answer to "a GPU-resource type
  that must not own the surface/instance/window" rather than a one-off.
- Embedded hosts that declare `EmbeddedColorSpace::EncodedUnorm` get
  gamma-space alpha blending for the UI lane instead of the standalone
  path's physically-linear blending — a known, narrow simplification, not a
  silent inconsistency (documented in `embedded.rs` and tested by the
  `gpu-tests` acceptance suite, which exercises exactly that case since the
  offscreen harness's target format is `Rgba8Unorm`). A host with a linear
  float target declares `Linear` instead and keeps physically-correct
  blending, matching the standalone path.
- Linux/Vulkan is not structurally excluded: `EmbeddedRenderer::new` takes a
  borrowed `wgpu::Device` and format with no backend-specific branch, and the
  `gpu-tests` acceptance suite already runs it on the Vulkan/Lavapipe CI
  harness — satisfying the #173 tracker's cross-cutting "Linux/Vulkan stays
  viable" constraint for this issue.

## Alternatives considered

- **Have the host render into an mkui-owned offscreen texture, then composite
  that texture into the host's frame.** Rejected outright — this is
  precisely the "extra texture copy" the issue's acceptance criteria
  prohibit, and it would need a second render pass (the blit) on top of the
  host's and mkui's own, not fewer.
- **`EmbeddedRenderer` owns a cached linear intermediate + present-style blit,
  mirroring `Renderer`'s standalone pipeline exactly.** Rejected for the
  same reason: the intermediate target is itself an extra texture the host
  didn't ask for, and sampling it back into the host's view in a second pass
  is the same forbidden copy, just mkui-internal instead of host-visible.
  Drawing directly into the host's view in one pass, with the fragment-entry
  branch described above, gets correct-enough color handling without it.
- **Reject (hard error) a non-sRGB host target format instead of the gamma-
  space blend fallback.** Rejected as unnecessarily restrictive for Phase 1:
  a UNORM target is a valid, if less common, host configuration, and the
  fallback is a documented, tested, narrow trade-off rather than a silent
  one — a host that needs linear blending into a UNORM target can configure
  an sRGB-format view instead, which `wgpu` supports on most swapchains via
  `view_formats`.
- **Infer the color-space handling from `target_format.is_srgb()` alone,
  with no separate parameter.** Tried first, rejected in review before
  merge: `is_srgb() == false` covers two groups needing opposite treatment
  (encoded 8-bit UNORM vs. linear float), and `wgpu::TextureFormat` has no
  general predicate that tells them apart — any such heuristic silently
  mishandles whichever group it didn't anticipate. An explicit
  `EmbeddedColorSpace` moves that decision to the one party who actually
  knows what the target format means: the host.
- **Fold Slug/vector-path lane support into this same PR.** Rejected per the
  issue's own scope discipline: the hosting contract (no owned
  instance/surface/window, one submission, no extra copy) is the load-
  bearing minimum #175 asks for; every render lane reaching parity with it
  is separable, deferrable scope, not a prerequisite for the contract to be
  correct.

[`Renderer`]: ../../crates/mkui-wgpu/src/render/mod.rs
[`WgpuApp`]: ../../crates/mkui-wgpu/src/app.rs
