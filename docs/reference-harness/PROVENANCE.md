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

## Extended size support (2026-07-31, `mikbry/ui#165` Part A)

The harness now accepts the optional `--cap-height <px>` render argument. It
defaults to `96`, so commands that omit it retain the original adapter
behavior. The camera is scaled by `cap-height / 96`: the emission scale is
`cap-height * dpi`, while the canvas and padding retain the original
`128:16:96` relationship and are converted to device pixels by `dpi`. This is
only a camera, viewport, and emission-scale parameterization; neither
`src/reference.wgsl` nor the seeded-mutant WGSL was modified. Their SHA-256
values remain `319cad1d9c76818a925c832029dc928fb2273b3cb8229076112b9ea74337283f`
and `09c750f33078559f21119f788be111f9031ba486729ebcea543fd1efe47e5b4d`,
respectively.

All 72 size-qualified known-good captures below and the three extended mutant
captures were produced in the same pinned ARM64 Docker/Lavapipe environment
recorded above. Every render identified llvmpipe 15.0.6, Vulkan, CPU device
type, and Mesa 22.3.6.

### 12px known-good manifest

| Glyph | 1x SHA-256 | 1.5x SHA-256 | 2x SHA-256 |
| --- | --- | --- | --- |
| A | `3bbb6fbaf5116ebb010ea317bcfaafa9ed0ae675ca82d4836403137ae0724b7d` | `84e49ca5c03606156199e465b9afa8b55e337319c4b3cce826873a96b1eedd44` | `28d4e2f1bfb0f656c19b03e52c7dbe67bdaf5fca2e72bc0c7065d80d0fad8dc6` |
| H | `52ea1f02f18cd591dba375d0557718959eeb986c69406a291c04e5fa1a37d0dd` | `048ba1f46cfddd161c2aa6e339bccf5d386de3d838a457c5bdaec396b00dbf22` | `ad562a28733be4ef4a3836c76ca9059cdc0a015649bdce23b8e0a0fd351a7ebe` |
| M | `2e734853d886b33ae50c0232dfcf9c02d14a33f049544d5d54a4afa752c5ddae` | `4ad10ec050cf73ac280240ac4a8d9eceeb749832a91b82a215a3c83a23a74a3b` | `1e060a5446f8607a8a5f344d74566b9d131765716a932c880dabb04a4dbf98fd` |
| V | `f8eb3eb9e69e80d9f2cb56b0b3e9894479b5790f9f2063598946899574f5dfc0` | `b40d7cd28bab2ea94e6ab83e7c3512430a521980b5bd795cd015dd3928f78cce` | `7d6a28b8b7890c0f2e251b549538661fbb29f217528f38fbd4d3b178476f2e7d` |
| g | `da7dee86e5f695849f23fa1e416d1bcee20313135ad21e99f8dde73d5faaccc4` | `17cb88fa84fc9757cde8bacf6be853700533586884a2ec6b3f2ef7c1b22682e3` | `6b575f6a2cd8ed0101f7a6fe2afced1c70827a59890be09053d316a5b78d9d2a` |
| o | `373da472e18c6ff3bb66db1734c59bc5f6b64b5af60ed69481fd471e0f20237d` | `34937496021de58adcb07069c181709026504dfcfba173557213dce327bb5349` | `8ce017ff5e33c0141defe15e698733342837bd31d2d46c89c885b120138721c8` |
| pipe | `e78b7a1ce8a206d638fee79ca62e7bc6b57c57f4067329d782adda9d5c98c413` | `1f4b627f5c995e020a088a57a8fa2c33581eddec7f749c76bdd0f71e56b43697` | `4c9fee0022b5dba19d5f100518de57d7a397b737b1adef42407e63821bb1670e` |
| plus | `0c8581cfa48eb0f5bac1168d5c6513bb966ca949b33dd849a81f2ba14a6330b1` | `98ef76d770ec72d4ed26963c52b7797ab817452f5b1a05cc2e5ea1814e4d72f2` | `64bba22bb37119051976de0fa3aea654e913111577c4d5f0e1260444b24ea1e6` |

### 16px known-good manifest

| Glyph | 1x SHA-256 | 1.5x SHA-256 | 2x SHA-256 |
| --- | --- | --- | --- |
| A | `aa5f2e831d2c77e45c3bee56b525f31f837663503d82d51612dc6e9b9226cfcb` | `28d4e2f1bfb0f656c19b03e52c7dbe67bdaf5fca2e72bc0c7065d80d0fad8dc6` | `6cae31cb6b7e472b5dac02c9527dbfd23cafabd3fce71765584926a367a73ddd` |
| H | `18ebe14798dee8796ef4c86db558431fe48b91e1e8cc92cb57f0c2359f3aba9f` | `ad562a28733be4ef4a3836c76ca9059cdc0a015649bdce23b8e0a0fd351a7ebe` | `4172488a2f3a139990fe98b751ada601934aff3be07c0f8979ee3bc712a822ec` |
| M | `4e36689e4e42830ebaca8ad6b0609e58fc32be33229b29f776e6b28392003931` | `1e060a5446f8607a8a5f344d74566b9d131765716a932c880dabb04a4dbf98fd` | `a2407ef41d6ead229747d22c4d27235fadb08f3ec6989445538db4c573b0f711` |
| V | `f5adef426c8a4173a1297a5c924b5e465f89f4d0931b504100707fe3f65493e0` | `7d6a28b8b7890c0f2e251b549538661fbb29f217528f38fbd4d3b178476f2e7d` | `71340a3121eb59016a734f4075a9095be8f274d42a80c5000ff1606a57927834` |
| g | `246edc9595ff1b69134db1a717dd9aa772ee5e280a49a63a26a8e77d48e87d29` | `6b575f6a2cd8ed0101f7a6fe2afced1c70827a59890be09053d316a5b78d9d2a` | `ea28481ee6c776ef74bdd0187ade0d0b587a2818f5aa7556d45323737efb9d06` |
| o | `ccf88efefad889f3f4870d46a5017906185f58ef0ba8d9efa72137afee323712` | `8ce017ff5e33c0141defe15e698733342837bd31d2d46c89c885b120138721c8` | `6946f5c1ddf7c81d0df01c2e012d8e96d7b1e6b3fe872530d741d2147827aa44` |
| pipe | `7b0a659085b08bc04d313c1c116b32d7163c99f9eb6f115f2e99f6b9e80a8696` | `4c9fee0022b5dba19d5f100518de57d7a397b737b1adef42407e63821bb1670e` | `bb35f7db2eb384c2bd64050bae87dc7dfd84e74362f8e6c676620c30b49da024` |
| plus | `19f2c3b46d3b72d01a58efe987366d531a37f47c486e27c15e074d466461e0e0` | `64bba22bb37119051976de0fa3aea654e913111577c4d5f0e1260444b24ea1e6` | `087b57a99b55fe9c08b71181bd048c7071769b6d4b4af57ac149b7f2107a4a4c` |

### 48px known-good manifest

| Glyph | 1x SHA-256 | 1.5x SHA-256 | 2x SHA-256 |
| --- | --- | --- | --- |
| A | `c9a9924402ec762f504e98db12c461ad0e2a8fba695ac79d4e3b2adeb8e3238e` | `472e24f6a003e0185c391b34351cfcfe45fa16ec63d31171b52dd7a3f0d6af9d` | `117a8e3a18d93f7413b9b9e87546471dc1b213eb2c53d0d2da235cb5c5dff90a` |
| H | `c2de7703c4e6c371edceae010b8ce751cda50c20e967631bc795acfd42fe241f` | `2fa2a18f08076e744c457037822b33a0798505bebf545585386d59a66a446616` | `cf76a82cdad190fadf9c61218899f8687522aff21d5f8300e8b854034369e50c` |
| M | `75a97342cad5a55b28dd3c2b3bedf346970e9207505dc6af6db16907ed11b788` | `e745eae648da25855d394aaef18a0564241d65de59aedd1450ccc118de1ca967` | `7cc1d4852b8d60516625b0d1e49bf2463a6150550670c9b8c862d4eece300834` |
| V | `1011eb8c6ef62d8b09802922d5796cede98cccbfd411ef44b06b07f480c9d120` | `8b60766aa3aec9d365307a689ea4a7b3a4736032668f887f40c7283469a2e7b1` | `55b416b080ed0b802ee2cf577a78c46e618703e8efaf6198d672f69c33bc1eaf` |
| g | `6d080b940c8239a4f32977f41e44fafc1cd7d7e65e20f1d4f6a393d2015364c3` | `3867dec6b1ec490b1a417364ba68225dd7d9705800233a8aa91d1390ee488b9c` | `549e041dedd7992d362329791e77a8d7a89ea500af4a000b33d9cb11aa2208d3` |
| o | `858460553c9c8ed576a2aebfecdf5e62ac0e20fc246f1c219a81305ad418ce10` | `9979565b60aef4ed67420b55b5ae717c3cf985e804ebd01d28a247e75cdf7ecd` | `0aa1a1d3cad93521b1718eb9e68938f62b364fcfa3e6ba67967530609b136e62` |
| pipe | `59d152e92e7b50ceed5be631583cabab201ebebe1997127b1324fea9472d8ae7` | `172f2f31a9c7bac416f77fd4aa09906d650e88e6ab421953d8f26d1204a029e4` | `9d40b2d8623178cd6193291ee2e24b6fa5a489848c28b822b4e45f1abdaf438d` |
| plus | `936bcf685f509af872501793c332606b339b4d1bab3cb8e4f08590c0fa69eacb` | `acf4fe8579c31277ca027a596454f340a2b25befac0dee87508408aa59ee0fef` | `e9f2a43cb9480517c21b25a93336018fa7a2e3b18ddcd824bb9d915329b852f9` |

### Extended sensitivity proof

The same H-root sign-flip mutant was captured once at each new size after the
known-good set, without changing or tuning the mutation. The comparator uses
the original criterion: a pixel differs only when its maximum absolute RGBA
channel delta is strictly greater than `8` on the 0-255 scale.

| Cap height | Pixel-column difference | Maximum channel delta | Mutant SHA-256 |
| ---: | --- | ---: | --- |
| 12px | 10 differing pixels in 1/16 columns; `x=3` has 10/16 | 51 | `3071c992db947273c321a676a69f3ad35e972a59f77efe94c1ded3f03cab3d64` |
| 16px | 36 differing pixels in 3/21 columns; `x=4` has 13/21 | 170 | `8b1d860b35ebb5ae652e99cd46fb1d8921dc64ac945ecaa7c125336baac7f788` |
| 48px | 38 differing pixels in 1/64 columns; `x=12` has 38/64 | 204 | `4715861babbb3c4b271159f18ac1c8a1c8cd6cb6afb6f423fad35294c9406662` |

### 96px backward-compatibility proof

All 24 legacy commands were rerun with explicit `--cap-height 96` in the
pinned capture environment. Each regenerated PNG passed byte-for-byte `cmp`
against its committed counterpart, and recomputing SHA-256 produced exactly
the unchanged 24-entry manifest under "Known-good Lavapipe captures" above.
The existing 96px files and filenames were not modified.

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
