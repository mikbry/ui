# ADR 0008 — Bitmap labels restricted to integer scales + device-pixel snap, not routed through Slug

**Status:** Accepted (mkui-slug-rewrite mission Phase 4, `mikbry/ui#157` step 8, 2026-07-30)

## Context

Codex's original 8-step plan for the Slug rendering completion (`docs/chevalier/mkui-slug-rewrite/codex-8-step-plan.md`) closes with step 8: the `text` example's demo composes an Abel title rendered through the new Slug vector lane (Phases 1-3) next to a label — `"Abel via Slug, label via bitmap"` — still rendered through the older 5×7 bitmap text system. Two options were on the table:

- **Variant A:** route the label (and, by extension, `mkui-text`'s bitmap system generally) through the SFNT/Slug pipeline too, so every text primitive shares one rendering path.
- **Variant B:** keep bitmap as a separate, intentional lane, but close its two known correctness gaps — non-integer scaling and no device-pixel alignment — so it stops being the "known-lossy fallback" and becomes a deliberate, correct-for-its-purpose renderer.

dame-rubric.md § Phase 4 frames this explicitly as chevalier's judgment call ("the choice itself is not judged"), with a full BLESS criteria list for each variant.

**The deciding constraint: the ratified reference-harness adapter cannot evaluate Variant A.** `reference-harness/src/main.rs`'s camera is hard-coded — `BASE_EM_PIXELS = 96.0`, scaled only by `--dpi` — with no flag to render at an arbitrary font size. Variant A's (N) criterion requires "dame renders the label text at 12px via adapter... SAME comparison for 48px", but the adapter has no way to produce a 12px or 48px render; it can only render the eight ratified glyph fixtures at their fixed ~96px-equivalent scale. Modifying the adapter to add a size parameter is forbidden outright (CHARTER § Immutability: the reference-harness is frozen at ratification sha `9f76af3`, and dame's cross-phase invariant check treats any adapter commit as `blocked_reason: adapter_tampered`). Choosing Variant A would therefore either require an operator amendment to the adapter (a second Phase-2-style oracle_ambiguity detour) or ship an unverifiable criterion. Variant B's own (N) criterion — sub-pixel-invariance via piecewise-constancy, "same math as Phase 3" — is entirely self-contained: it needs no external adapter at all, only mkui's own renderer, exactly the infrastructure Phase 3 already built and proved out.

A secondary consideration: Variant A is also the larger implementation, requiring a genuine SFNT-outline-to-Slug-curve bridge for *arbitrary* label text (not just the eight curated, hand-vetted glyph fixtures the adapter and this mission's whole verification chain are built around), which is a materially bigger surface to get right in one phase than tightening an existing, already-correct-shaped bitmap code path.

## Decision

Choose **Variant B**. Fix the bitmap text lane's two correctness gaps in place, rather than routing labels through Slug:

1. `mkui-text::bitmap::bitmap_scale` now rounds to the nearest integer (`(font_size_px / REFERENCE_FONT_SIZE_PX).max(1.0).round()`, with a `debug_assert!` documenting the invariant) instead of returning an arbitrary float. The bitmap face is a fixed 5×7 pixel grid with no intermediate representation; a non-integer scale forces nearest-neighbor upscaling to duplicate source rows/columns unevenly — exactly the asymmetric-scaling defect this mission's Slug work fixes for vector text, now closed for the bitmap lane too.
2. `mkui-wgpu::tessellate_text` snaps each glyph cell's screen-space origin to the device-pixel grid when `font_size_px` is below the same `SMALL_TEXT_CAP_HEIGHT_PX` (16px) threshold Phase 3 established for Slug, using the frame's fresh `device_pixel_ratio`. `tessellate_primitives`/`tessellate_text` gained a `device_pixel_ratio` parameter, computed once per `Renderer::render` call (`crates/mkui-wgpu/src/render/mod.rs`) and shared with the Slug lane's own dilation/baseline-snap math — bitmap tessellation already re-runs fresh every frame from the declarative `Scene` (unlike Phase 3's Slug `place_slug_run`, which is called once at scene-construction time), so there is no scene-construction-time caching for a DPI change to go stale against; no Phase-3-style redesign was needed here.

`tessellate_scene`/`tessellate_scene_with_text` (used by `examples/native-window` and `examples/atoms-on-wgpu`, and `mkui-wgpu`'s own non-windowed `Renderer` helper) keep their existing signatures, internally passing `device_pixel_ratio = 1.0` — those callers are unaffected in behavior; only the real windowed render path gains the fresh per-frame ratio.

## Consequences

- Every bitmap-rendered string in mkui — not just the demo label — gets integer-scale bitmap blocks and device-pixel-aligned positioning at small sizes, for free. This is a correctness fix to the shared bitmap lane, not a special case for one demo string.
- The "Abel via Slug, label via bitmap" composition in `examples/text` is unchanged in shape: it still demonstrates two distinct, intentional text lanes side by side. That composition itself was never the thing under question — the demo's *point* was always to show both lanes existing; Phase 4 makes the bitmap lane one of them stop being lossy for lossy's sake.
- No new SFNT-to-Slug bridge for arbitrary text is built. If a future mission wants every text primitive routed through Slug, that remains available as Variant A — this decision does not foreclose it, it only defers it past a phase whose own oracle can't verify it.
- `bitmap_scale`'s public contract changes: callers relying on fractional scales between integers (e.g. requesting an 11px font previously getting scale `1.1`) now get the nearest integer (`1.0`) instead. mkui is pre-1.0 (`CHANGELOG.md`'s stated policy: "breaking changes can land on minor bumps"), and no in-tree caller depended on fractional bitmap scaling.

## Alternatives considered

**Variant A (route bitmap through SFNT/Slug):** rejected primarily because its own BLESS criterion depends on adapter capabilities (font-size-parameterized rendering) that the ratified, frozen reference-harness does not have and cannot be given without an operator amendment — the same oracle-ambiguity shape as Phase 2's thin-gap block, avoidable this time by choosing the variant whose verification is actually self-contained. Secondarily, it is materially more implementation surface (a general SFNT-outline-to-Slug bridge for arbitrary label text) than Variant B's two targeted fixes to an already-correct-shaped code path.

**Leaving bitmap's non-integer scaling and unsnapped positions as-is (do nothing):** rejected because Codex's original plan step 8 explicitly named this as one of the two live options, not a non-issue — the bitmap lane's blurriness at fractional scales and sub-pixel positions is a real, user-visible defect class, the same one this entire mission exists to fix for the vector lane.
