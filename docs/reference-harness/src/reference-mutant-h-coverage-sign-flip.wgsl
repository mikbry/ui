// INTENTIONAL SEEDED MUTANT: horizontal root-1 coverage sign flip.
// Reference WGSL transliteration of Eric Lengyel's Slug shaders.
// Upstream: https://github.com/EricLengyel/Slug
// Commit: be3c13eb7d63f9e8aa5c583e42d92c374cb91d98
// SPDX-License-Identifier: MIT
// Copyright 2017 Eric Lengyel.
// Modified by the mikbry/ui adapter author: HLSL mechanically transliterated to WGSL.
//
// Every executable statement cites its source HLSL line or is marked HARNESS.
// PROVENANCE.md records structural WGSL deviations.

const K_LOG_BAND_TEXTURE_WIDTH: u32 = 12u; // SlugPixelShader.hlsl:8-10

struct Params {
    slug_matrix: array<vec4<f32>, 4>, // SlugVertexShader.hlsl:65-68
    slug_viewport: vec4<f32>, // SlugVertexShader.hlsl:65-69
};

@group(0) @binding(0) var<uniform> params: Params; // SlugVertexShader.hlsl:65-69
@group(0) @binding(1) var curve_texture: texture_2d<f32>; // SlugPixelShader.hlsl:274
@group(0) @binding(2) var band_texture: texture_2d<u32>; // SlugPixelShader.hlsl:275

struct VertexInput {
    @location(0) pos: vec4<f32>, // SlugVertexShader.hlsl:8-17
    @location(1) tex: vec4<f32>, // SlugVertexShader.hlsl:19-33
    @location(2) jac: vec4<f32>, // SlugVertexShader.hlsl:35
    @location(3) bnd: vec4<f32>, // SlugVertexShader.hlsl:36
    @location(4) col: vec4<f32>, // SlugVertexShader.hlsl:37
    @builtin(vertex_index) vid: u32, // SlugVertexShader.hlsl:80 (unused upstream)
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>, // SlugVertexShader.hlsl:71-78
    @location(0) color: vec4<f32>, // SlugVertexShader.hlsl:74
    @location(1) texcoord: vec2<f32>, // SlugVertexShader.hlsl:75
    @location(2) @interpolate(flat) banding: vec4<f32>, // SlugVertexShader.hlsl:76
    @location(3) @interpolate(flat) glyph: vec4<i32>, // SlugVertexShader.hlsl:77
};

struct Unpacked {
    banding: vec4<f32>, // SlugVertexShader.hlsl:40-45
    glyph: vec4<i32>, // SlugVertexShader.hlsl:40-45
};

fn slug_unpack(tex: vec4<f32>, bnd: vec4<f32>) -> Unpacked { // SlugVertexShader.hlsl:40-45
    let g = vec2<u32>(bitcast<u32>(tex.z), bitcast<u32>(tex.w)); // SlugVertexShader.hlsl:42
    var result: Unpacked; // WGSL return-value replacement for HLSL out parameters
    result.glyph = vec4<i32>(i32(g.x & 0xffffu), i32(g.x >> 16u), i32(g.y & 0xffffu), i32(g.y >> 16u)); // SlugVertexShader.hlsl:43
    result.banding = bnd; // SlugVertexShader.hlsl:44
    return result; // SlugVertexShader.hlsl:40-45
}

struct Dilated {
    texcoord: vec2<f32>, // SlugVertexShader.hlsl:47-63
    position: vec2<f32>, // SlugVertexShader.hlsl:47-63
};

fn slug_dilate(
    pos: vec4<f32>,
    tex: vec4<f32>,
    jac: vec4<f32>,
    m0: vec4<f32>,
    m1: vec4<f32>,
    m3: vec4<f32>,
    dim: vec2<f32>,
) -> Dilated { // SlugVertexShader.hlsl:47-63
    let n = normalize(pos.zw); // SlugVertexShader.hlsl:49
    let s = dot(m3.xy, pos.xy) + m3.w; // SlugVertexShader.hlsl:50
    let t = dot(m3.xy, n); // SlugVertexShader.hlsl:51
    let u = (s * dot(m0.xy, n) - t * (dot(m0.xy, pos.xy) + m0.w)) * dim.x; // SlugVertexShader.hlsl:53
    let v = (s * dot(m1.xy, n) - t * (dot(m1.xy, pos.xy) + m1.w)) * dim.y; // SlugVertexShader.hlsl:54
    let s2 = s * s; // SlugVertexShader.hlsl:56
    let st = s * t; // SlugVertexShader.hlsl:57
    let uv = u * u + v * v; // SlugVertexShader.hlsl:58
    let d = pos.zw * (s2 * (st + sqrt(uv)) / (uv - st * st)); // SlugVertexShader.hlsl:59
    var result: Dilated; // WGSL return-value replacement for HLSL out parameter
    result.position = pos.xy + d; // SlugVertexShader.hlsl:61
    result.texcoord = vec2<f32>(tex.x + dot(d, jac.xy), tex.y + dot(d, jac.zw)); // SlugVertexShader.hlsl:62
    return result; // SlugVertexShader.hlsl:47-63
}

@vertex
fn vs_main(input: VertexInput) -> VertexOutput { // SlugVertexShader.hlsl:80-101
    let dilation = slug_dilate(input.pos, input.tex, input.jac, params.slug_matrix[0], params.slug_matrix[1], params.slug_matrix[3], params.slug_viewport.xy); // SlugVertexShader.hlsl:85-87
    let p = dilation.position; // SlugVertexShader.hlsl:82,87
    var result: VertexOutput; // SlugVertexShader.hlsl:83
    result.texcoord = dilation.texcoord; // SlugVertexShader.hlsl:87
    result.position.x = p.x * params.slug_matrix[0].x + p.y * params.slug_matrix[0].y + params.slug_matrix[0].w; // SlugVertexShader.hlsl:91
    result.position.y = p.x * params.slug_matrix[1].x + p.y * params.slug_matrix[1].y + params.slug_matrix[1].w; // SlugVertexShader.hlsl:92
    result.position.z = p.x * params.slug_matrix[2].x + p.y * params.slug_matrix[2].y + params.slug_matrix[2].w; // SlugVertexShader.hlsl:93
    result.position.w = p.x * params.slug_matrix[3].x + p.y * params.slug_matrix[3].y + params.slug_matrix[3].w; // SlugVertexShader.hlsl:94
    let unpacked = slug_unpack(input.tex, input.bnd); // SlugVertexShader.hlsl:96-98
    result.banding = unpacked.banding; // SlugVertexShader.hlsl:98
    result.glyph = unpacked.glyph; // SlugVertexShader.hlsl:98
    result.color = input.col; // SlugVertexShader.hlsl:99
    return result; // SlugVertexShader.hlsl:100
}

fn calc_root_code(y1: f32, y2: f32, y3: f32) -> u32 { // SlugPixelShader.hlsl:17-32
    let i1 = bitcast<u32>(y1) >> 31u; // SlugPixelShader.hlsl:22
    let i2 = bitcast<u32>(y2) >> 30u; // SlugPixelShader.hlsl:23
    let i3 = bitcast<u32>(y3) >> 29u; // SlugPixelShader.hlsl:24
    var shift = (i2 & 2u) | (i1 & ~2u); // SlugPixelShader.hlsl:26
    shift = (i3 & 4u) | (shift & ~4u); // SlugPixelShader.hlsl:27
    return (0x2e74u >> shift) & 0x0101u; // SlugPixelShader.hlsl:29-31
}

fn solve_horiz_poly(p12: vec4<f32>, p3: vec2<f32>) -> vec2<f32> { // SlugPixelShader.hlsl:34-62
    let a = p12.xy - p12.zw * 2.0 + p3; // SlugPixelShader.hlsl:46
    let b = p12.xy - p12.zw; // SlugPixelShader.hlsl:47
    let ra = 1.0 / a.y; // SlugPixelShader.hlsl:48
    let rb = 0.5 / b.y; // SlugPixelShader.hlsl:49
    let d = sqrt(max(b.y * b.y - a.y * p12.y, 0.0)); // SlugPixelShader.hlsl:51
    var t1 = (b.y - d) * ra; // SlugPixelShader.hlsl:52
    var t2 = (b.y + d) * ra; // SlugPixelShader.hlsl:53
    if abs(a.y) < 1.0 / 65536.0 { // SlugPixelShader.hlsl:55-57
        t1 = p12.y * rb; // SlugPixelShader.hlsl:57
        t2 = t1; // SlugPixelShader.hlsl:57
    }
    return vec2<f32>((a.x * t1 - b.x * 2.0) * t1 + p12.x, (a.x * t2 - b.x * 2.0) * t2 + p12.x); // SlugPixelShader.hlsl:59-61
}

fn solve_vert_poly(p12: vec4<f32>, p3: vec2<f32>) -> vec2<f32> { // SlugPixelShader.hlsl:64-84
    let a = p12.xy - p12.zw * 2.0 + p3; // SlugPixelShader.hlsl:68
    let b = p12.xy - p12.zw; // SlugPixelShader.hlsl:69
    let ra = 1.0 / a.x; // SlugPixelShader.hlsl:70
    let rb = 0.5 / b.x; // SlugPixelShader.hlsl:71
    let d = sqrt(max(b.x * b.x - a.x * p12.x, 0.0)); // SlugPixelShader.hlsl:73
    var t1 = (b.x - d) * ra; // SlugPixelShader.hlsl:74
    var t2 = (b.x + d) * ra; // SlugPixelShader.hlsl:75
    if abs(a.x) < 1.0 / 65536.0 { // SlugPixelShader.hlsl:77-79
        t1 = p12.x * rb; // SlugPixelShader.hlsl:79
        t2 = t1; // SlugPixelShader.hlsl:79
    }
    return vec2<f32>((a.y * t1 - b.y * 2.0) * t1 + p12.y, (a.y * t2 - b.y * 2.0) * t2 + p12.y); // SlugPixelShader.hlsl:81-83
}

fn calc_band_loc(glyph_loc: vec2<i32>, offset: u32) -> vec2<i32> { // SlugPixelShader.hlsl:86-94
    var band_loc = vec2<i32>(glyph_loc.x + i32(offset), glyph_loc.y); // SlugPixelShader.hlsl:90
    band_loc.y += band_loc.x >> K_LOG_BAND_TEXTURE_WIDTH; // SlugPixelShader.hlsl:91
    band_loc.x &= (1 << K_LOG_BAND_TEXTURE_WIDTH) - 1; // SlugPixelShader.hlsl:92
    return band_loc; // SlugPixelShader.hlsl:93
}

fn calc_coverage(xcov: f32, ycov: f32, xwgt: f32, ywgt: f32, flags: i32) -> f32 { // SlugPixelShader.hlsl:96-137
    _ = flags; // SlugPixelShader.hlsl:103-126; default upstream build omits SLUG_EVENODD
    var coverage = max(abs(xcov * xwgt + ycov * ywgt) / max(xwgt + ywgt, 1.0 / 65536.0), min(abs(xcov), abs(ycov))); // SlugPixelShader.hlsl:98-101
    coverage = clamp(coverage, 0.0, 1.0); // SlugPixelShader.hlsl:103-126, nonzero-fill default at 112-114
    return coverage; // SlugPixelShader.hlsl:128-136; default upstream build omits SLUG_WEIGHT
}

fn slug_render(render_coord: vec2<f32>, band_transform: vec4<f32>, glyph_data: vec4<i32>) -> f32 { // SlugPixelShader.hlsl:139-263
    let ems_per_pixel = fwidth(render_coord); // SlugPixelShader.hlsl:143-146
    let pixels_per_em = vec2<f32>(1.0) / ems_per_pixel; // SlugPixelShader.hlsl:147
    var band_max = glyph_data.zw; // SlugPixelShader.hlsl:149
    band_max.y &= 0x00ff; // SlugPixelShader.hlsl:150
    let unclamped_band_index = vec2<i32>(render_coord * band_transform.xy + band_transform.zw); // SlugPixelShader.hlsl:152-156
    let band_index = clamp(unclamped_band_index, vec2<i32>(0), band_max); // SlugPixelShader.hlsl:156
    let glyph_loc = glyph_data.xy; // SlugPixelShader.hlsl:157
    var xcov = 0.0; // SlugPixelShader.hlsl:159
    var xwgt = 0.0; // SlugPixelShader.hlsl:160
    let hband_data = textureLoad(band_texture, vec2<i32>(glyph_loc.x + band_index.y, glyph_loc.y), 0).xy; // SlugPixelShader.hlsl:162-166
    let hband_loc = calc_band_loc(glyph_loc, hband_data.y); // SlugPixelShader.hlsl:167
    for (var curve_index = 0; curve_index < i32(hband_data.x); curve_index += 1) { // SlugPixelShader.hlsl:169-172
        let curve_loc_u = textureLoad(band_texture, vec2<i32>(hband_loc.x + curve_index, hband_loc.y), 0).xy; // SlugPixelShader.hlsl:173-175
        let curve_loc = vec2<i32>(curve_loc_u); // SlugPixelShader.hlsl:175
        let p12 = textureLoad(curve_texture, curve_loc, 0) - vec4<f32>(render_coord, render_coord); // SlugPixelShader.hlsl:177-185
        let p3 = textureLoad(curve_texture, vec2<i32>(curve_loc.x + 1, curve_loc.y), 0).xy - render_coord; // SlugPixelShader.hlsl:186
        if max(max(p12.x, p12.z), p3.x) * pixels_per_em.x < -0.5 { // SlugPixelShader.hlsl:188-193
            break; // SlugPixelShader.hlsl:193
        }
        let code = calc_root_code(p12.y, p12.w, p3.y); // SlugPixelShader.hlsl:195
        if code != 0u { // SlugPixelShader.hlsl:196-197
            let r = solve_horiz_poly(p12, p3) * pixels_per_em.x; // SlugPixelShader.hlsl:198-201
            if (code & 1u) != 0u { // SlugPixelShader.hlsl:203-206
                xcov -= clamp(r.x + 0.5, 0.0, 1.0); // INTENTIONAL MUTANT of SlugPixelShader.hlsl:207 (root-1 sign flip)
                xwgt = max(xwgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); // SlugPixelShader.hlsl:208
            }
            if code > 1u { // SlugPixelShader.hlsl:211-212
                xcov -= clamp(r.y + 0.5, 0.0, 1.0); // SlugPixelShader.hlsl:213
                xwgt = max(xwgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); // SlugPixelShader.hlsl:214
            }
        }
    }
    var ycov = 0.0; // SlugPixelShader.hlsl:219
    var ywgt = 0.0; // SlugPixelShader.hlsl:220
    let vband_data = textureLoad(band_texture, vec2<i32>(glyph_loc.x + band_max.y + 1 + band_index.x, glyph_loc.y), 0).xy; // SlugPixelShader.hlsl:222-225
    let vband_loc = calc_band_loc(glyph_loc, vband_data.y); // SlugPixelShader.hlsl:226
    for (var curve_index = 0; curve_index < i32(vband_data.x); curve_index += 1) { // SlugPixelShader.hlsl:228-230
        let curve_loc_u = textureLoad(band_texture, vec2<i32>(vband_loc.x + curve_index, vband_loc.y), 0).xy; // SlugPixelShader.hlsl:232
        let curve_loc = vec2<i32>(curve_loc_u); // SlugPixelShader.hlsl:232
        let p12 = textureLoad(curve_texture, curve_loc, 0) - vec4<f32>(render_coord, render_coord); // SlugPixelShader.hlsl:233
        let p3 = textureLoad(curve_texture, vec2<i32>(curve_loc.x + 1, curve_loc.y), 0).xy - render_coord; // SlugPixelShader.hlsl:234
        if max(max(p12.y, p12.w), p3.y) * pixels_per_em.y < -0.5 { // SlugPixelShader.hlsl:236-241
            break; // SlugPixelShader.hlsl:241
        }
        let code = calc_root_code(p12.x, p12.z, p3.x); // SlugPixelShader.hlsl:243
        if code != 0u { // SlugPixelShader.hlsl:244-245
            let r = solve_vert_poly(p12, p3) * pixels_per_em.y; // SlugPixelShader.hlsl:246
            if (code & 1u) != 0u { // SlugPixelShader.hlsl:248-249
                ycov -= clamp(r.x + 0.5, 0.0, 1.0); // SlugPixelShader.hlsl:250
                ywgt = max(ywgt, clamp(1.0 - abs(r.x) * 2.0, 0.0, 1.0)); // SlugPixelShader.hlsl:251
            }
            if code > 1u { // SlugPixelShader.hlsl:254-255
                ycov += clamp(r.y + 0.5, 0.0, 1.0); // SlugPixelShader.hlsl:256
                ywgt = max(ywgt, clamp(1.0 - abs(r.y) * 2.0, 0.0, 1.0)); // SlugPixelShader.hlsl:257
            }
        }
    }
    return calc_coverage(xcov, ycov, xwgt, ywgt, glyph_data.w); // SlugPixelShader.hlsl:262
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> { // SlugPixelShader.hlsl:265-280
    let coverage = slug_render(input.texcoord, input.banding, input.glyph); // SlugPixelShader.hlsl:277-279
    return input.color * coverage; // SlugPixelShader.hlsl:280
}
