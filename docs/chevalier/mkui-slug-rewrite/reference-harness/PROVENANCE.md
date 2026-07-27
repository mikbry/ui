# Port and capture provenance

## Independence boundary

This adapter was authored in the adapter-author role, not the Slug chevalier
role. The WGSL was transliterated from Eric Lengyel's two HLSL files at
`be3c13eb7d63f9e8aa5c583e42d92c374cb91d98`, cross-checked against the JCGT
paper and the 2026 retrospective. No file under
`crates/mkui-vector2d-wgpu/` was read. No mkui implementation is linked,
copied, or used at runtime.

The permitted public serialization source was
`crates/mkui-vector2d/src/slug.rs`. During the public-layout inspection, one
line-range read extended beyond the public `encode_slug_glyph` signature into
the CPU outline-flattening helper. Nothing in that overscanned helper was used
for shader authoring, band packing, fixture design, or expected output; those
all derive from the pinned upstream sources. This disclosure is included for
ratification audit completeness.

## WGSL line mapping

`src/reference.wgsl` has an upstream `file:line` comment on every executable
statement and non-trivial declaration. The following table is the section
index; the inline annotations are the per-line manifest.

| WGSL section | WGSL lines | Pinned HLSL source | Paper/retrospective cross-check |
| --- | ---: | --- | --- |
| Band texture width | 11 | `SlugPixelShader.hlsl:8-10` | JCGT p. 42 |
| Parameter, texture, and vertex I/O declarations | 13-42 | `SlugVertexShader.hlsl:8-37,65-78`; `SlugPixelShader.hlsl:265-275` | JCGT pp. 40-42 |
| `slug_unpack` | 44-50 | `SlugVertexShader.hlsl:40-45` | Vertex packing described by its HLSL comments |
| `slug_dilate` | 52-79 | `SlugVertexShader.hlsl:47-63` | "A Decade of Slug", "Dynamic Dilation", equations through the plus-root solution |
| `vs_main` | 81-96 | `SlugVertexShader.hlsl:80-101` | Retrospective: per-render MVP/viewport dilation |
| `calc_root_code` | 98-105 | `SlugPixelShader.hlsl:17-32` | JCGT pp. 35-38, especially Eq. (2) and `0x2E74` |
| `solve_horiz_poly` | 107-120 | `SlugPixelShader.hlsl:34-62` | JCGT p. 35, Eq. (1) |
| `solve_vert_poly` | 122-135 | `SlugPixelShader.hlsl:64-84` | Axis-transposed form of JCGT p. 35, Eq. (1) |
| `calc_band_loc` | 137-142 | `SlugPixelShader.hlsl:86-94` | JCGT pp. 41-42 |
| `calc_coverage` | 144-149 | `SlugPixelShader.hlsl:96-137` | JCGT p. 38, Eq. (3) |
| `slug_render` | 151-210 | `SlugPixelShader.hlsl:139-263` | JCGT pp. 38-42 |
| `fs_main` | 212-216 | `SlugPixelShader.hlsl:265-280` | JCGT pp. 41-42 |

Known-good source SHA-256:
`319cad1d9c76818a925c832029dc928fb2273b3cb8229076112b9ea74337283f`.

## Mechanical WGSL deviations

These are language or harness-binding translations, not algorithm changes:

1. HLSL `out` parameters in `SlugUnpack` and `SlugDilate` become small WGSL
   return structs.
2. HLSL `asuint` becomes WGSL `bitcast<u32>`. `saturate` becomes
   `clamp(value, 0.0, 1.0)`. The `TexelLoad2D` macro becomes `textureLoad`.
3. The HLSL array of five `float4` vertex attributes becomes five explicit
   WGSL locations. `nointerpolation` becomes `@interpolate(flat)`.
4. WGSL has no preprocessor. The port fixes the pinned HLSL's default build:
   `SLUG_EVENODD` and `SLUG_WEIGHT` are both undefined, so nonzero fill is
   used and optical-weight boosting is absent.
5. The upstream 4096-texel addressing rule is retained. The harness binds
   `RGBA32Float` curve data and `RGBA32Uint` band data rather than the
   repository README's compact 16-bit recommendations. The shader observes
   the same component values and only uses `xy` from band texels. This avoids
   host-side half-float quantization in an oracle; it is a storage-format
   deviation, not a shader equation change.
6. Each fixture curve receives two curve texels instead of sharing connected
   endpoints. The HLSL addressing and control-point values are unchanged;
   only unused texture space differs.
7. The harness supplies an orthographic row-vector MVP, identity inverse
   Jacobian, a bounding quad with outward corner normals, and the live
   viewport. Thus the complete pinned vertex shader, including dynamic
   half-pixel dilation, runs for every capture.
8. The target is `RGBA8Unorm`, cleared transparent, with white vertex color.
   PNG bytes are copied without blending, multisampling, supersampling, or
   color-space conversion.

No upstream bug was identified. Per adapter-author rules, no upstream formula
was corrected or redesigned.

## Fixture provenance

Fixtures use the public `SlugGlyph::to_le_bytes` field order. Lines use the
upstream duplicated-terminal-endpoint sentinel. Curves are assigned to eight
equal horizontal and vertical bands with the upstream README's `1/1024` em
overlap; axis-parallel lines are excluded from parallel-ray bands. Horizontal
members are sorted descending by maximum x and vertical members descending by
maximum y, as specified by the upstream README and JCGT p. 39.

The outlines are deliberately small, auditable test shapes:

| Fixture | Intended coverage | SHA-256 |
| --- | --- | --- |
| `H.slug` | horizontal plus vertical strokes; asymmetric contribution check | `1a2ecf92f2ac9daeadae604faa7b1e7ddea794652bb14221566ca662da02f0c8` |
| `A.slug` | diagonals and a crossbar join | `26e71682e87f050a18e9ba8ca7ebea3843fc68dcc16e73930a5772ac49e7d816` |
| `V.slug` | opposing diagonal edges | `0db2d3fdda5468c376eb81fdb68158b39f78f97f693298afab54bd99ba43bf52` |
| `M.slug` | multiple vertical and diagonal strokes | `bc2abeac039bc2c310214c7f98f4aa7508ab6e4f31cba7258b672a1c26e576c6` |
| `g.slug` | quadratic bowl, counter, descender, and curved join | `2919cfb55b3a0508db4fb2d1aacf4f1e5175dc73d50fa757b3ca5615464f9245` |
| `o.slug` | quadratic outer/inner circular symmetry | `3e19359006d1ea61cd8f429842804694a7b1b75059475879f4fbfc20abc4e024` |
| `plus.slug` | symmetric horizontal/vertical crossing | `8f04c8ece5bfd04a7680e3d9593bc0a7bcdc060715dd7b5b962b248d6e49c089` |
| `pipe.slug` | pure vertical stroke | `eb0b0c8ce4b035f620fc0587493d67f0cdd6aff52e7f17a91eb0b8edc4517866` |

`cargo test` byte-round-trips all eight generated fixtures through the
standalone parser.

## Known-good Lavapipe captures

All 24 files were captured on 2026-07-27 at approximately 11:31 UTC
(13:31 Europe/Paris) on host `Miks-Mac-Studio.local`, inside Docker Desktop
Linux/aarch64. Every render identified:

```text
adapter=llvmpipe (LLVM 15.0.6, 128 bits)
backend=Vulkan
device_type=Cpu
driver=llvmpipe
driver_info=Mesa 22.3.6 (LLVM 15.0.6)
```

| Glyph | 1x SHA-256 | 1.5x SHA-256 | 2x SHA-256 |
| --- | --- | --- | --- |
| A | `117a8e3a18d93f7413b9b9e87546471dc1b213eb2c53d0d2da235cb5c5dff90a` | `d1fe5fa5adfe4f3d1bcacb97b88ffa5693e9afc28b4412afcc5083b89051aff5` | `b9ef5e92e05d95a180ebfb115827d6929218cf63870235510ee70b992a95dd58` |
| H | `cf76a82cdad190fadf9c61218899f8687522aff21d5f8300e8b854034369e50c` | `8e6fc23e6d8e5c722482a84fe0da96585a1d99988a7016d9557b29fcee8364b5` | `4822487b8cdeb291669cf00066fe8e1d9091f70fdcc7b86fea3fe8c5bfcb34f8` |
| M | `7cc1d4852b8d60516625b0d1e49bf2463a6150550670c9b8c862d4eece300834` | `739d2bdbc7a4527cd99a6250d95d60bfe2fb40f829105ff6aa6d9d185c6ffe56` | `b0415912c4013112ef13eff0eeac94ae08eb8b827d47315f669a678733ba1b57` |
| V | `55b416b080ed0b802ee2cf577a78c46e618703e8efaf6198d672f69c33bc1eaf` | `0ed7419a8f940629ff73e62bc59c678e6c23ff0d6ab9a5a727a6d6ae6d535f06` | `f78364ef9f89b60fc96eaceb5ddb2c5cc7cfc7211ed16eda45780dcc5451570d` |
| g | `549e041dedd7992d362329791e77a8d7a89ea500af4a000b33d9cb11aa2208d3` | `6a2f4eb8b60bfa4ad5372e37edd5810c209f5632ea95ee7681e15e762ab72784` | `ab46ec2467917d69ab80456be534005eb0a5559b10aa303fa6d577ede8c9efe0` |
| o | `0aa1a1d3cad93521b1718eb9e68938f62b364fcfa3e6ba67967530609b136e62` | `cde59eacc75dd0cecc6a4d838d842c51b39ab9c0ceafecb70ad994baed0b277d` | `33a4322dd0443d9b315d3034f263dbd5401ffa74f65a55944435cea43c3fd1e9` |
| pipe | `9d40b2d8623178cd6193291ee2e24b6fa5a489848c28b822b4e45f1abdaf438d` | `28f63f6c778989dbd479c881bf0bee3cfcc79e1feb8578c242c789341b5643d3` | `0c4aedfd26bbd153e74088c28b6311aeee53af40825805b57eea6b576584bbf5` |
| plus | `e9f2a43cb9480517c21b25a93336018fa7a2e3b18ddcd824bb9d915329b852f9` | `607f934df08ea7099a56340387314d9051c9be07e2c3ed36dbf38fa6abb543c4` | `69bda5b74e151796bd3cdbcea05e1e47d291febefb6d42e80a8b0a68c7bb7ca1` |

## Seeded-mutant sensitivity result

The preselected mutation changes only the first horizontal root contribution:

```diff
- xcov += clamp(r.x + 0.5, 0.0, 1.0);
+ xcov -= clamp(r.x + 0.5, 0.0, 1.0);
```

- Mutant source:
  `src/reference-mutant-h-coverage-sign-flip.wgsl`
- Mutant source SHA-256:
  `09c750f33078559f21119f788be111f9031ba486729ebcea543fd1efe47e5b4d`
- Mutant capture SHA-256:
  `1301fab22c782907e0ab6420b07eab6655a61bfbf79b93d7d7da369d4f792dca`
- Known-good H 1x SHA-256:
  `cf76a82cdad190fadf9c61218899f8687522aff21d5f8300e8b854034369e50c`
- Comparison threshold: maximum absolute per-channel delta strictly greater
  than `8` on the 0-255 RGBA scale.
- Result: `76` differing pixels in `1/128` columns. Column `x=25` has all
  `76` differing pixels; maximum channel delta is `153`.

The mutant was executed once after the full known-good capture and passed on
that first execution. It was not adjusted or hand-tuned after observing output.

## Toolchain

| Component | Captured version |
| --- | --- |
| `rustc` | `1.89.0 (29483883e 2025-08-04)` |
| Cargo | `1.89.0 (c24e10642 2025-06-23)` in capture container |
| `wgpu` | `26.0.1` |
| `naga --version` | `26.0.0` (`naga-cli` 26.0.0); locked library `naga` 26.0.0 |
| `mesa-vulkan-drivers` | Debian `22.3.6-1+deb12u2` |
| Lavapipe/llvmpipe | Mesa `22.3.6`, LLVM `15.0.6`, 128 bits |
| Vulkan loader/tools | `libvulkan1 1.3.239.0-1`, `vulkan-tools 1.3.239.0+dfsg1-1` |
| Vulkan device API | `1.3.230`; conformance `1.3.1.1` |
| Docker image | `rust:1.89-bookworm`, digest `sha256:948f9b08a66e7fe01b03a98ef1c7568292e07ec2e4fe90d88c07bb14563c84ff` |

`Cargo.lock` SHA-256:
`7b9d7ec2b8ad6b3e3f17c6b6a08fc2b8ad54e0068a79f85be407735e12cfa915`.
