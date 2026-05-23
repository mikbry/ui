# ADR 0004 — `mkui-wgpu` 2D HUD pipeline port

## Status

Accepted (Sprint 2, 2026-05-22).

## Context

`mkui-wgpu` needed a real GPU renderer to satisfy
[#2](https://github.com/mikbry/ui/issues/2). Building one from scratch is
multi-sprint substrate work — surface management, MSAA picking, swapchain
configuration, pipeline layout, render-pass orchestration, and a winit shell
each need a working baseline before any UI primitive can land.

A production-grade 2D HUD pipeline already existed in an upstream reference
project (stonesketch-render, ~2 854 lines of wgpu code). That pipeline was
production-tested but carried 3D-renderer concerns mkui does not need: scene
pass, shadow map, ambient occlusion, selection outline, accumulator. Importing
the full renderer would have brought all of those along, plus the maintenance
cost of code paths mkui's HUD use case never executes.

## Decision

Port the 2D HUD-specific subset of the upstream renderer into
`mkui-wgpu/src/render/mod.rs` (~600 lines).

- Drop the 3D-specific passes: scene pass, shadow map, ambient occlusion,
  selection outline, accumulator.
- Keep surface management, MSAA picker, swapchain config, HUD pipeline.
- Add a `mkui_wgpu::App` `winit::application::ApplicationHandler` shell so
  downstream apps consume the renderer in two lines via `Mkui::run()` — the
  shell owns the event loop and forwards events into `mkui-core`'s normalised
  `InputEvent` model.
- Expose the HUD primitives the renderer publishes (`Scene`, quads, panels,
  hit regions, theme-aware variant resolvers) as the public API surface for
  `mkui-wgpu`-targeted components.

The upstream project's licence permits the port; provenance is recorded in the
crate-level docs and in the `mkui-wgpu` README.

## Consequences

- `mkui-wgpu` has a working wgpu surface + HUD rendering as of v0.4.0. The
  `cargo run --example native-window` smoke opens a `winit` window and paints
  a single quad through the HUD `Scene` API — any visual regression in the
  pipeline shows up there.
- The 3D-specific passes can be re-added in future sprints if mkui ever
  needs 3D scenes; the current architecture does not preclude them. The
  pipeline boundary is `Scene` → HUD pass; an additional scene pass would
  layer on top without disturbing HUD rendering.
- Sprint 4+ adds Slug-style direct-GPU outline text rendering on top of the
  HUD pipeline foundation. The text pipeline ([ADR 0002](0002-mkui-text-own-the-stack.md))
  produces glyph geometry that this pass consumes — bitmap fallback today,
  GPU outlines later.
- The first two shadcn-aligned atoms (`Badge` with 6 variants, `Dot` with
  status variants + halo + animation modifiers) ship as the proof-of-life
  consumers of the ported pipeline.

## Alternatives considered

- **Build from scratch.** Rejected. Multi-sprint timeline — surface,
  swapchain, MSAA, pipeline, render-pass orchestration, winit shell each need
  a working baseline before any UI primitive can land. The upstream port is
  bounded extraction work that delivered the same baseline in one sprint.
- **Adopt an existing Rust UI framework's renderer** (`egui`, `iced`, `gpui`).
  Rejected. Each of those frameworks carries opinions about scene shape,
  layout flow, and input handling that conflict with mkui's own
  contract-crate-driven scene primitives ([ADR 0001](0001-mkui-core-as-contract-crate.md)).
  Pulling in another framework's renderer would either force mkui's component
  model to bend toward that framework's assumptions, or require a thick
  adapter layer that re-implements most of what the port already gives us.
