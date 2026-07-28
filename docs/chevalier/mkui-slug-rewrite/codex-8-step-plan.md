## Context

Sprint 8's substrate goal (arbitrary vector paths + stroke + Bezier encoder + wgpu adapter) landed in Wave 1: #146 + #148 + #150 + #152 + #154 + #155 all on `main`. Smoke tests confirm the pipeline runs (no panic on Metal, gamma-linear compositing correct).

But visible text quality on `cargo run -p text --features slug` and `cargo run -p atoms-on-wgpu --features slug` is not-crisp — asymmetric antialiasing, pixel-step fringing, mixed edge weights. Investigation via #156 established this is **not** an MSAA problem (empirical smoke test showed zero visible improvement); #156 was closed.

Codex's technical review of the current `mkui-vector2d-wgpu` Slug implementation identified the root cause: the current code is a Slug-inspired prototype implementing ~half the published algorithm.

## What Codex found (verbatim summary)

Comparison table (current vs full Slug):

| Area | Current | Full Slug |
|---|---|---|
| Source geometry | Original quadratic outlines | Same |
| Band encoding | Horizontal and vertical generated | Same |
| **GPU band use** | **Horizontal only** | **Both directions** |
| **Pixel size** | **CPU logical scale** | **GPU derivatives / real pixels** |
| **Edge coverage** | **One ray** | **Two weighted rays** |
| **Root handling** | **Direct quadratic roots** | **Robust root eligibility** |
| **Band overlap** | **Exact boundary** | **Small em-space epsilon** |
| Quad expansion | Constant 1.5 logical px | Half-pixel dynamic dilation |
| Small text | No optical snapping | Cap-height/device-pixel alignment |
| Supersampling | 1× | Generally unnecessary once analytic coverage is correct |

Key file references:
- Only horizontal bands uploaded: `crates/mkui-vector2d-wgpu/src/lib.rs:151`
- Only horizontal ray cast: `crates/mkui-vector2d-wgpu/src/slug.wgsl:149`
- AA width uses `scale_px_per_unit` (logical): `crates/mkui-vector2d-wgpu/src/slug.wgsl:153`
- Simplified root solver: `crates/mkui-vector2d-wgpu/src/slug.wgsl:114`
- Exact band overlap: `crates/mkui-vector2d/src/slug.rs:431`
- App logical→physical pixel gap: `crates/mkui-wgpu/src/app.rs:578`
- Bitmap 5×7 nearest-neighbor label: `crates/mkui-text/src/bitmap.rs:187`

References:
- Official Slug pixel shader: https://github.com/EricLengyel/Slug/blob/main/SlugPixelShader.hlsl
- JCGT Slug paper: https://jcgt.org/published/0006/02/02/
- Eric Lengyel's "A Decade of Slug" retrospective: https://terathon.com/blog/decade-slug.html
- Slug README tips-and-tricks (band epsilon): https://github.com/EricLengyel/Slug#tips-and-tricks

## Codex's recommended implementation order (verbatim)

1. Pack and upload the vertical bands and vertical curve indices.
2. Port the official horizontal and vertical coverage calculations, including root eligibility and weighted combination.
3. Replace `scale_px_per_unit` as the AA width with `fwidth(in.font_pos)` or equivalent derivatives.
4. Use a half-physical-pixel dilation for ordinary 2D text; add full dynamic dilation later if transformed/perspective text is needed.
5. Normalize band calculations to em space or provide `units_per_em`, then add the recommended band epsilon.
6. Snap the baseline or cap height to the physical pixel grid for small UI text.
7. Add golden-image tests at 1×, 1.5×, and 2× DPI. Existing tests mostly verify buffer structure and "some pixels changed," so they cannot catch softness or asymmetric antialiasing.
8. Render the lower label through the SFNT/Slug path too. If the bitmap style is intentional, restrict it to integer scales and snap every glyph to device pixels.

## Scope for this tracker (Sprint 8 tail)

**Phase 1 (highest visible impact, foundational)** — steps **1 + 2 + 3**:
- Upload vertical bands + vertical curve indices to GPU
- Port official H + V weighted-coverage calculation with root-eligibility rules
- Replace logical-pixel AA width with `fwidth()` derivatives
- **Acceptance**: `cargo run -p text --features slug` shows visibly symmetric antialiasing (no more asymmetric H vs V edge weights); `native-showcase` text edges look meaningfully crisper on Metal at both 1× and 2× DPI.

**Phase 2 (robustness)** — steps **4 + 5**:
- Half-physical-pixel dynamic dilation for the expanded quad
- Band overlap epsilon (~1/1024 em) to close floating-point gaps
- **Acceptance**: no thin gaps or unstable-pixel artifacts on rotated or near-tangent inputs; unit-test scaffolding for boundary cases.

**Phase 3 (polish + testability)** — steps **6 + 7**:
- Cap-height / baseline snapping to device pixel grid for small text
- Golden-image regression tests at 1×, 1.5×, 2× DPI (Lavapipe `gpu-offscreen` job, existing gate)

**Phase 4 (label consistency, optional)** — step **8**:
- Route the lower "Abel via Slug, label via bitmap" through Slug too, or explicitly restrict bitmap to integer scales + device-pixel snapping. Decision + scoping call.

## Dispatch model

Each phase = one issue = one PR = Codex review. Phases 1 → 2 → 3 in sequence (each rebases onto the last). Phase 4 is scope-decision-first.

**Sprint 8 close cannot happen until at least Phase 1 lands** — otherwise the substrate has no visible payoff and Sprint 8's "visible result" goal is unmet.

## Non-goals

- 4× MSAA (established as band-aid via #156; closed)
- Overhaul of the CPU-side vector encoder (that's done + tested in #148)
- Distance-field text (SDF) as a separate rendering path — orthogonal, later sprint if ever

## Related

- Closed #156 (MSAA — wrong direction; established empirically)
- Closed #147 (original MSAA attempt on pre-#155 architecture)
- #135 → #155 (linear-color pipeline reintroduce, merged)
- #137 → #148 (vector substrate, merged)
- #138 → #150 (wgpu VectorPath+Stroke adapter, merged)
- #144 → #146 (Mkui rustdoc, merged)
- #151 → #152 (CI cost, merged)
- #153 → #154 (VVL on gpu-offscreen, merged)

