# ADR 0002 — `mkui-text` owns the stack (no external Rust text crates)

## Status

Accepted (Sprint 2, 2026-05-22).

## Context

Text rendering for a Rust UI framework is a substrate decision — once made, it
is expensive to reverse. The Rust ecosystem offers mature options that any
project facing "render UI text" reaches for:

- `cosmic-text` (shaping + layout, ~40 transitive deps)
- `swash` (font parsing + rasterisation)
- `HarfRust` (HarfBuzz port for shaping)
- `skrifa` (font parsing)
- `fontdb` (font database)
- `glyphon` (`cosmic-text` → `wgpu` atlas adapter)
- `parley` (Linebender layout)
- `fontdue`, `ab_glyph` (pure rasterisers)

mkui's predecessor reference (stonesketch's `gui.md`) committed explicitly to a
from-scratch text stack — the same architectural commitment mkui inherits.
External code review (Codex rounds 1-3) proposed adopting `cosmic-text` +
`glyphon`; the operator reversed that direction on 2026-05-21.

## Decision

`mkui-text` owns the entire text pipeline. Zero external Rust text-stack crates.

- Sprint 2 shipping deliverable: `BitmapTextSystem` — a 5×7 ASCII bitmap
  prototype, ~475 lines. It implements the `TextSystem` trait that backends
  consume.
- Future sprints implement SFNT parsing, glyph outlines, and GPU-side
  rasterisation directly in `mkui-text`.
- Sprint 4+ targets Slug-style direct-GPU outline rendering per external
  review.
- The `BitmapTextSystem` path stays as the permanent debug-fallback and
  visual-regression oracle even after richer rendering ships.

`thiserror` is the deliberate exception, added in Sprint 3 ([#38](https://github.com/mikbry/ui/issues/38)).
It is required for typed error propagation across the trait surface but does
not pull in any text-stack functionality.

## Consequences

- `mkui-text` stays small and reviewable. The round-2 external review named it
  "the new gold standard" for focused responsibility — a single substrate
  concern, no incidental complexity.
- mkui does not get `cosmic-text`'s full Arabic / Devanagari / Khmer / emoji
  shaping for free. That is owned engineering work in future sprints.
- The `TextSystem` trait is the public contract; backends consume that trait,
  not concrete implementations. Swapping the bitmap path for a glyph-outline
  path later is a trait-impl change, not a backend change.
- Build-time and binary size stay bounded — no ~2 MB binary delta from a
  text-stack pull.
- Release cadence is owned. mkui-text does not block on Linebender or pop-os
  release schedules for shaping or layout updates.

## Alternatives considered

- **`cosmic-text` + `glyphon`-shaped atlas.** Rejected. Pulls ~40 transitive
  deps and a ~2 MB binary delta. Creates opaque coupling to Linebender / pop-os
  release cadences. Does not fit the "own the stack" commitment that the
  predecessor reference and operator committed to.
- **Platform-native bindings** (CoreText on macOS, DirectWrite on Windows,
  Pango on Linux). Rejected. Three implementations to maintain, per-OS metric
  drift, no WASM option, and a third more surface area to keep in lockstep
  across backends.
- **Pure rasterisers** (`fontdue`, `ab_glyph`). Rejected. No shaping, no
  fallback, no emoji. The current bitmap prototype is already adequate for
  Latin smoke tests; a pure rasteriser would not unlock the future work
  (shaping, fallback chains, GPU outline rendering) that mkui-text needs.
