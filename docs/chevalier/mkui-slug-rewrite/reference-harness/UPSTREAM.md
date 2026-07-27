# Upstream sources and license

## Immutable Slug shader pin

- Repository: <https://github.com/EricLengyel/Slug>
- Commit:
  [`be3c13eb7d63f9e8aa5c583e42d92c374cb91d98`](https://github.com/EricLengyel/Slug/commit/be3c13eb7d63f9e8aa5c583e42d92c374cb91d98)
- Short pin: `be3c13e`

Files were read from a detached checkout at that exact commit:

| File | SHA-256 |
| --- | --- |
| `SlugPixelShader.hlsl` | `6eb2d77216aa6fd2bf92e0141a7fd54012340e6b3fdb4fff2c3a033d01719cf0` |
| `SlugVertexShader.hlsl` | `e24271022b152d09c4a1c9bfc6a8ddcae758c6cbe90d2a2276fc8b1f32976d11` |
| `README.md` | `e320f015a6b9d306feb74dd89296d2ecb1388544a1916588976dc23976604171` |
| `LICENSE` | `fa59b27dbd7045884b473e17874cb9d31426fb3c519d2278473dcabd606a4dfb` |
| `LICENSE-MIT` | `8b284b866415649e107444b72f35f6931ba2adcddef2febb83b25ed99b10f0f0` |
| `LICENSE-APACHE` | `c71d239df91726fc519c6eb72d318ec65820627232b2f796219e87dcf35d0ab4` |
| `NOTICE` | `37d1aff22dfd8feae3ec59fe279ede7f3309c4078ddc1f48662c7be48f26c628` |

The detached repository tree contains `SlugPixelShader.hlsl` and
`SlugVertexShader.hlsl`; these are the pixel and vertex sources used by the
port.

## Algorithm cross-checks

Eric Lengyel, ["GPU-Centered Font Rendering Directly from Glyph
Outlines"](https://jcgt.org/published/0006/02/02/), *Journal of Computer
Graphics Techniques* 6(2), 2017:

- Printed pp. 35-38: quadratic roots, sign-only root eligibility, the
  `0x2E74` lookup, and fractional coverage.
- Printed pp. 39-42: horizontal/vertical bands, descending maximum-coordinate
  sorting, early exits, texture packing, and shader inputs.
- Captured `paper.pdf` SHA-256:
  `6f6d693e6ed8ec3c788d475cbe961a89f41c990f4635f2af018a65fcf40a57b2`.

Eric Lengyel, ["A Decade of
Slug"](https://terathon.com/blog/decade-slug.html), dated **March 17, 2026**
(not 2027):

- "Rendering Evolution" states that root eligibility and winding remain
  essentially the 2017 method and that the band-split optimization was
  removed.
- The same section states that supersampling was removed.
- "Dynamic Dilation" states that dilation is recalculated by the vertex shader
  from the MVP matrix and viewport so the bounding polygon expands by half a
  pixel, and derives the formula used by the pinned vertex shader.
- Captured HTML SHA-256:
  `2c385308d1116421636021ac1bd0e115efca55dee99b7f7b96df179b2df72cbc`.

## mkui input-format source

The only mkui source authorized for consultation was
`crates/mkui-vector2d/src/slug.rs` at the worktree base commit `70e57c2`.
The public `SlugGlyph` record fields and its public little-endian serialization
define the committed `.slug` inputs. The harness does not link to that crate
and does not read or use `crates/mkui-vector2d-wgpu/`.

## License and attribution

The pinned repository is dual-licensed MIT or Apache-2.0. The pixel shader's
own header specifically names the MIT License, so this combined vertex/pixel
WGSL port selects **MIT**. The port retains Eric Lengyel's copyright and
attribution in both WGSL files. `LICENSE-UPSTREAM-MIT` reproduces the selected
license and `NOTICE-UPSTREAM` reproduces the upstream notice.

The upstream README also says the shader code may be used for any purpose with
credit and that the Slug patent has been dedicated to the public domain. This
file and the WGSL headers provide that credit; no claim is made beyond the
upstream license and dedication.
