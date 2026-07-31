# Horizontal first-root coverage sign flip

## Mutation

The committed mutant changes exactly one executable statement corresponding
to pinned `SlugPixelShader.hlsl:207`:

```diff
- xcov += clamp(r.x + 0.5, 0.0, 1.0);
+ xcov -= clamp(r.x + 0.5, 0.0, 1.0);
```

Source:
`../../../src/reference-mutant-h-coverage-sign-flip.wgsl`

This is an intentional defect, not an alternate implementation. Root 1 is
eligible to add horizontal-ray coverage under the `0x2E74` root code. Flipping
its sign causes an edge contribution to cancel or reinforce with the wrong
winding direction. H is the chosen probe because its separated vertical stems
produce long, easy-to-localize horizontal-ray intersections.

## First and only seeded execution

The mutation was selected and committed before execution. It was run once on
2026-07-27 with the same Lavapipe environment used for known-good captures. No
mutation parameter or test glyph was changed after output was observed.

At the committed `8/255` maximum per-channel threshold:

```text
PASS threshold=8 differing_pixels=76 differing_columns=1/128 max_column=25 max_column_differing_pixels=76/128 max_channel_delta=153
```

The mutant visibly narrows the left H stem by one raster column. This proves
that the harness is sensitive to this specific wrong-sign coverage defect.

## Extended-size executions

On 2026-07-31, the same preselected mutation and H probe were rerun without
adjustment at each cap height added for `mikbry/ui#165` Part A. At the same
strictly-greater-than-`8/255` threshold, every size remains detectable:

| Cap height | Differing pixels | Differing columns | Maximum-difference column | Max channel delta |
| ---: | ---: | ---: | --- | ---: |
| 12px | 10 | 1/16 | `x=3`, 10/16 pixels | 51 |
| 16px | 36 | 3/21 | `x=4`, 13/21 pixels | 170 |
| 48px | 38 | 1/64 | `x=12`, 38/64 pixels | 204 |

| File | SHA-256 |
| --- | --- |
| Known-good `H_1x.png` | `cf76a82cdad190fadf9c61218899f8687522aff21d5f8300e8b854034369e50c` |
| Mutant `H_1x.png` | `1301fab22c782907e0ab6420b07eab6655a61bfbf79b93d7d7da369d4f792dca` |
| Mutant `H_12px_1x.png` | `3071c992db947273c321a676a69f3ad35e972a59f77efe94c1ded3f03cab3d64` |
| Mutant `H_16px_1x.png` | `8b1d860b35ebb5ae652e99cd46fb1d8921dc64ac945ecaa7c125336baac7f788` |
| Mutant `H_48px_1x.png` | `4715861babbb3c4b271159f18ac1c8a1c8cd6cb6afb6f423fad35294c9406662` |
| Mutant WGSL | `09c750f33078559f21119f788be111f9031ba486729ebcea543fd1efe47e5b4d` |
