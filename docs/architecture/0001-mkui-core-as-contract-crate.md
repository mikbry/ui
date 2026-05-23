# ADR 0001 — `mkui-core` as the contract crate

## Status

Accepted (Sprint 1, 2026-05-12).

## Context

mkui targets multiple backend renderers (web, console, wgpu, native) from a single
component model. Without a single contract crate, each backend would re-define
`View`, `Text`, `Button`, the theming surface, the layout surface, and the input
surface independently. Backends would drift apart in subtle ways — different
field names, different default values, different event semantics — and any change
to the component model would require coordinated edits across every backend.

The workspace also has language bindings (`mkui-c`, `mkui-py`) and a bridge crate
(`mkui`) that re-exports a backend selected by Cargo features. These layers all
need a stable type surface to consume.

## Decision

`mkui-core` is the domain-free contract crate.

- It depends on `thiserror` and nothing else. No backend-specific deps.
- Every backend crate depends ON `mkui-core` for component definitions, layout,
  theming, input, and shared headless logic.
- The dependency direction is strict and one-way:

  ```
  bindings (mkui-c, mkui-py)
      ↓
  bridge (mkui)
      ↓
  backends (mkui-web, mkui-console, mkui-wgpu, mkui-native)
      ↓
  mkui-core
  ```

- `mkui-core` never imports anything from a backend crate. Backend-specific
  types (DOM nodes, crossterm cells, wgpu pipelines, winit events) stay in their
  respective backends.

What `mkui-core` owns:

- `components` — the renderable tree (`Component`, `View`, `Text`, `Button`).
- `headless` — pure-logic components shared by every backend (state, events,
  a11y traits).
- `theme` — `Theme`, `ThemeMode`, `ColorTheme` (no platform colors).
- `layout` — `Layout`, `FlexDirection`, `Justify`, `Align`, `Edges`.
- `input` — `InputEvent`, `Key`, `PointerButton` (backend-neutral events).
- `style`, `event`, `state`, `error` — supporting contracts.

## Consequences

- Adding a new backend means depending on `mkui-core` only. The new backend
  consumes `Component` trees via `Any` downcasting, maps `Theme` and `Layout`
  to its native styling, and normalises native events into
  `mkui_core::input::InputEvent`.
- Adding a new component requires a coordinated change in `mkui-core`'s
  `components` module — every backend then opts in to rendering it (or falls
  back to its registered default; see [ADR 0003](0003-mkui-web-registry-based-extension.md)).
- Bug-for-bug parity across backends is achievable because everyone consumes
  the same headless logic — state machines, variant resolution, and event
  normalisation live in one place.
- The one-way dependency graph keeps the workspace acyclic and keeps
  compile-time low for downstream consumers that pick a single backend.

## Alternatives considered

- **One-crate workspace.** Rejected because backend-specific code (DOM
  bindings, wgpu pipelines, crossterm rendering) would clutter every consumer's
  dependency tree. A WASM-only app would still compile crossterm; a console app
  would still pull wgpu.
- **Per-backend contract crates** (e.g. `mkui-web-contract`, `mkui-console-contract`).
  Rejected because the contracts would diverge as each backend evolved
  independently. The whole point of a multi-backend toolkit is one component
  model, not several.
