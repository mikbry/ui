# Slug reference harness

This standalone crate is the `verification_oracle.reference-adapter` for the
`mkui-slug-rewrite` chevalier mission. It mechanically ports Eric Lengyel's
pinned Slug vertex and pixel shaders from HLSL to WGSL, renders committed
`SlugGlyph` byte streams through `wgpu`, and writes deterministic RGBA PNGs.
It has no dependency on an mkui crate.

The oracle is independent of mkui's WGPU implementation. Its shader source is
derived only from the pinned upstream shaders, the 2017 JCGT paper, and
Lengyel's 2026 retrospective. `UPSTREAM.md` records the immutable source pin
and license; `PROVENANCE.md` maps the port and records every capture hash.

## Render one glyph

Run from this directory:

```sh
cargo run -- --cap-height 16 --dpi 1.5 --glyph A \
  --output /tmp/A_16px_1.5x.png
```

The required interface is:

```text
[--cap-height PX] --dpi N --glyph X --output path.png
```

`PX` is a positive logical-pixel cap height and defaults to `96`, preserving
the original adapter interface and output. The camera, canvas, padding, and
emission scale retain the original `128:16:96` proportions at every cap
height. `N` is a positive device-pixel scale factor. The committed references
use cap heights `12`, `16`, and `48` at DPI scales `1`, `1.5`, and `2`, plus
the original `96` captures at the same scales. `X` is one of `H`, `A`, `V`,
`M`, `g`, `o`, `plus`, or `pipe`. Set `WGPU_BACKEND=vulkan` to require
Vulkan/Lavapipe, or `WGPU_BACKEND=metal` to require Metal. `--shader` selects
a WGSL source and is used only to reproduce the committed mutant.

The eight `.slug` files are the public `SlugGlyph::to_le_bytes` representation:
revision, bounds, quadratic records, horizontal band table/index stream, then
vertical band table/index stream, all little-endian. Regenerate them with:

```sh
cargo run -- --write-fixtures glyphs
```

## Reproduce the Lavapipe capture

The committed PNGs were captured with the ARM64 Docker image
`rust:1.89-bookworm` at digest
`sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff`,
using Debian's Mesa Lavapipe packages. On a machine with Docker:

```sh
docker run --rm -v "$PWD":/work -w /work \
  -e CARGO_TARGET_DIR=/tmp/target rust:1.89-bookworm bash -c '
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq mesa-vulkan-drivers vulkan-tools >/dev/null
cargo test
mkdir -p goldens/known-good \
  goldens/seeded-mutant/mutant-h-coverage-sign-flip
# Preserve the original 96px names and bytes.
for dpi_pair in 1:1x 1.5:1.5x 2:2x; do
  dpi_value=${dpi_pair%%:*}
  dpi_label=${dpi_pair#*:}
  for glyph_name in H A V M g o plus pipe; do
    WGPU_BACKEND=vulkan cargo run --quiet -- \
      --cap-height 96 --dpi "$dpi_value" --glyph "$glyph_name" \
      --output "goldens/known-good/${glyph_name}_${dpi_label}.png"
  done
done
# Add size-qualified captures for the extended adapter.
for cap_height in 12 16 48; do
  for dpi_pair in 1:1x 1.5:1.5x 2:2x; do
    dpi_value=${dpi_pair%%:*}
    dpi_label=${dpi_pair#*:}
    for glyph_name in H A V M g o plus pipe; do
      WGPU_BACKEND=vulkan cargo run --quiet -- \
        --cap-height "$cap_height" --dpi "$dpi_value" \
        --glyph "$glyph_name" \
        --output "goldens/known-good/${glyph_name}_${cap_height}px_${dpi_label}.png"
    done
  done
done
WGPU_BACKEND=vulkan cargo run --quiet -- \
  --cap-height 96 --dpi 1 --glyph H \
  --shader src/reference-mutant-h-coverage-sign-flip.wgsl \
  --output goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_1x.png
cargo run --quiet -- \
  --compare goldens/known-good/H_1x.png \
  goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_1x.png \
  --threshold 8
for cap_height in 12 16 48; do
  WGPU_BACKEND=vulkan cargo run --quiet -- \
    --cap-height "$cap_height" --dpi 1 --glyph H \
    --shader src/reference-mutant-h-coverage-sign-flip.wgsl \
    --output "goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_${cap_height}px_1x.png"
  cargo run --quiet -- \
    --compare "goldens/known-good/H_${cap_height}px_1x.png" \
    "goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_${cap_height}px_1x.png" \
    --threshold 8
done
'
```

Each render prints its selected adapter. Ratification should require
`backend=Vulkan`, `device_type=Cpu`, and an adapter/driver identifying
`llvmpipe`; this prevents an accidental hardware or Metal recapture from being
mistaken for the committed Lavapipe set.

## Sensitivity test

The only mutation is committed as
`src/reference-mutant-h-coverage-sign-flip.wgsl`. It changes the first
horizontal root's coverage update corresponding to
`SlugPixelShader.hlsl:207` from addition to subtraction. The capture and
rationale live under
`goldens/seeded-mutant/mutant-h-coverage-sign-flip/`.

Inside the same Lavapipe container, reproduce the mutant once after producing
known-good `H_1x.png`, then run the pixel-column comparator (the full Docker
command above does both):

```sh
WGPU_BACKEND=vulkan cargo run --quiet -- \
  --cap-height 12 --dpi 1 --glyph H \
  --shader src/reference-mutant-h-coverage-sign-flip.wgsl \
  --output goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_12px_1x.png

cargo run --quiet -- \
  --compare goldens/known-good/H_12px_1x.png \
  goldens/seeded-mutant/mutant-h-coverage-sign-flip/H_12px_1x.png \
  --threshold 8
```

Expected extended-size results at 1x:

```text
12px: PASS threshold=8 differing_pixels=10 differing_columns=1/16 max_column=3 max_column_differing_pixels=10/16 max_channel_delta=51
16px: PASS threshold=8 differing_pixels=36 differing_columns=3/21 max_column=4 max_column_differing_pixels=13/21 max_channel_delta=170
48px: PASS threshold=8 differing_pixels=38 differing_columns=1/64 max_column=12 max_column_differing_pixels=38/64 max_channel_delta=204
```

This adapter distinguishes correct Slug output from at least one specific
defect class (`horizontal first-root coverage sign inversion at
SlugPixelShader.hlsl:207`), verified by comparing known-good against
seeded-mutant at threshold `8/255 maximum per-channel difference`. Adapter is
ratified iff this test passes at ratification-review time and can be reproduced
from committed sources.
